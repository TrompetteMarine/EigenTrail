//! Independent reconstruction of grouping, spectra and regional view records.
use super::frames::{CheckedReference, VerifiedDate};
use eigentrail_core::{
    Error, Matrix, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    rolling_lineage::{
        Lens, Request, Uncertainty,
        report::{
            AxisSign, Enclosure, GroupRecord, GroupingCause, InformationLevel, Multiplicity,
            Reason, Reference, RegionState, ShareState, SpectrumState, ViewGroups,
        },
    },
    scalar::{self, Interval, interval, point},
};

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn range_error() -> Error {
    memory::text("checker rolling: scalar reconstruction exceeds finite range")
        .map(Error::Exhausted)
        .unwrap_or_else(|error| error)
}
fn bounded(value: Interval) -> Result<Interval> {
    if value.is_empty() || !value.inf().is_finite() || !value.sup().is_finite() {
        Err(range_error())
    } else {
        Ok(value)
    }
}
fn pair(value: Interval) -> [f64; 2] {
    [value.inf() + 0., value.sup() + 0.]
}

#[derive(Clone, Copy)]
struct Planar {
    centre: [f64; 2],
    population: [f64; 2],
    cutoff: [f64; 2],
    sign: AxisSign,
}
impl HeapBytes for Planar {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}

fn planar(root: &Matrix, rho: f64) -> Result<Planar> {
    let r =
        bounded(point(root.at(0, 1)) / point(root.at(0, 0)).sqrt() / point(root.at(1, 1)).sqrt())?;
    if r.inf() >= 1. || r.sup() <= -1. {
        return Err(invalid("checker rolling: non-SPD planar correlation"));
    }
    let r = interval(r.inf().max(-1.), r.sup().min(1.))?;
    let cut = scalar::tanh_nonnegative(point(rho))?;
    let product = bounded(r * cut)?;
    let denominator_low = bounded(point(1.) - product)?;
    let denominator_high = bounded(point(1.) + product)?;
    if denominator_low.inf() <= 0. || denominator_high.inf() <= 0. {
        return Err(range_error());
    }
    let low = bounded((r - cut) / denominator_low)?.inf().max(-1.);
    let high = bounded((r + cut) / denominator_high)?.sup().min(1.);
    if low > high {
        return Err(range_error());
    }
    Ok(Planar {
        centre: [r.inf(), r.sup()],
        population: [low, high],
        cutoff: [cut.inf(), cut.sup()],
        sign: if low > 0. {
            AxisSign::Positive
        } else if high < 0. {
            AxisSign::Negative
        } else {
            AxisSign::Unresolved
        },
    })
}

fn transferred(frame: CheckedReference<'_>, rho: f64) -> Result<Option<f64>> {
    let values = &frame.eigensystem.values;
    let error = bounded(point(frame.eigensystem.backward) + point(frame.matrix_error))?;
    let lower = bounded(point(values[values.len() - 1]) - point(error.sup()))?;
    let upper = bounded(point(values[0]) + point(error.sup()))?;
    if lower.inf() <= 0. {
        return Ok(None);
    }
    let sqrt_condition = bounded(upper.sqrt() / lower.sqrt())?;
    if rho == 0. {
        return Ok(Some(0.));
    }
    let quarter = bounded(point(rho) / point(4.))?;
    let (sinh, cosh) = scalar::sinh_cosh_nonnegative(quarter)?;
    let expm1_half = bounded(point(2.) * sinh * (cosh + sinh))?;
    let twice = bounded(point(2.) * point(rho))?;
    let t = bounded(scalar::exp_nonnegative(twice)? * sqrt_condition * expm1_half)?;
    if t.sup() >= 1. {
        return Ok(None);
    }
    let denominator = bounded(point(1.) - t)?;
    if denominator.inf() <= 0. {
        return Ok(None);
    }
    Ok(Some(
        bounded(point(rho) - point(2.) * scalar::ln(denominator)?)?.sup(),
    ))
}

/// None means a boundary cannot be retained. Range failure after proving the
/// exterior gap permits the genuine universal principal-angle bound.
fn radius(
    frame: CheckedReference<'_>,
    rank: [usize; 2],
    rho: f64,
    cancel: &Cancellation,
) -> Result<Option<[f64; 2]>> {
    let n = frame.eigensystem.values.len();
    if rank == [0, n] {
        return Ok(Some([0., 0.]));
    }
    let total = point(rho) + point(frame.numerical_rho);
    if !total.sup().is_finite() {
        return Ok(None);
    }
    let total = total.sup();
    let mut exterior: Option<Interval> = None;
    for cut in rank {
        cancel.poll()?;
        if cut == 0 || cut == n {
            continue;
        }
        let gap = bounded(
            scalar::ln(point(frame.eigensystem.values[cut - 1]))?
                - scalar::ln(point(frame.eigensystem.values[cut]))?,
        )?;
        exterior = Some(match exterior {
            None => gap,
            Some(previous) => {
                interval(previous.inf().min(gap.inf()), previous.sup().min(gap.sup()))?
            }
        });
    }
    let gap = exterior.ok_or_else(|| invalid("checker rolling: proper group lacks an exterior"))?;
    if gap.inf() <= (point(2.) * point(total)).sup() {
        return Ok(None);
    }
    if total == 0. {
        return Ok(Some([0., 0.]));
    }
    let result = memory::scope(|| {
        let half_rho = bounded(point(total) / point(2.))?;
        let half_gap = bounded((gap - point(total)) / point(2.))?;
        if half_gap.inf() <= 0. {
            return Err(range_error());
        }
        let numerator = scalar::sinh_cosh_nonnegative(half_rho)?.0;
        let denominator = scalar::sinh_cosh_nonnegative(half_gap)?.0;
        if denominator.inf() <= 0. {
            return Err(range_error());
        }
        let sine = bounded(numerator / denominator)?;
        let lo = sine.inf().max(0.);
        let hi = sine.sup().min(1.);
        if lo > hi {
            return Err(range_error());
        }
        let beta = scalar::asin(interval(lo, hi)?)?;
        Ok([beta.inf(), beta.sup()])
    });
    match result {
        Ok(beta) => Ok(Some(beta)),
        Err(Error::Exhausted(_)) => Ok(Some([0., Interval::FRAC_PI_2.sup()])),
        Err(error) => Err(error),
    }
}

struct Regime {
    record: RegionState,
    radius: Option<f64>,
    merge_all: bool,
}

fn regime(
    request: &Request,
    lens: Lens,
    frame: Option<CheckedReference<'_>>,
    rho: f64,
    planar: Option<Planar>,
    cancel: &Cancellation,
) -> Result<Regime> {
    cancel.poll()?;
    let Some(frame) = frame else {
        return Ok(Regime {
            record: RegionState::Unsupported {
                reason: Reason::NormalisationUnresolved,
            },
            radius: None,
            merge_all: true,
        });
    };
    if frame.midpoint.n == 2 && matches!(lens, Lens::Correlation | Lens::Partial) {
        let Some(planar) = planar else {
            return Ok(Regime {
                record: RegionState::Unsupported {
                    reason: Reason::UnsupportedTransfer,
                },
                radius: None,
                merge_all: true,
            });
        };
        return Ok(Regime {
            record: RegionState::Planar {
                centre_correlation: planar.centre,
                population_correlation: planar.population,
                cutoff: planar.cutoff,
                numerical_rho: frame.numerical_rho,
                sign: planar.sign,
            },
            radius: Some(0.),
            merge_all: planar.sign == AxisSign::Unresolved,
        });
    }
    let mapped = match request.uncertainty {
        Uncertainty::Point => Some(0.),
        Uncertainty::Conditional { .. } if matches!(lens, Lens::Covariance | Lens::Precision) => {
            Some(rho)
        }
        Uncertainty::Conditional { .. } => match memory::scope(|| transferred(frame, rho)) {
            Ok(mapped) => mapped,
            Err(Error::Exhausted(_)) => None,
            Err(error) => return Err(error),
        },
    };
    let Some(mapped) = mapped else {
        return Ok(Regime {
            record: RegionState::Unsupported {
                reason: Reason::UnsupportedTransfer,
            },
            radius: None,
            merge_all: true,
        });
    };
    let total = point(mapped) + point(frame.numerical_rho);
    if !total.sup().is_finite() {
        return Ok(Regime {
            record: RegionState::Unsupported {
                reason: Reason::NumericalUnresolved,
            },
            radius: None,
            merge_all: true,
        });
    }
    Ok(Regime {
        record: match request.uncertainty {
            Uncertainty::Point => RegionState::Point {
                numerical_rho: frame.numerical_rho,
            },
            Uncertainty::Conditional { .. } => RegionState::Conditional {
                mapped_rho: mapped,
                numerical_rho: frame.numerical_rho,
                total_rho: total.sup(),
            },
        },
        radius: Some(mapped),
        merge_all: false,
    })
}

fn display_centres(
    frame: CheckedReference<'_>,
    lens: Lens,
    reverse: bool,
    cancel: &Cancellation,
) -> Result<Vec<Interval>> {
    let n = frame.eigensystem.values.len();
    let mut out = memory::capacity(n)?;
    for i in 0..n {
        cancel.poll()?;
        let value = point(frame.eigensystem.values[if reverse { n - 1 - i } else { i }]);
        out.push(bounded(match lens {
            Lens::Precision => point(1.) / value,
            Lens::Partial if n > 2 => point(2.) - value,
            _ => value,
        })?);
    }
    Ok(out)
}

fn spectrum(
    frame: Option<CheckedReference<'_>>,
    lens: Lens,
    regime: &Regime,
    n: usize,
    reverse: bool,
    cancel: &Cancellation,
) -> Result<SpectrumState> {
    let produced = memory::scope(|| {
        let mut out = memory::capacity(n)?;
        if regime.radius.is_none() || frame.is_none() {
            if matches!(lens, Lens::Covariance | Lens::Precision) {
                return Ok(None);
            }
            let bounds = if n == 1 {
                [1., 1.]
            } else if lens == Lens::Partial && n > 2 {
                [2. - n as f64, 2.]
            } else {
                [0., n as f64]
            };
            for _ in 0..n {
                cancel.poll()?;
                out.push(bounds);
            }
            return Ok(Some(out));
        }
        let frame = frame.ok_or_else(|| invalid("checker rolling: missing spectral reference"))?;
        if let RegionState::Planar {
            population_correlation,
            ..
        } = regime.record
        {
            let absolute = interval(population_correlation[0], population_correlation[1])?.abs();
            for value in [point(1.) + absolute, point(1.) - absolute] {
                cancel.poll()?;
                let value = bounded(value)?.intersection(interval(0., 2.)?);
                if value.is_empty() {
                    return Err(range_error());
                }
                out.push(pair(value));
            }
            return Ok(Some(out));
        }
        if n == 1 && matches!(lens, Lens::Correlation | Lens::Partial) {
            out.push([1., 1.]);
            return Ok(Some(out));
        }
        let rho = match regime.record {
            RegionState::Point { numerical_rho } => numerical_rho,
            RegionState::Conditional { total_rho, .. } => total_rho,
            _ => return Err(invalid("checker rolling: missing spectral radius recipe")),
        };
        let exp = scalar::exp_nonnegative(point(rho))?;
        let band = interval((point(1.) / exp).inf(), exp.sup())?;
        for i in 0..n {
            cancel.poll()?;
            let target = if reverse { n - 1 - i } else { i };
            let positive = bounded(point(frame.eigensystem.values[target]) * band)?;
            let value = match lens {
                Lens::Precision => {
                    if positive.inf() <= 0. {
                        return Err(range_error());
                    }
                    point(1.) / positive
                }
                Lens::Partial if n > 2 => point(2.) - positive,
                _ => positive,
            };
            out.push(pair(bounded(value)?));
        }
        Ok(Some(out))
    });
    match produced {
        Ok(Some(intervals)) => Ok(SpectrumState::Available { intervals }),
        Ok(None) | Err(Error::Exhausted(_)) => Ok(SpectrumState::Unbounded {
            reason: Reason::NumericalUnresolved,
        }),
        Err(error) => Err(error),
    }
}

fn diagonal(input: &Matrix, cancel: &Cancellation) -> Result<Option<Vec<f64>>> {
    for column in 0..input.n {
        cancel.poll()?;
        for row in 0..column {
            if input.at(row, column) != 0. {
                return Ok(None);
            }
        }
    }
    let mut values = memory::capacity(input.n)?;
    for index in 0..input.n {
        cancel.poll()?;
        values.push(input.at(index, index));
    }
    values.sort_unstable_by(|a, b| b.total_cmp(a));
    Ok(Some(values))
}

fn multiplicity(
    lens: Lens,
    ranks: [usize; 2],
    columns: [usize; 2],
    diagonal: Option<&[f64]>,
) -> Multiplicity {
    let q = ranks[1] - ranks[0];
    if let Some(diagonal) = diagonal {
        if matches!(lens, Lens::Correlation | Lens::Partial) {
            if q == diagonal.len() {
                return Multiplicity::Exact { count: q };
            }
        } else {
            let value = diagonal[columns[0]];
            if diagonal[columns[0]..columns[1]]
                .iter()
                .all(|item| *item == value)
                && (columns[0] == 0 || diagonal[columns[0] - 1] != value)
                && (columns[1] == diagonal.len() || diagonal[columns[1]] != value)
            {
                return Multiplicity::Exact { count: q };
            }
        }
    }
    if q == 1 {
        Multiplicity::Exact { count: 1 }
    } else {
        Multiplicity::Unknown
    }
}

fn spectral_sum(
    state: &SpectrumState,
    rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<Option<Interval>> {
    let SpectrumState::Available { intervals } = state else {
        return Ok(None);
    };
    let mut result = point(0.);
    for value in &intervals[rank[0]..rank[1]] {
        cancel.poll()?;
        result += interval(value[0], value[1])?;
    }
    if result.is_empty() || !result.inf().is_finite() || !result.sup().is_finite() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

fn eligible(
    request: &Request,
    lens: Lens,
    rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<bool> {
    let Some(mask) = request
        .scope
        .rank_masks
        .iter()
        .find(|mask| mask.lens == lens)
    else {
        return Ok(false);
    };
    for ordinal in rank[0]..rank[1] {
        cancel.poll()?;
        if !mask
            .ranks
            .iter()
            .any(|range| range[0] <= ordinal && ordinal < range[1])
        {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Context<'a> {
    request: &'a Request,
    checked: &'a VerifiedDate<'a>,
    rho: f64,
    planar: Option<Planar>,
    diagonal: Option<&'a [f64]>,
}

impl Context<'_> {
    fn view(&self, lens: Lens, next: &mut usize, cancel: &Cancellation) -> Result<ViewGroups> {
        memory::scope(|| {
            let n = self.checked.root.midpoint.n;
            let (reference, reverse) = match lens {
                Lens::Covariance => (Reference::Root, false),
                Lens::Precision => (Reference::Root, true),
                Lens::Correlation => (Reference::Correlation, false),
                Lens::Partial if n <= 2 => (Reference::Correlation, false),
                Lens::Partial => (Reference::NormalisedPrecision, true),
            };
            let frame = self.checked.reference(reference);
            let mut regime = regime(self.request, lens, frame, self.rho, self.planar, cancel)?;
            let centres = match frame {
                Some(frame) => {
                    match memory::scope(|| display_centres(frame, lens, reverse, cancel)) {
                        Ok(values) => Some(values),
                        Err(Error::Exhausted(_)) => None,
                        Err(error) => return Err(error),
                    }
                }
                None => None,
            };
            if centres.is_none() && regime.radius.is_some() {
                regime = Regime {
                    record: RegionState::Unsupported {
                        reason: Reason::NumericalUnresolved,
                    },
                    radius: None,
                    merge_all: true,
                };
            }
            let spectrum = spectrum(frame, lens, &regime, n, reverse, cancel)?;
            let mut flags = memory::capacity(n.saturating_sub(1))?;
            if let (Some(frame), Some(centres), Some(mapped)) =
                (frame, centres.as_ref(), regime.radius)
            {
                let mut maximum = 0_f64;
                for &value in centres {
                    cancel.poll()?;
                    maximum = maximum.max(scalar::magnitude(value));
                }
                let threshold = point(self.request.grouping.rtol) * point(maximum);
                for cut in 1..n {
                    cancel.poll()?;
                    let numeric = (centres[cut - 1] - centres[cut]).inf() <= threshold.sup();
                    let reference_cut = if reverse { n - cut } else { cut };
                    let unresolved = regime.merge_all
                        || radius(frame, [0, reference_cut], mapped, cancel)?.is_none();
                    flags.push([numeric, unresolved]);
                }
            } else {
                for _ in 1..n {
                    cancel.poll()?;
                    flags.push([true, true]);
                }
            }
            let mut groups = memory::capacity(n)?;
            let mut first = 0;
            for last in 1..=n {
                cancel.poll()?;
                if last < n && (flags[last - 1][0] || flags[last - 1][1]) {
                    continue;
                }
                let rank = [first, last];
                let q = last - first;
                let columns = if reverse { [n - last, n - first] } else { rank };
                let mut numerical = false;
                let mut regional = false;
                for &[numeric, region] in &flags[first..last.saturating_sub(1)] {
                    cancel.poll()?;
                    numerical |= numeric;
                    regional |= region;
                }
                let grouping = if regime.radius.is_none() {
                    GroupingCause::Unsupported
                } else {
                    match (numerical, regional) {
                        (false, false) => GroupingCause::Simple,
                        (true, false) => GroupingCause::Numerical,
                        (false, true) => GroupingCause::Region,
                        (true, true) => GroupingCause::NumericalAndRegion,
                    }
                };
                let angular_radius_deg = if q == n {
                    0.
                } else {
                    let frame = frame
                        .ok_or_else(|| invalid("checker rolling: proper group lacks a frame"))?;
                    let mapped = regime
                        .radius
                        .ok_or_else(|| invalid("checker rolling: proper group lacks a region"))?;
                    let beta = radius(frame, columns, mapped, cancel)?
                        .ok_or_else(|| invalid("checker rolling: unmerged exterior boundary"))?;
                    let degrees = (interval(beta[0], beta[1])? * (point(180.) / Interval::PI))
                        .intersection(interval(0., 90.)?);
                    if degrees.is_empty() {
                        return Err(range_error());
                    }
                    bounded(degrees)?.sup() + 0.
                };
                let group_sum = spectral_sum(&spectrum, rank, cancel)?;
                let spectral_sum = if q == n && matches!(lens, Lens::Correlation | Lens::Partial) {
                    Enclosure::Bounds {
                        range: [n as f64, n as f64],
                    }
                } else {
                    match group_sum {
                        Some(value) => Enclosure::Bounds { range: pair(value) },
                        None => Enclosure::Unbounded {
                            reason: Reason::NumericalUnresolved,
                        },
                    }
                };
                let covariance_share = if lens != Lens::Covariance {
                    ShareState::NotApplicable
                } else if q == n {
                    ShareState::Covariance { range: [1., 1.] }
                } else {
                    let total = spectral_sum_value(&spectrum, n, cancel)?;
                    match (group_sum, total) {
                        (Some(numerator), Some(denominator)) if denominator.inf() > 0. => {
                            let share = (numerator / denominator).intersection(interval(0., 1.)?);
                            if share.is_empty()
                                || !share.inf().is_finite()
                                || !share.sup().is_finite()
                            {
                                return Err(range_error());
                            }
                            ShareState::Covariance { range: pair(share) }
                        }
                        _ => ShareState::Unavailable {
                            reason: Reason::NumericalUnresolved,
                        },
                    }
                };
                let node = *next;
                *next = next
                    .checked_add(1)
                    .ok_or_else(|| Error::Resource(String::new()))?;
                groups.push(GroupRecord {
                    node,
                    rank,
                    columns,
                    grouping,
                    multiplicity: multiplicity(lens, rank, columns, self.diagonal),
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
                    eligible: eligible(self.request, lens, rank, cancel)?,
                });
                first = last;
            }
            Ok(ViewGroups {
                subject: self.checked.subject,
                date: self.checked.date,
                lens,
                reference,
                spectrum,
                region: regime.record,
                groups,
            })
        })
    }
}

fn spectral_sum_value(
    state: &SpectrumState,
    n: usize,
    cancel: &Cancellation,
) -> Result<Option<Interval>> {
    spectral_sum(state, [0, n], cancel)
}

/// Reconstructs expected complete view records from independently checked
/// witnesses. No producer numerical, regional or view routine is invoked.
pub fn reconstruct_date(
    request: &Request,
    checked: &VerifiedDate<'_>,
    first_node: usize,
    cancel: &Cancellation,
) -> Result<Vec<ViewGroups>> {
    memory::scope(|| {
        cancel.poll()?;
        let rho = match request.uncertainty {
            Uncertainty::Point => 0.,
            Uncertainty::Conditional { rho, .. } => rho,
        };
        if !rho.is_finite()
            || rho < 0.
            || !request.grouping.rtol.is_finite()
            || request.grouping.rtol < 0.
        {
            return Err(invalid(
                "checker rolling: invalid regional/grouping request",
            ));
        }
        let n = checked.root.midpoint.n;
        let planar = if n == 2 && checked.correlation.is_some() {
            match memory::scope(|| planar(checked.root.midpoint, rho)) {
                Ok(value) => Some(value),
                Err(Error::Exhausted(_)) => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let diagonal = diagonal(checked.root.midpoint, cancel)?;
        let context = Context {
            request,
            checked,
            rho,
            planar,
            diagonal: diagonal.as_deref(),
        };
        let mut result = memory::capacity(request.lenses.len())?;
        let mut next = first_node;
        for &lens in &request.lenses {
            cancel.poll()?;
            result.push(context.view(lens, &mut next, cancel)?);
        }
        Ok(result)
    })
}
