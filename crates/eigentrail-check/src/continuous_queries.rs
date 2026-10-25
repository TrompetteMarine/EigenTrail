//! Independent reconstruction of continuous history lineage and query claims.
use eigentrail_core::{
    continuous_queries::{Origin, Query, QueryState, SCOPE, SegmentLineage, Workflow},
    families::{Configuration, Feasibility},
    resources::{Cancellation, memory},
    scalar::*,
    *,
};

fn bad() -> Error {
    Error::Invalid("continuous history checker: scope, lineage, query or witness claim".into())
}

fn append_origins(target: &mut Vec<usize>, source: &[usize]) -> Result<()> {
    for &value in source {
        if !target.contains(&value) {
            memory::push(target, value)?;
        }
    }
    target.sort_unstable();
    Ok(())
}

pub(crate) fn derive_lineage_range(
    report: &TraceReport,
    first: usize,
    last: usize,
    cancel: &Cancellation,
) -> Result<Vec<SegmentLineage>> {
    memory::scope(|| {
        if first >= last || last > report.segments.len() {
            return Err(bad());
        }
        let mut result: Vec<SegmentLineage> = memory::capacity(last - first)?;
        for cell in first..last {
            let segment = &report.segments[cell];
            cancel.poll()?;
            let row = memory::scope(|| {
                let mut start = memory::capacity(segment.tubes.len())?;
                for (index, block) in segment.tubes.iter().enumerate() {
                    let mut origins = Vec::new();
                    let mut persistent = cell == first;
                    if cell == first {
                        memory::push(&mut origins, index)?;
                    } else {
                        let previous = &report.segments[cell - 1];
                        let mut contributors = 0;
                        for (old_index, old) in previous.tubes.iter().enumerate() {
                            if old.start.max(block.start) < old.end.min(block.end) {
                                contributors += 1;
                                let predecessor = &result[cell - first - 1].end[old_index];
                                append_origins(&mut origins, &predecessor.origins)?;
                                persistent = predecessor.persistent
                                    && old.start == block.start
                                    && old.end == block.end;
                            }
                        }
                        persistent &= contributors == 1 && origins.len() == 1;
                    }
                    start.push(Origin {
                        origins,
                        persistent,
                    });
                }
                let mut end = memory::capacity(segment.tubes.len())?;
                for (target, block) in segment.tubes.iter().enumerate() {
                    let mut origins = Vec::new();
                    let mut persistent = false;
                    if segment.association != Association::NoPerfectMatching {
                        for (source, edges) in segment.edges.iter().enumerate() {
                            if edges.contains(&target) {
                                append_origins(&mut origins, &start[source].origins)?;
                            }
                        }
                        if let Association::Unique(matching) = &segment.association {
                            let source = matching
                                .iter()
                                .position(|&value| value == target)
                                .ok_or_else(bad)?;
                            let old = &segment.tubes[source];
                            persistent = start[source].persistent
                                && old.start == block.start
                                && old.end == block.end;
                        }
                    }
                    end.push(Origin {
                        origins,
                        persistent,
                    });
                }
                Ok(SegmentLineage { start, end })
            })?;
            result.push(row);
        }
        Ok(result)
    })
}

fn cell_at(request: &TraceRequest, time: f64) -> usize {
    request
        .times
        .partition_point(|&node| node < time)
        .saturating_sub(1)
        .min(request.times.len() - 2)
}

fn origin_at(
    request: &TraceRequest,
    report: &TraceReport,
    cell: usize,
    block: usize,
    time: f64,
    row: &SegmentLineage,
) -> Result<Origin> {
    if report.segments[cell].association == Association::NoPerfectMatching {
        return Ok(Origin {
            origins: Vec::new(),
            persistent: false,
        });
    }
    let start = &row.start[block];
    let end = &row.end[block];
    let source = if time == request.times[cell] {
        Some(start)
    } else if time == request.times[cell + 1] {
        Some(end)
    } else {
        None
    };
    if let Some(source) = source {
        return Ok(Origin {
            origins: memory::copy(&source.origins)?,
            persistent: source.persistent,
        });
    }
    let mut origins = memory::copy(&start.origins)?;
    append_origins(&mut origins, &end.origins)?;
    Ok(Origin {
        origins,
        persistent: start.persistent && end.persistent && start.origins == end.origins,
    })
}

fn matches_range(actual: &Range, lower: f64, upper: f64) -> bool {
    actual.lower.is_finite()
        && actual.upper.is_finite()
        && actual.lower_closed
        && actual.upper_closed
        && actual.lower >= 0.
        && actual.lower == lower
        && actual.upper == upper
        && actual.lower <= actual.upper
}

fn spectral_error(p: usize, block: &Tube, noise: f64, polar: f64) -> Result<f64> {
    if block.start == 0 && block.end == p {
        return Ok(0.);
    }
    if block.gap <= upper(point(2.) * point(noise))? {
        return Ok(1.);
    }
    upper(
        point(2.) * point((block.end - block.start) as f64).sqrt() * point(noise)
            / point(block.gap)
            + (point(2.) + point(polar)) * point(polar),
    )
    .map(|bound| bound.min(1.))
}

fn nodal_error(report: &TraceReport, date: usize, block: &Tube) -> Result<f64> {
    let eigen = &report.eigensystems[date];
    spectral_error(
        eigen.values.len(),
        block,
        upper(point(report.radii[date]) + point(eigen.backward))?,
        eigen.polar,
    )
}

fn displacement(
    a: &Eigensystem,
    b: &Eigensystem,
    block: &Tube,
    uncertainty: f64,
    cancel: &Cancellation,
) -> Result<Range> {
    let p = a.vectors.n;
    if block.start == 0 && block.end == p {
        return Ok(Range::closed(0., 0.));
    }
    let mut squared = point(0.);
    for row in 0..p {
        cancel.poll()?;
        for col in 0..p {
            let mut difference = point(0.);
            for axis in block.start..block.end {
                difference += point(a.vectors.at(row, axis)) * point(a.vectors.at(col, axis))
                    - point(b.vectors.at(row, axis)) * point(b.vectors.at(col, axis));
            }
            squared += square(difference);
        }
    }
    let polar = (point(2.) + point(a.polar)) * point(a.polar)
        + (point(2.) + point(b.polar)) * point(b.polar);
    let lower = (squared.sqrt() / point(p as f64).sqrt() - polar - point(uncertainty))
        .inf()
        .max(0.);
    let upper = upper(squared.sqrt() + polar + point(uncertainty))?.min(1.);
    Ok(Range::closed(lower, upper))
}

fn check_quantity(actual: &Quantity, expected: Option<&Range>) -> Result<()> {
    match (actual, expected) {
        (Quantity::Interval(actual), Some(expected))
            if actual.upper <= 1. && matches_range(actual, expected.lower, expected.upper) =>
        {
            Ok(())
        }
        (Quantity::Undefined(reason), None) if !reason.is_empty() && reason.len() <= 256 => Ok(()),
        _ => Err(bad()),
    }
}

pub(crate) fn check_component_query(
    request: &TraceRequest,
    report: &TraceReport,
    lineage: &[SegmentLineage],
    cells: [usize; 2],
    time: f64,
    query: &Query,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        let [first, last] = cells;
        cancel.poll()?;
        let cell = cell_at(request, time);
        if first >= last
            || last > report.segments.len()
            || lineage.len() != last - first
            || cell < first
            || cell >= last
        {
            return Err(bad());
        }
        if query.time.to_bits() != time.to_bits()
            || query.centre.time.to_bits() != time.to_bits()
            || query.centre.segment != cell
        {
            return Err(bad());
        }
        crate::curves::check_evaluation(request, &query.centre, cancel)?;
        let mut strengths = memory::capacity(request.features.len())?;
        let mut inconsistent = false;
        let left_drift = point(request.lipschitz) * (point(time) - point(request.times[cell]));
        let right_drift = point(request.lipschitz) * (point(request.times[cell + 1]) - point(time));
        for (left, right) in report.summaries[cell]
            .iter()
            .zip(&report.summaries[cell + 1])
        {
            let lower = (point(left.strength.lower) - left_drift)
                .inf()
                .max((point(right.strength.lower) - right_drift).inf())
                .max(0.);
            let high = upper(point(left.strength.upper) + left_drift)?
                .min(upper(point(right.strength.upper) + right_drift)?);
            inconsistent |= lower > high;
            if inconsistent {
                break;
            }
            strengths.push(Range::closed(lower, high));
        }
        if inconsistent {
            return if query.state == QueryState::InconsistentOuterBounds {
                Ok(())
            } else {
                Err(bad())
            };
        }
        let QueryState::Bounds { trace, blocks } = &query.state else {
            return Err(bad());
        };
        let trlo = strengths
            .iter()
            .fold(point(0.), |sum, value| sum + point(value.lower))
            .inf()
            .max(0.);
        let trhi = upper(
            strengths
                .iter()
                .fold(point(0.), |sum, value| sum + point(value.upper)),
        )?;
        if !matches_range(trace, trlo, trhi) || blocks.len() != report.segments[cell].tubes.len() {
            return Err(bad());
        }
        let dt = point(request.times[cell + 1]) - point(request.times[cell]);
        let fraction = (point(time) - point(request.times[cell])) / dt;
        let noise = upper(
            (point(1.) - fraction) * point(report.radii[cell])
                + fraction * point(report.radii[cell + 1])
                + point(2.) * point(request.lipschitz) * dt * fraction * (point(1.) - fraction)
                + point(query.centre.evaluation_error)
                + point(query.centre.eigensystem.backward),
        )?;
        for (index, (block, tube)) in blocks.iter().zip(&report.segments[cell].tubes).enumerate() {
            cancel.poll()?;
            let origin = origin_at(request, report, cell, index, time, &lineage[cell - first])?;
            if block.start != tube.start
                || block.end != tube.end
                || block.rank != [tube.start + 1, tube.end]
                || block.origins != origin.origins
                || block.persistent != origin.persistent
            {
                return Err(bad());
            }
            let lower = strengths[tube.start..tube.end]
                .iter()
                .fold(point(0.), |sum, value| sum + point(value.lower))
                .inf()
                .max(0.);
            let high = upper(
                strengths[tube.start..tube.end]
                    .iter()
                    .fold(point(0.), |sum, value| sum + point(value.upper)),
            )?;
            if !matches_range(&block.strength, lower, high) {
                return Err(bad());
            }
            let share = if trace.lower > 0. {
                Some(Range::closed(
                    (point(block.strength.lower) / point(trace.upper))
                        .inf()
                        .max(0.),
                    upper(point(block.strength.upper) / point(trace.lower))?.min(1.),
                ))
            } else {
                None
            };
            check_quantity(&block.share, share.as_ref())?;
            let projector = spectral_error(
                request.features.len(),
                tube,
                noise,
                query.centre.eigensystem.polar,
            )?;
            if !block.projector_error.is_finite() || block.projector_error != projector {
                return Err(bad());
            }
            let first_time = time == request.times[first];
            let local_matching = matches!(&report.segments[cell].association,
                Association::Unique(matching) if matching[index] == index);
            let local = if first_time {
                Some(Range::closed(0., 0.))
            } else if local_matching {
                Some(displacement(
                    &report.eigensystems[cell],
                    &query.centre.eigensystem,
                    tube,
                    upper(point(nodal_error(report, cell, tube)?) + point(block.projector_error))?,
                    cancel,
                )?)
            } else {
                None
            };
            check_quantity(&block.local_displacement, local.as_ref())?;
            let reference = if first_time {
                Some(Range::closed(0., 0.))
            } else if origin.persistent && origin.origins.len() == 1 {
                let initial = &report.segments[first].tubes[origin.origins[0]];
                if initial.start == tube.start && initial.end == tube.end {
                    Some(displacement(
                        &report.eigensystems[first],
                        &query.centre.eigensystem,
                        tube,
                        upper(
                            point(nodal_error(report, first, initial)?)
                                + point(block.projector_error),
                        )?,
                        cancel,
                    )?)
                } else {
                    None
                }
            } else {
                None
            };
            check_quantity(&block.reference_displacement, reference.as_ref())?;
        }
        Ok(())
    })
}

fn check_queries(
    request: &TraceRequest,
    report: &TraceReport,
    times: &[f64],
    lineage: &[SegmentLineage],
    workflow: &Workflow,
    cancel: &Cancellation,
) -> Result<()> {
    if workflow.queries.len() != times.len() {
        return Err(bad());
    }
    for (&time, query) in times.iter().zip(&workflow.queries) {
        check_component_query(
            request,
            report,
            lineage,
            [0, report.segments.len()],
            time,
            query,
            cancel,
        )?;
    }
    Ok(())
}

fn check_witnesses(
    request: &TraceRequest,
    configuration: &Configuration,
    times: &[f64],
    feasibility: &Feasibility,
    workflow: &Workflow,
    cancel: &Cancellation,
) -> Result<()> {
    let verified = matches!(
        feasibility,
        Feasibility::OriginalAffineWitness | Feasibility::WidenedAffineWitness { .. }
    );
    if workflow.witnesses.len() != usize::from(verified) {
        return Err(bad());
    }
    if !verified {
        return Ok(());
    }
    memory::scope(|| {
        let observed = calibration::normalized(request)?;
        let nodes = configuration
            .witnesses
            .first()
            .map_or(observed.as_slice(), |value| value.nodes.as_slice());
        let witness = &workflow.witnesses[0];
        if witness.feasibility != *feasibility
            || witness.values.len() != times.len()
            || witness.evaluation_errors.len() != times.len()
        {
            return Err(bad());
        }
        for ((&time, matrix), &error) in times
            .iter()
            .zip(&witness.values)
            .zip(&witness.evaluation_errors)
        {
            cancel.poll()?;
            numerics::validate_matrix(matrix)?;
            if matrix.n != request.features.len() || !error.is_finite() || error < 0. {
                return Err(bad());
            }
            let cell = cell_at(request, time);
            let fraction = (point(time) - point(request.times[cell]))
                / (point(request.times[cell + 1]) - point(request.times[cell]));
            let mut squared = point(0.);
            for (index, &value) in matrix.data.iter().enumerate() {
                if index % 256 == 0 {
                    cancel.poll()?;
                }
                let exact = (point(1.) - fraction) * point(nodes[cell].data[index])
                    + fraction * point(nodes[cell + 1].data[index]);
                let expected = exact.inf() / 2. + exact.sup() / 2.;
                if value != expected {
                    return Err(bad());
                }
                squared += square(exact - point(value));
            }
            if error != upper(squared.sqrt())? {
                return Err(bad());
            }
        }
        Ok(())
    })
}

pub(crate) fn check(
    request: &TraceRequest,
    configuration: &Configuration,
    report: &TraceReport,
    query_times: &[f64],
    feasibility: &Feasibility,
    workflow: &Workflow,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        cancel.poll()?;
        if workflow.scope != SCOPE {
            return Err(bad());
        }
        let expected = derive_lineage_range(report, 0, report.segments.len(), cancel)?;
        if workflow.lineage != expected {
            return Err(bad());
        }
        check_queries(request, report, query_times, &expected, workflow, cancel)?;
        check_witnesses(
            request,
            configuration,
            query_times,
            feasibility,
            workflow,
            cancel,
        )
    })
}
