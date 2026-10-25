//! Checked-centre proposals and outer summaries on the same continuous history family.
use crate::{
    families::{Configuration, Feasibility},
    resources::{Cancellation, memory},
    scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

pub const SCOPE: &str = "same-history continuous block summaries are marginal outer enclosures; complete competitor reachability is outer information, not realizability; only a checked whole affine chain certifies original or explicitly widened witness existence";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub scope: String,
    pub lineage: Vec<SegmentLineage>,
    pub queries: Vec<Query>,
    pub witnesses: Vec<QueryWitness>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentLineage {
    pub start: Vec<Origin>,
    pub end: Vec<Origin>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub origins: Vec<usize>,
    pub persistent: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub time: f64,
    pub centre: curves::CentreEvaluation,
    pub state: QueryState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum QueryState {
    Bounds {
        trace: Range,
        blocks: Vec<BlockSummary>,
    },
    InconsistentOuterBounds,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSummary {
    pub start: usize,
    pub end: usize,
    pub origins: Vec<usize>,
    pub persistent: bool,
    pub strength: Range,
    pub share: Quantity,
    pub rank: [usize; 2],
    pub projector_error: f64,
    pub local_displacement: Quantity,
    pub reference_displacement: Quantity,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryWitness {
    pub values: Vec<Matrix>,
    pub evaluation_errors: Vec<f64>,
    pub feasibility: Feasibility,
}

fn union(target: &mut Vec<usize>, source: &[usize]) -> Result<()> {
    for &origin in source {
        if !target.contains(&origin) {
            memory::push(target, origin)?;
        }
    }
    target.sort_unstable();
    Ok(())
}
fn clone_origin(origin: &Origin) -> Result<Origin> {
    Ok(Origin {
        origins: memory::copy(&origin.origins)?,
        persistent: origin.persistent,
    })
}

pub(crate) fn lineage_range(
    report: &TraceReport,
    first: usize,
    last: usize,
    cancel: &Cancellation,
) -> Result<Vec<SegmentLineage>> {
    memory::scope(|| {
        let mut result: Vec<SegmentLineage> = memory::capacity(last - first)?;
        for (j, segment) in report.segments.iter().enumerate().take(last).skip(first) {
            cancel.poll()?;
            let mut start = memory::capacity(segment.tubes.len())?;
            for (b, tube) in segment.tubes.iter().enumerate() {
                let mut origins = Vec::new();
                let mut persistent = false;
                if j == first {
                    memory::push(&mut origins, b)?;
                    persistent = true;
                } else {
                    let previous = &report.segments[j - 1].tubes;
                    let mut count = 0;
                    for (a, old) in previous.iter().enumerate() {
                        if old.start.max(tube.start) < old.end.min(tube.end) {
                            count += 1;
                            let predecessor = &result[j - first - 1].end[a];
                            union(&mut origins, &predecessor.origins)?;
                            persistent = predecessor.persistent
                                && old.start == tube.start
                                && old.end == tube.end;
                        }
                    }
                    persistent &= count == 1 && origins.len() == 1;
                }
                start.push(Origin {
                    origins,
                    persistent,
                });
            }
            let mut end = memory::capacity(segment.tubes.len())?;
            for b in 0..segment.tubes.len() {
                let mut origins = Vec::new();
                let mut persistent = false;
                if segment.association != Association::NoPerfectMatching {
                    for (a, neighbours) in segment.edges.iter().enumerate() {
                        if neighbours.contains(&b) {
                            union(&mut origins, &start[a].origins)?;
                        }
                    }
                    if let Association::Unique(matching) = &segment.association {
                        persistent = matching[b] == b && start[b].persistent;
                    }
                }
                end.push(Origin {
                    origins,
                    persistent,
                });
            }
            result.push(SegmentLineage { start, end });
        }
        Ok(result)
    })
}

fn query_origin(
    request: &TraceRequest,
    report: &TraceReport,
    lineage: &SegmentLineage,
    segment: usize,
    block: usize,
    time: f64,
) -> Result<Origin> {
    if report.segments[segment].association == Association::NoPerfectMatching {
        return Ok(Origin {
            origins: Vec::new(),
            persistent: false,
        });
    }
    if time == request.times[segment] {
        return clone_origin(&lineage.start[block]);
    }
    if time == request.times[segment + 1] {
        return clone_origin(&lineage.end[block]);
    }
    let mut origins = memory::copy(&lineage.start[block].origins)?;
    union(&mut origins, &lineage.end[block].origins)?;
    Ok(Origin {
        origins,
        persistent: lineage.start[block].persistent
            && lineage.end[block].persistent
            && lineage.start[block].origins == lineage.end[block].origins,
    })
}

fn spectral_error(p: usize, tube: &Tube, eta: f64, polar: f64) -> Result<f64> {
    if tube.start == 0 && tube.end == p {
        return Ok(0.);
    }
    if tube.gap <= upper(point(2.) * point(eta))? {
        return Ok(1.);
    }
    Ok(upper(
        point(2.) * point((tube.end - tube.start) as f64).sqrt() * point(eta) / point(tube.gap)
            + (point(2.) + point(polar)) * point(polar),
    )?
    .min(1.))
}

fn nodal_error(report: &TraceReport, date: usize, tube: &Tube) -> Result<f64> {
    let e = &report.eigensystems[date];
    spectral_error(
        e.values.len(),
        tube,
        upper(point(report.radii[date]) + point(e.backward))?,
        e.polar,
    )
}

fn displacement(
    first: &Eigensystem,
    second: &Eigensystem,
    tube: &Tube,
    error: f64,
    cancel: &Cancellation,
) -> Result<Quantity> {
    if tube.start == 0 && tube.end == first.values.len() {
        return Ok(Quantity::Interval(Range::closed(0., 0.)));
    }
    let p = first.values.len();
    let mut squared = point(0.);
    for row in 0..p {
        cancel.poll()?;
        for column in 0..p {
            let mut entry = point(0.);
            for k in tube.start..tube.end {
                entry += point(first.vectors.at(row, k)) * point(first.vectors.at(column, k))
                    - point(second.vectors.at(row, k)) * point(second.vectors.at(column, k));
            }
            squared += square(entry);
        }
    }
    let polar = (point(2.) + point(first.polar)) * point(first.polar)
        + (point(2.) + point(second.polar)) * point(second.polar);
    let lower = (squared.sqrt() / point(p as f64).sqrt() - polar - point(error))
        .inf()
        .max(0.);
    let upper = upper(squared.sqrt() + polar + point(error))?.min(1.);
    Ok(Quantity::Interval(Range::closed(lower, upper)))
}

fn unresolved() -> Result<Quantity> {
    Ok(Quantity::Undefined(memory::text(
        "identity unresolved across retained competitors or block merge/split",
    )?))
}

fn query_state(
    request: &TraceRequest,
    report: &TraceReport,
    lineage: &[SegmentLineage],
    first: usize,
    centre: &curves::CentreEvaluation,
    cancel: &Cancellation,
) -> Result<QueryState> {
    memory::scope(|| {
        let j = centre.segment;
        let p = request.features.len();
        let mut strengths = memory::capacity(p)?;
        let left = point(request.lipschitz) * (point(centre.time) - point(request.times[j]));
        let right = point(request.lipschitz) * (point(request.times[j + 1]) - point(centre.time));
        upper(left)?;
        upper(right)?;
        for k in 0..p {
            cancel.poll()?;
            let a = &report.summaries[j][k].strength;
            let b = &report.summaries[j + 1][k].strength;
            let lo = (point(a.lower) - left)
                .inf()
                .max((point(b.lower) - right).inf())
                .max(0.);
            let hi = upper(point(a.upper) + left)?.min(upper(point(b.upper) + right)?);
            if lo > hi {
                return Ok(QueryState::InconsistentOuterBounds);
            }
            strengths.push(Range::closed(lo, hi));
        }
        let lo = strengths.iter().fold(point(0.), |s, r| s + point(r.lower));
        let hi = strengths.iter().fold(point(0.), |s, r| s + point(r.upper));
        let trace = Range::closed(lo.inf().max(0.), upper(hi)?);
        let dt = point(request.times[j + 1]) - point(request.times[j]);
        let s = (point(centre.time) - point(request.times[j])) / dt;
        let eta = upper(
            (point(1.) - s) * point(report.radii[j])
                + s * point(report.radii[j + 1])
                + point(2.) * point(request.lipschitz) * dt * s * (point(1.) - s)
                + point(centre.evaluation_error)
                + point(centre.eigensystem.backward),
        )?;
        let segment = &report.segments[j];
        let mut blocks = memory::capacity(segment.tubes.len())?;
        for (b, tube) in segment.tubes.iter().enumerate() {
            cancel.poll()?;
            let lower = strengths[tube.start..tube.end]
                .iter()
                .fold(point(0.), |s, r| s + point(r.lower));
            let high = strengths[tube.start..tube.end]
                .iter()
                .fold(point(0.), |s, r| s + point(r.upper));
            let strength = Range::closed(lower.inf().max(0.), upper(high)?);
            let share = if trace.lower > 0. {
                Quantity::Interval(Range::closed(
                    (point(strength.lower) / point(trace.upper)).inf().max(0.),
                    upper(point(strength.upper) / point(trace.lower))?.min(1.),
                ))
            } else {
                Quantity::Undefined(memory::text("trace may be zero")?)
            };
            let origin = query_origin(request, report, &lineage[j - first], j, b, centre.time)?;
            let projector_error = spectral_error(p, tube, eta, centre.eigensystem.polar)?;
            let range_preserving = matches!(
                &segment.association,
                Association::Unique(matching) if matching[b] == b
            );
            let local_displacement = if centre.time == request.times[first] {
                Quantity::Interval(Range::closed(0., 0.))
            } else if range_preserving {
                displacement(
                    &report.eigensystems[j],
                    &centre.eigensystem,
                    tube,
                    upper(point(nodal_error(report, j, tube)?) + point(projector_error))?,
                    cancel,
                )?
            } else {
                unresolved()?
            };
            let reference_displacement = if centre.time == request.times[first] {
                Quantity::Interval(Range::closed(0., 0.))
            } else if origin.persistent && origin.origins.len() == 1 {
                let initial = &report.segments[first].tubes[origin.origins[0]];
                if initial.start == tube.start && initial.end == tube.end {
                    displacement(
                        &report.eigensystems[first],
                        &centre.eigensystem,
                        tube,
                        upper(
                            point(nodal_error(report, first, initial)?) + point(projector_error),
                        )?,
                        cancel,
                    )?
                } else {
                    unresolved()?
                }
            } else {
                unresolved()?
            };
            blocks.push(BlockSummary {
                start: tube.start,
                end: tube.end,
                origins: origin.origins,
                persistent: origin.persistent,
                strength,
                share,
                rank: [tube.start + 1, tube.end],
                projector_error,
                local_displacement,
                reference_displacement,
            });
        }
        Ok(QueryState::Bounds { trace, blocks })
    })
}

pub(crate) fn query_at(
    request: &TraceRequest,
    report: &TraceReport,
    lineage: &[SegmentLineage],
    first: usize,
    segment: usize,
    time: f64,
    cancel: &Cancellation,
) -> Result<Query> {
    memory::scope(|| {
        let centre = curves::evaluate(request, segment, time, cancel)?;
        let state = query_state(request, report, lineage, first, &centre, cancel)?;
        Ok(Query {
            time,
            centre,
            state,
        })
    })
}

fn interpolate(
    times: &[f64],
    nodes: &[Matrix],
    time: f64,
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    memory::scope(|| {
        let j = times.partition_point(|&t| t < time).saturating_sub(1);
        let j = j.min(times.len() - 2);
        let s = (point(time) - point(times[j])) / (point(times[j + 1]) - point(times[j]));
        let mut data = resources::zeros(nodes[j].data.len())?;
        let mut squared = point(0.);
        for (i, value) in data.iter_mut().enumerate() {
            if i % 256 == 0 {
                cancel.poll()?;
            }
            let exact = (point(1.) - s) * point(nodes[j].data[i]) + s * point(nodes[j + 1].data[i]);
            *value = exact.inf() / 2. + exact.sup() / 2.;
            if !value.is_finite() {
                return Err(Error::Exhausted("affine query witness overflow".into()));
            }
            squared += square(exact - point(*value));
        }
        Ok((
            Matrix {
                n: nodes[j].n,
                data,
            },
            upper(squared.sqrt())?,
        ))
    })
}

pub(crate) fn analyze(
    request: &TraceRequest,
    configuration: &Configuration,
    report: &TraceReport,
    query_times: &[f64],
    feasibility: &Feasibility,
    cancel: &Cancellation,
) -> Result<Workflow> {
    memory::scope(|| {
        let lineage = lineage_range(report, 0, report.segments.len(), cancel)?;
        let mut queries = memory::capacity(query_times.len())?;
        for &time in query_times {
            let query = memory::scope(|| {
                cancel.poll()?;
                let segment = request
                    .times
                    .partition_point(|&t| t < time)
                    .saturating_sub(1)
                    .min(request.times.len() - 2);
                let centre = curves::evaluate(request, segment, time, cancel)?;
                let state = query_state(request, report, &lineage, 0, &centre, cancel)?;
                Ok(Query {
                    time,
                    centre,
                    state,
                })
            })?;
            queries.push(query);
        }
        let mut witnesses = Vec::new();
        if matches!(
            feasibility,
            Feasibility::OriginalAffineWitness | Feasibility::WidenedAffineWitness { .. }
        ) {
            let witness = memory::scope(|| {
                let normalized = calibration::normalized(request)?;
                let nodes = configuration
                    .witnesses
                    .first()
                    .map_or(normalized.as_slice(), |w| w.nodes.as_slice());
                let mut values = memory::capacity(query_times.len())?;
                let mut evaluation_errors = memory::capacity(query_times.len())?;
                for &time in query_times {
                    let (value, error) = interpolate(&request.times, nodes, time, cancel)?;
                    values.push(value);
                    evaluation_errors.push(error);
                }
                Ok(QueryWitness {
                    values,
                    evaluation_errors,
                    feasibility: feasibility.clone(),
                })
            })?;
            memory::push(&mut witnesses, witness)?;
        }
        Ok(Workflow {
            scope: memory::text(SCOPE)?,
            lineage,
            queries,
            witnesses,
        })
    })
}
