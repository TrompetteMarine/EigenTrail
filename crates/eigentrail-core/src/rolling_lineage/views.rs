//! Dated four-view assembly. Graph eligibility never bypasses root-SPD checks.
use super::{
    Lens, Request, Slot, Uncertainty,
    numerical::{self, ReferenceFrame},
    regional::{self, GroupBound, PlanarBound, PlanarSign, TransferBound},
    report::{
        AxisSign, DateEvidence, Enclosure, FrameProof, FrameState, GroupRecord, GroupingCause,
        InformationLevel, InverseState, Multiplicity, Reason, Reference, RegionState, ShareState,
        SpectrumState, ViewGroups,
    },
};
use crate::{
    Error, Matrix, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    scalar::{self, Interval, interval, point},
};

pub struct PreparedDate {
    pub evidence: DateEvidence,
    pub views: Vec<ViewGroups>,
    pub next_node: usize,
}

impl HeapBytes for PreparedDate {
    fn heap_bytes(&self) -> Result<usize> {
        self.evidence
            .heap_bytes()?
            .checked_add(self.views.heap_bytes()?)
            .ok_or_else(|| Error::Resource(String::new()))
    }
}

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn exhausted(message: &str) -> Error {
    memory::text(message)
        .map(Error::Exhausted)
        .unwrap_or_else(|error| error)
}
fn finite(value: Interval) -> Result<Interval> {
    if value.is_empty() || !value.inf().is_finite() || !value.sup().is_finite() {
        Err(exhausted(
            "rolling view scalar enclosure exceeds finite range",
        ))
    } else {
        Ok(value)
    }
}
fn endpoints(value: Interval) -> [f64; 2] {
    [value.inf() + 0., value.sup() + 0.]
}
fn frame_proof(frame: ReferenceFrame) -> FrameProof {
    FrameProof {
        midpoint: frame.matrix,
        eigensystem: frame.eigensystem,
        matrix_error: frame.matrix_error,
        numerical_rho: frame.numerical_rho,
    }
}
fn frame_state(frame: Option<ReferenceFrame>, requested: bool) -> FrameState {
    match frame {
        Some(frame) => FrameState::Available {
            proof: frame_proof(frame),
        },
        None if requested => FrameState::Unavailable {
            reason: Reason::NormalisationUnresolved,
        },
        None => FrameState::NotRequested,
    }
}
fn optional_numerical<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::Exhausted(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn diagonal_values(root: &Matrix, cancel: &Cancellation) -> Result<Option<Vec<f64>>> {
    for j in 0..root.n {
        cancel.poll()?;
        for i in 0..j {
            if root.at(i, j) != 0. {
                return Ok(None);
            }
        }
    }
    let mut values = memory::capacity(root.n)?;
    for i in 0..root.n {
        cancel.poll()?;
        values.push(root.at(i, i));
    }
    values.sort_unstable_by(|left, right| right.total_cmp(left));
    Ok(Some(values))
}

struct Recipe {
    region: RegionState,
    mapped_rho: Option<f64>,
    force_whole: bool,
}

struct GroupContext<'a> {
    lens: Lens,
    reference: Option<&'a ReferenceFrame>,
    recipe: &'a Recipe,
    flags: &'a [[bool; 2]],
    spectrum: &'a SpectrumState,
}

struct Assembler<'a> {
    request: &'a Request,
    subject: usize,
    date: usize,
    root_rho: f64,
    root: &'a ReferenceFrame,
    correlation: Option<&'a ReferenceFrame>,
    normalised_precision: Option<&'a ReferenceFrame>,
    planar: Option<PlanarBound>,
    diagonal: Option<&'a [f64]>,
}

impl Assembler<'_> {
    fn reference(&self, lens: Lens) -> (Reference, Option<&ReferenceFrame>, bool) {
        match lens {
            Lens::Covariance => (Reference::Root, Some(self.root), false),
            Lens::Precision => (Reference::Root, Some(self.root), true),
            Lens::Correlation => (Reference::Correlation, self.correlation, false),
            // In one dimension both normalised views are the scalar identity too.
            Lens::Partial if self.root.matrix.n <= 2 => {
                (Reference::Correlation, self.correlation, false)
            }
            Lens::Partial => (
                Reference::NormalisedPrecision,
                self.normalised_precision,
                true,
            ),
        }
    }

    fn recipe(
        &self,
        lens: Lens,
        frame: Option<&ReferenceFrame>,
        cancel: &Cancellation,
    ) -> Result<Recipe> {
        let Some(frame) = frame else {
            return Ok(Recipe {
                region: RegionState::Unsupported {
                    reason: Reason::NormalisationUnresolved,
                },
                mapped_rho: None,
                force_whole: true,
            });
        };
        if self.root.matrix.n == 2 && matches!(lens, Lens::Correlation | Lens::Partial) {
            let Some(planar) = self.planar else {
                return Ok(Recipe {
                    region: RegionState::Unsupported {
                        reason: Reason::UnsupportedTransfer,
                    },
                    mapped_rho: None,
                    force_whole: true,
                });
            };
            let sign = match planar.sign {
                PlanarSign::Positive => AxisSign::Positive,
                PlanarSign::Negative => AxisSign::Negative,
                PlanarSign::Unresolved => AxisSign::Unresolved,
            };
            return Ok(Recipe {
                region: RegionState::Planar {
                    centre_correlation: planar.centre_correlation,
                    population_correlation: planar.population_correlation,
                    cutoff: planar.cutoff,
                    numerical_rho: frame.numerical_rho,
                    sign,
                },
                // The exact planar axes have statistical radius zero. Raw
                // eigenvectors still require their own numerical alignment.
                mapped_rho: Some(0.),
                force_whole: planar.sign == PlanarSign::Unresolved,
            });
        }
        let mapped = if matches!(self.request.uncertainty, Uncertainty::Point) {
            0.
        } else if matches!(lens, Lens::Covariance | Lens::Precision) {
            self.root_rho
        } else {
            match regional::normalisation_radius(frame, self.root_rho, cancel)? {
                TransferBound::Bound { rho, .. } => rho,
                TransferBound::Unsupported { .. } => {
                    return Ok(Recipe {
                        region: RegionState::Unsupported {
                            reason: Reason::UnsupportedTransfer,
                        },
                        mapped_rho: None,
                        force_whole: true,
                    });
                }
            }
        };
        let total = point(mapped) + point(frame.numerical_rho);
        if !total.sup().is_finite() {
            return Ok(Recipe {
                region: RegionState::Unsupported {
                    reason: Reason::NumericalUnresolved,
                },
                mapped_rho: None,
                force_whole: true,
            });
        }
        Ok(Recipe {
            region: match self.request.uncertainty {
                Uncertainty::Point => RegionState::Point {
                    numerical_rho: frame.numerical_rho,
                },
                Uncertainty::Conditional { .. } => RegionState::Conditional {
                    mapped_rho: mapped,
                    numerical_rho: frame.numerical_rho,
                    total_rho: total.sup(),
                },
            },
            mapped_rho: Some(mapped),
            force_whole: false,
        })
    }

    fn centres(
        &self,
        lens: Lens,
        frame: &ReferenceFrame,
        reverse: bool,
        cancel: &Cancellation,
    ) -> Result<Vec<Interval>> {
        let n = frame.eigensystem.values.len();
        let mut values = memory::capacity(n)?;
        for i in 0..n {
            cancel.poll()?;
            let rank = if reverse { n - 1 - i } else { i };
            let value = point(frame.eigensystem.values[rank]);
            values.push(finite(match lens {
                Lens::Precision => point(1.) / value,
                Lens::Partial if n > 2 => point(2.) - value,
                _ => value,
            })?);
        }
        Ok(values)
    }

    fn universal_spectrum(&self, lens: Lens, cancel: &Cancellation) -> Result<SpectrumState> {
        if matches!(lens, Lens::Covariance | Lens::Precision) {
            return Ok(SpectrumState::Unbounded {
                reason: Reason::NumericalUnresolved,
            });
        }
        let n = self.root.matrix.n;
        let range = if n == 1 {
            [1., 1.]
        } else if lens == Lens::Partial && n > 2 {
            [2. - n as f64, 2.]
        } else {
            [0., n as f64]
        };
        let mut intervals = memory::capacity(n)?;
        for _ in 0..n {
            cancel.poll()?;
            intervals.push(range);
        }
        Ok(SpectrumState::Available { intervals })
    }

    fn spectrum(
        &self,
        lens: Lens,
        frame: Option<&ReferenceFrame>,
        recipe: &Recipe,
        reverse: bool,
        cancel: &Cancellation,
    ) -> Result<SpectrumState> {
        if recipe.mapped_rho.is_none() {
            return self.universal_spectrum(lens, cancel);
        }
        let Some(frame) = frame else {
            return self.universal_spectrum(lens, cancel);
        };
        let produced = memory::scope(|| {
            let n = frame.eigensystem.values.len();
            let mut intervals = memory::capacity(n)?;
            if let RegionState::Planar {
                population_correlation,
                ..
            } = recipe.region
            {
                let q = interval(population_correlation[0], population_correlation[1])?.abs();
                for value in [point(1.) + q, point(1.) - q] {
                    cancel.poll()?;
                    let value = finite(value)?.intersection(interval(0., 2.)?);
                    if value.is_empty() {
                        return Err(exhausted("planar spectrum contradicts [0,2]"));
                    }
                    intervals.push(endpoints(value));
                }
                return Ok(intervals);
            }
            if n == 1 && matches!(lens, Lens::Correlation | Lens::Partial) {
                intervals.push([1., 1.]);
                return Ok(intervals);
            }
            let total = match recipe.region {
                RegionState::Point { numerical_rho } => numerical_rho,
                RegionState::Conditional { total_rho, .. } => total_rho,
                _ => return Err(invalid("missing rolling spectral radius recipe")),
            };
            let exp = scalar::exp_nonnegative(point(total))?;
            let band = interval((point(1.) / exp).inf(), exp.sup())?;
            for i in 0..n {
                cancel.poll()?;
                let rank = if reverse { n - 1 - i } else { i };
                let centre = point(frame.eigensystem.values[rank]);
                let positive = finite(centre * band)?;
                let value = match lens {
                    Lens::Precision => {
                        if positive.inf() <= 0. {
                            return Err(exhausted("precision spectral denominator unresolved"));
                        }
                        point(1.) / positive
                    }
                    Lens::Partial if n > 2 => point(2.) - positive,
                    _ => positive,
                };
                intervals.push(endpoints(finite(value)?));
            }
            Ok(intervals)
        });
        match produced {
            Ok(intervals) => Ok(SpectrumState::Available { intervals }),
            Err(Error::Exhausted(_)) => Ok(SpectrumState::Unbounded {
                reason: Reason::NumericalUnresolved,
            }),
            Err(error) => Err(error),
        }
    }

    fn boundaries(
        &self,
        frame: &ReferenceFrame,
        centres: &[Interval],
        reverse: bool,
        recipe: &Recipe,
        cancel: &Cancellation,
    ) -> Result<Vec<[bool; 2]>> {
        let n = centres.len();
        let mut maximum = 0_f64;
        for &value in centres {
            cancel.poll()?;
            maximum = maximum.max(scalar::magnitude(value));
        }
        let cutoff = point(self.request.grouping.rtol) * point(maximum);
        let mut flags = memory::capacity(n.saturating_sub(1))?;
        for boundary in 1..n {
            cancel.poll()?;
            let gap = centres[boundary - 1] - centres[boundary];
            let numerical = gap.inf() <= cutoff.sup();
            let reference_boundary = if reverse { n - boundary } else { boundary };
            let regional = recipe.force_whole
                || !matches!(
                    regional::group_bound(
                        frame,
                        0,
                        reference_boundary,
                        recipe.mapped_rho.unwrap_or(0.),
                        cancel
                    )?,
                    GroupBound::Separated { .. }
                );
            flags.push([numerical, regional]);
        }
        Ok(flags)
    }

    fn multiplicity(&self, lens: Lens, rank: [usize; 2], columns: [usize; 2]) -> Multiplicity {
        let q = rank[1] - rank[0];
        if let Some(values) = self.diagonal {
            if matches!(lens, Lens::Correlation | Lens::Partial) {
                if q == values.len() {
                    return Multiplicity::Exact { count: q };
                }
            } else {
                let value = values[columns[0]];
                if values[columns[0]..columns[1]]
                    .iter()
                    .all(|other| *other == value)
                    && (columns[0] == 0 || values[columns[0] - 1] != value)
                    && (columns[1] == values.len() || values[columns[1]] != value)
                {
                    return Multiplicity::Exact { count: q };
                }
            }
        }
        if q == 1 {
            // Every retained proper boundary has a strict checked separation.
            Multiplicity::Exact { count: 1 }
        } else {
            Multiplicity::Unknown
        }
    }

    fn group(
        &self,
        context: &GroupContext<'_>,
        rank: [usize; 2],
        next_node: &mut usize,
        cancel: &Cancellation,
    ) -> Result<GroupRecord> {
        let lens = context.lens;
        let reference = context.reference;
        let recipe = context.recipe;
        let flags = context.flags;
        let spectrum = context.spectrum;
        let n = self.root.matrix.n;
        let q = rank[1] - rank[0];
        let reversed = lens == Lens::Precision || lens == Lens::Partial && n > 2;
        let columns = if reversed {
            [n - rank[1], n - rank[0]]
        } else {
            rank
        };
        let unsupported = recipe.mapped_rho.is_none();
        let grouping = if unsupported {
            GroupingCause::Unsupported
        } else {
            let mut numerical = false;
            let mut region = false;
            for &[numerical_boundary, regional_boundary] in
                &flags[rank[0]..rank[1].saturating_sub(1)]
            {
                cancel.poll()?;
                numerical |= numerical_boundary;
                region |= regional_boundary;
            }
            match (numerical, region) {
                (false, false) => GroupingCause::Simple,
                (true, false) => GroupingCause::Numerical,
                (false, true) => GroupingCause::Region,
                (true, true) => GroupingCause::NumericalAndRegion,
            }
        };
        let angular_radius_deg = if q == n {
            0.
        } else {
            let frame =
                reference.ok_or_else(|| invalid("proper rolling group lost its reference"))?;
            let mapped = recipe
                .mapped_rho
                .ok_or_else(|| invalid("proper rolling group lost its region"))?;
            match regional::group_bound(frame, columns[0], columns[1], mapped, cancel)? {
                GroupBound::Separated { beta_radians, .. } => {
                    let degrees = (interval(beta_radians[0], beta_radians[1])?
                        * (point(180.) / Interval::PI))
                        .intersection(interval(0., 90.)?);
                    if degrees.is_empty() {
                        return Err(exhausted("rolling group angular radius outside [0,90]"));
                    }
                    finite(degrees)?.sup() + 0.
                }
                _ => {
                    return Err(exhausted(
                        "rolling partition retained an unresolved exterior boundary",
                    ));
                }
            }
        };
        let spectral_sum = if q == n && matches!(lens, Lens::Correlation | Lens::Partial) {
            // Both normalised views have exact unit diagonal, including signed P.
            Enclosure::Bounds {
                range: [n as f64, n as f64],
            }
        } else {
            spectral_sum(spectrum, rank, cancel)?
        };
        let covariance_share = if lens != Lens::Covariance {
            ShareState::NotApplicable
        } else if q == n {
            ShareState::Covariance { range: [1., 1.] }
        } else {
            covariance_share(spectrum, rank, cancel)?
        };
        let node = *next_node;
        *next_node = next_node
            .checked_add(1)
            .ok_or_else(|| Error::Resource(String::new()))?;
        Ok(GroupRecord {
            node,
            rank,
            columns,
            grouping,
            multiplicity: self.multiplicity(lens, rank, columns),
            information: if q == n {
                InformationLevel::WholeSpace
            } else if q == 1 {
                InformationLevel::Line
            } else {
                InformationLevel::ProperSubspace
            },
            angular_radius_deg,
            spectral_sum,
            covariance_share,
            eligible: self.request.scope.covers_group(lens, rank),
        })
    }

    fn view(&self, lens: Lens, next_node: &mut usize, cancel: &Cancellation) -> Result<ViewGroups> {
        memory::scope(|| {
            let n = self.root.matrix.n;
            let (reference, frame, reversed) = self.reference(lens);
            let mut recipe = self.recipe(lens, frame, cancel)?;
            let centres = match frame {
                Some(frame) => optional_numerical(self.centres(lens, frame, reversed, cancel))?,
                None => None,
            };
            if centres.is_none() && recipe.mapped_rho.is_some() {
                recipe = Recipe {
                    region: RegionState::Unsupported {
                        reason: Reason::NumericalUnresolved,
                    },
                    mapped_rho: None,
                    force_whole: true,
                };
            }
            let spectrum = self.spectrum(lens, frame, &recipe, reversed, cancel)?;
            let mut groups = memory::capacity(n)?;
            if let (Some(frame), Some(centres), Some(_)) =
                (frame, centres.as_ref(), recipe.mapped_rho)
            {
                let flags = self.boundaries(frame, centres, reversed, &recipe, cancel)?;
                let context = GroupContext {
                    lens,
                    reference: Some(frame),
                    recipe: &recipe,
                    flags: &flags,
                    spectrum: &spectrum,
                };
                let mut start = 0;
                for end in 1..=n {
                    cancel.poll()?;
                    if end == n || !flags[end - 1][0] && !flags[end - 1][1] {
                        groups.push(self.group(&context, [start, end], next_node, cancel)?);
                        start = end;
                    }
                }
            } else {
                groups.push(self.group(
                    &GroupContext {
                        lens,
                        reference: frame,
                        recipe: &recipe,
                        flags: &[],
                        spectrum: &spectrum,
                    },
                    [0, n],
                    next_node,
                    cancel,
                )?);
            }
            Ok(ViewGroups {
                subject: self.subject,
                date: self.date,
                lens,
                reference,
                spectrum,
                region: recipe.region,
                groups,
            })
        })
    }
}

fn sum(
    spectrum: &SpectrumState,
    rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<Option<Interval>> {
    let SpectrumState::Available { intervals } = spectrum else {
        return Ok(None);
    };
    let mut total = point(0.);
    for value in &intervals[rank[0]..rank[1]] {
        cancel.poll()?;
        total += interval(value[0], value[1])?;
    }
    if total.is_empty() || !total.inf().is_finite() || !total.sup().is_finite() {
        Ok(None)
    } else {
        Ok(Some(total))
    }
}
fn spectral_sum(
    spectrum: &SpectrumState,
    rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<Enclosure> {
    match sum(spectrum, rank, cancel)? {
        Some(value) => Ok(Enclosure::Bounds {
            range: endpoints(value),
        }),
        None => Ok(Enclosure::Unbounded {
            reason: Reason::NumericalUnresolved,
        }),
    }
}
fn covariance_share(
    spectrum: &SpectrumState,
    rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<ShareState> {
    let SpectrumState::Available { intervals } = spectrum else {
        return Ok(ShareState::Unavailable {
            reason: Reason::NumericalUnresolved,
        });
    };
    let numerator = sum(spectrum, rank, cancel)?;
    let denominator = sum(spectrum, [0, intervals.len()], cancel)?;
    let (Some(numerator), Some(denominator)) = (numerator, denominator) else {
        return Ok(ShareState::Unavailable {
            reason: Reason::NumericalUnresolved,
        });
    };
    if denominator.inf() <= 0. {
        return Ok(ShareState::Unavailable {
            reason: Reason::NumericalUnresolved,
        });
    }
    let share = (numerator / denominator).intersection(interval(0., 1.)?);
    if share.is_empty() || !share.inf().is_finite() || !share.sup().is_finite() {
        return Err(exhausted("rolling covariance share contradicts [0,1]"));
    }
    Ok(ShareState::Covariance {
        range: endpoints(share),
    })
}

/// Structural admission is performed once by the caller; root-SPD qualification
/// is still mandatory here for every observed slot, including an empty query.
pub fn prepare_date(
    request: &Request,
    subject: usize,
    date: usize,
    first_node: usize,
    cancel: &Cancellation,
) -> Result<Option<PreparedDate>> {
    memory::scope(|| {
        cancel.poll()?;
        let root_rho = match request.uncertainty {
            Uncertainty::Point => 0.,
            Uncertainty::Conditional { rho, .. } => rho,
        };
        if !root_rho.is_finite()
            || root_rho < 0.
            || !request.grouping.rtol.is_finite()
            || request.grouping.rtol < 0.
            || request.lenses.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid(
                "invalid rolling date radius, grouping or lens order",
            ));
        }
        if date >= request.dates.len() {
            return Err(invalid("rolling date index is absent"));
        }
        let slot = request
            .subjects
            .get(subject)
            .and_then(|person| person.slots.get(date))
            .ok_or_else(|| invalid("rolling subject or dated slot is absent"))?;
        let Slot::Observed { covariance } = slot else {
            return Ok(None);
        };
        if covariance.n != request.features.len() {
            return Err(invalid("rolling dated covariance has different features"));
        }
        let root = numerical::root_frame(covariance, cancel)?;
        let n = covariance.n;
        let diagonal = diagonal_values(covariance, cancel)?;
        let need_c = request.lenses.contains(&Lens::Correlation)
            || n <= 2 && request.lenses.contains(&Lens::Partial);
        let need_q = n > 2 && request.lenses.contains(&Lens::Partial);
        let correlation = if need_c {
            optional_numerical(numerical::correlation_frame(covariance, cancel))?
        } else {
            None
        };
        let (inverse, normalised_precision) = if need_q {
            match optional_numerical(numerical::normalised_precision_with_inverse(
                covariance, &root, cancel,
            ))? {
                Some((inverse, frame)) => (Some(inverse), Some(frame)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        let planar = if n == 2 && correlation.is_some() {
            optional_numerical(regional::planar_correlation(covariance, root_rho, cancel))?
        } else {
            None
        };
        let assembler = Assembler {
            request,
            subject,
            date,
            root_rho,
            root: &root,
            correlation: correlation.as_ref(),
            normalised_precision: normalised_precision.as_ref(),
            planar,
            diagonal: diagonal.as_deref(),
        };
        let mut views = memory::capacity(request.lenses.len())?;
        let mut next_node = first_node;
        for &lens in &request.lenses {
            cancel.poll()?;
            views.push(assembler.view(lens, &mut next_node, cancel)?);
        }
        let evidence = DateEvidence {
            subject,
            date,
            root: root.eigensystem,
            root_numerical_rho: root.numerical_rho,
            correlation: frame_state(correlation, need_c),
            inverse: match inverse {
                Some(inverse) => InverseState::Available {
                    midpoint: inverse.matrix,
                    operator_error: inverse.error,
                },
                None if need_q => InverseState::Unavailable {
                    reason: Reason::NormalisationUnresolved,
                },
                None => InverseState::NotRequested,
            },
            normalised_precision: frame_state(normalised_precision, need_q),
        };
        cancel.poll()?;
        Ok(Some(PreparedDate {
            evidence,
            views,
            next_node,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::super::{
        Chart, Correction, Date, Feature, Grouping, Limits, Precision, Projection, RankMask,
        Representation, SCHEMA, Scope, Subject,
    };
    use super::*;

    fn all_lenses() -> Vec<Lens> {
        vec![
            Lens::Covariance,
            Lens::Precision,
            Lens::Correlation,
            Lens::Partial,
        ]
    }
    fn request(covariance: Matrix, lenses: Vec<Lens>) -> Request {
        let n = covariance.n;
        Request {
            schema: SCHEMA.into(),
            chart: Chart {
                frame: "synthetic-unit-chart".into(),
                coordinates: "frame_coefficients".into(),
            },
            features: (0..n)
                .map(|i| Feature {
                    id: format!("feature{i}"),
                    unit: "one".into(),
                })
                .collect(),
            dates: vec![Date {
                id: "date0".into(),
                time: 0.,
            }],
            subjects: vec![Subject {
                id: "subject0".into(),
                slots: vec![Slot::Observed { covariance }],
            }],
            representation: Representation {
                symmetry: "exact".into(),
                correction: Correction::None,
            },
            grouping: Grouping { rtol: 1e-12 },
            tolerance_deg: 20.,
            uncertainty: Uncertainty::Point,
            scope: Scope {
                lens_travel: lenses
                    .iter()
                    .flat_map(|&left| lenses.iter().map(move |&right| [left, right]))
                    .collect(),
                rank_masks: lenses
                    .iter()
                    .map(|&lens| RankMask {
                        lens,
                        ranks: vec![[0, n]],
                    })
                    .collect(),
                max_lag: 1,
                missing: "skip_recorded".into(),
                policy: "draft_f_conservative".into(),
                certificate: "observed_dated_subspaces".into(),
                direct_scope: "all_later_slots".into(),
            },
            lenses,
            founders: vec![],
            projection: Projection::Counts,
            limits: Limits {
                max_candidates: 10000,
                max_edge_records: 10000,
                max_bytes: crate::resources::DEFAULT_BYTES,
                chunk_records: 64,
            },
            precision: Precision {
                format: "binary64".into(),
                angle_unit: "degrees".into(),
            },
        }
    }
    fn conditional(request: &mut Request, rho: f64) {
        request.uncertainty = Uncertainty::Conditional {
            joint_event: "supplied-joint-event".into(),
            provider: "test-input-not-a-sampling-level".into(),
            rho,
        };
    }
    fn intervals(view: &ViewGroups) -> &[[f64; 2]] {
        let SpectrumState::Available { intervals } = &view.spectrum else {
            panic!("finite spectral enclosure required")
        };
        intervals
    }
    fn indefinite_example() -> Matrix {
        Matrix {
            n: 3,
            data: vec![1., 0.8, 0., 0.8, 1., 0.5, 0., 0.5, 1.],
        }
    }

    #[test]
    fn reciprocal_ranks_planar_identity_and_deterministic_node_ids() {
        let input = request(
            Matrix {
                n: 2,
                data: vec![4., 0., 0., 1.],
            },
            all_lenses(),
        );
        let cancel = Cancellation::default();
        super::super::validate_structure(&input, &cancel).unwrap();
        let prepared = prepare_date(&input, 0, 0, 10, &cancel).unwrap().unwrap();
        assert_eq!(prepared.next_node, 16);
        assert_eq!(
            prepared
                .views
                .iter()
                .flat_map(|view| view.groups.iter().map(|g| g.node))
                .collect::<Vec<_>>(),
            vec![10, 11, 12, 13, 14, 15]
        );
        assert_eq!(prepared.views[0].groups[0].columns, [0, 1]);
        assert_eq!(prepared.views[1].groups[0].columns, [1, 2]);
        assert_eq!(prepared.views[1].reference, Reference::Root);
        assert!(
            intervals(&prepared.views[1])[0][0] <= 1. && intervals(&prepared.views[1])[0][1] >= 1.
        );
        let ShareState::Covariance { range } = prepared.views[0].groups[0].covariance_share else {
            panic!("covariance share required")
        };
        assert!(range[0] <= 0.8 && range[1] >= 0.8);
        assert!(matches!(
            prepared.evidence.inverse,
            InverseState::NotRequested
        ));
        assert!(matches!(
            prepared.evidence.normalised_precision,
            FrameState::NotRequested
        ));
        for view in &prepared.views[2..] {
            assert_eq!(view.reference, Reference::Correlation);
            assert_eq!(view.groups.len(), 1);
            assert_eq!(view.groups[0].rank, [0, 2]);
            assert_eq!(view.groups[0].information, InformationLevel::WholeSpace);
            assert_eq!(
                view.groups[0].multiplicity,
                Multiplicity::Exact { count: 2 }
            );
            assert_eq!(view.groups[0].angular_radius_deg, 0.);
            assert_eq!(view.groups[0].covariance_share, ShareState::NotApplicable);
            assert_eq!(intervals(view), [[1., 1.], [1., 1.]]);
        }
    }

    #[test]
    fn planar_population_strength_changes_without_statistical_axis_drift() {
        let mut input = request(
            Matrix {
                n: 2,
                data: vec![1., 0.5, 0.5, 1.],
            },
            all_lenses(),
        );
        conditional(&mut input, 0.1);
        let prepared = prepare_date(&input, 0, 0, 0, &Cancellation::default())
            .unwrap()
            .unwrap();
        let covariance = &prepared.views[0];
        let correlation = &prepared.views[2];
        let partial = &prepared.views[3];
        assert_eq!(correlation.groups.len(), 2);
        let RegionState::Planar {
            population_correlation,
            sign,
            ..
        } = correlation.region
        else {
            panic!("sharp planar region required")
        };
        assert_eq!(sign, AxisSign::Positive);
        let lower = (0.5_f64.atanh() - 0.1).tanh();
        let upper = (0.5_f64.atanh() + 0.1).tanh();
        assert!(population_correlation[0] <= lower && population_correlation[1] >= upper);
        let spectrum = intervals(correlation);
        assert!(spectrum[0][0] < 1.5 && spectrum[0][1] > 1.5);
        assert!(correlation.groups[0].angular_radius_deg < covariance.groups[0].angular_radius_deg);
        assert_eq!(correlation.spectrum, partial.spectrum);
        assert_eq!(correlation.region, partial.region);
        assert!(matches!(
            prepared.evidence.inverse,
            InverseState::NotRequested
        ));
    }

    #[test]
    fn signed_partial_spectrum_uses_checked_normalised_precision() {
        let input = request(indefinite_example(), all_lenses());
        let prepared = prepare_date(&input, 0, 0, 0, &Cancellation::default())
            .unwrap()
            .unwrap();
        let partial = &prepared.views[3];
        assert_eq!(partial.reference, Reference::NormalisedPrecision);
        assert_eq!(partial.groups.len(), 3);
        assert_eq!(partial.groups[0].columns, [2, 3]);
        assert!(intervals(partial)[2][1] < 0.);
        assert!(
            partial
                .groups
                .iter()
                .all(|group| group.covariance_share == ShareState::NotApplicable)
        );
        assert!(matches!(
            prepared.evidence.inverse,
            InverseState::Available { .. }
        ));
        assert!(matches!(
            prepared.evidence.normalised_precision,
            FrameState::Available { .. }
        ));
    }

    #[test]
    fn unsupported_transfer_keeps_whole_groups_and_universal_signed_bounds() {
        let mut input = request(indefinite_example(), all_lenses());
        conditional(&mut input, 1.);
        let prepared = prepare_date(&input, 0, 0, 0, &Cancellation::default())
            .unwrap()
            .unwrap();
        for view in &prepared.views[2..] {
            assert_eq!(
                view.region,
                RegionState::Unsupported {
                    reason: Reason::UnsupportedTransfer
                }
            );
            assert_eq!(view.groups.len(), 1);
            let group = &view.groups[0];
            assert_eq!(group.rank, [0, 3]);
            assert_eq!(group.information, InformationLevel::WholeSpace);
            assert_eq!(group.grouping, GroupingCause::Unsupported);
            assert_eq!(group.angular_radius_deg, 0.);
            assert_eq!(group.multiplicity, Multiplicity::Unknown);
            assert_eq!(group.spectral_sum, Enclosure::Bounds { range: [3., 3.] });
        }
        assert_eq!(intervals(&prepared.views[2]), [[0., 3.]; 3]);
        assert_eq!(intervals(&prepared.views[3]), [[-1., 2.]; 3]);
    }

    #[test]
    fn near_tie_is_not_exact_and_masks_do_not_split_its_projector() {
        let mut input = request(
            Matrix {
                n: 3,
                data: vec![2., 0., 0., 0., 2. - 1e-10, 0., 0., 0., 1.],
            },
            vec![Lens::Covariance],
        );
        input.grouping.rtol = 1e-8;
        input.scope.rank_masks[0].ranks = vec![[0, 1]];
        let prepared = prepare_date(&input, 0, 0, 0, &Cancellation::default())
            .unwrap()
            .unwrap();
        assert_eq!(prepared.views[0].groups.len(), 2);
        let group = &prepared.views[0].groups[0];
        assert_eq!(group.rank, [0, 2]);
        assert_eq!(group.columns, [0, 2]);
        assert_eq!(group.grouping, GroupingCause::Numerical);
        assert_eq!(group.multiplicity, Multiplicity::Unknown);
        assert!(!group.eligible);
    }

    #[test]
    fn managed_normalisation_retains_and_releases_every_owned_buffer() {
        let input = request(indefinite_example(), all_lenses());
        let operation = memory::Operation::new(crate::resources::DEFAULT_BYTES).unwrap();
        let _input_guard = operation.external(input.heap_bytes().unwrap()).unwrap();
        let baseline = operation.stats().live;
        let prepared = operation
            .own(|| prepare_date(&input, 0, 0, 0, &Cancellation::default()))
            .unwrap();
        assert_eq!(prepared.as_ref().unwrap().views.len(), 4);
        assert!(operation.stats().peak <= operation.stats().limit);
        drop(prepared);
        assert_eq!(operation.stats().live, baseline);
    }

    #[test]
    fn empty_queries_still_check_spd_and_missing_slots_remain_explicit() {
        let cancel = Cancellation::default();
        let bad = request(
            Matrix {
                n: 2,
                data: vec![-1., 0., 0., 1.],
            },
            vec![],
        );
        assert!(matches!(
            prepare_date(&bad, 0, 0, 0, &cancel),
            Err(Error::Invalid(_))
        ));
        let mut input = request(
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 1.],
            },
            vec![],
        );
        let empty = prepare_date(&input, 0, 0, 7, &cancel).unwrap().unwrap();
        assert!(empty.views.is_empty());
        assert_eq!(empty.next_node, 7);
        input.subjects[0].slots[0] = Slot::Missing {
            reason: "explicitly not observed".into(),
        };
        assert!(prepare_date(&input, 0, 0, 0, &cancel).unwrap().is_none());
        let full = request(
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 1.],
            },
            vec![Lens::Covariance],
        );
        assert!(matches!(
            prepare_date(&full, 0, 0, usize::MAX, &cancel),
            Err(Error::Resource(_))
        ));
        let operation = memory::Operation::new(8192).unwrap();
        assert!(matches!(
            operation.run(|| prepare_date(&full, 0, 0, 0, &cancel)),
            Err(Error::Resource(_))
        ));
        cancel.cancel();
        assert!(matches!(
            prepare_date(&full, 0, 0, 0, &cancel),
            Err(Error::Cancelled)
        ));
    }
}
