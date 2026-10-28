//! Reconstruct the small readout only after independently checking its full history.
use eigentrail_core::{
    Error, Quantity, Range, Result,
    common_frame::CommonFrameAssessment,
    common_frame_queries, continuous_queries,
    families::{self, PathClass},
    lineage_summary::{
        InfoLevel, Motion, RESOURCE_SCOPE, Report, Request, Row, RowState, SCHEMA, Target,
    },
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    serialization,
};

fn bad(reason: &str) -> Error {
    match memory::text(reason) {
        Ok(message) => Error::Invalid(message),
        Err(error) => error,
    }
}

fn overflow() -> Error {
    Error::Resource(String::new())
}

fn reservation(request: &Request) -> Result<usize> {
    if request.schema != SCHEMA
        || request.history.schema != families::SCHEMA_V2
        || !(1..=16).contains(&request.history.configurations.len())
        || !(1..=10000).contains(&request.history.query_times.len())
        || !request.delta.is_finite()
        || request.delta <= 0.
        || request.history.configurations.iter().any(|configuration| {
            !matches!(
                configuration.class,
                PathClass::Continuous | PathClass::CommonFrame { .. }
            )
        })
    {
        return Err(bad(
            "readout checker: schema, queries, delta or model scope",
        ));
    }
    let base = families::validate_request(&request.history)?;
    let p = request.history.observations.features.len();
    let rows = request
        .history
        .configurations
        .len()
        .checked_mul(request.history.query_times.len())
        .and_then(|n| n.checked_mul(p))
        .ok_or_else(overflow)?;
    let bytes = p
        .checked_mul(16)
        .and_then(|n| n.checked_add(8192))
        .ok_or_else(overflow)?;
    let total = rows
        .checked_mul(bytes)
        .and_then(|n| n.checked_add(4096))
        .and_then(|n| n.checked_add(base))
        .ok_or_else(overflow)?;
    if total > request.history.observations.memory_bytes {
        return Err(overflow());
    }
    Ok(total)
}

enum SourceTarget<'a> {
    OrderedBlock {
        start: usize,
        end: usize,
        origins: &'a [usize],
        persistent: bool,
    },
    AnchorAxis(usize),
}

struct Expected<'a> {
    target: SourceTarget<'a>,
    dimension: usize,
    strength: &'a Range,
    share: &'a Quantity,
    rank: [usize; 2],
    projector_error: f64,
    reference: &'a Quantity,
}

fn motion(reference: &Quantity, q: usize, p: usize, delta: f64) -> Result<Motion> {
    // This deliberately does not call the producer's classification function.
    if q == p {
        return Ok(Motion::Unresolved);
    }
    match reference {
        Quantity::Undefined(_) => Ok(Motion::Unresolved),
        Quantity::Interval(range) => {
            if !range.lower.is_finite() || !range.upper.is_finite() || range.lower > range.upper {
                return Err(bad("readout checker: invalid checked displacement"));
            }
            Ok(if range.upper < delta {
                Motion::WithinTolerance
            } else if range.lower > delta {
                Motion::BeyondTolerance
            } else {
                Motion::Unresolved
            })
        }
    }
}

fn bounds(row: &Row, expected: Expected<'_>, request: &Request) -> Result<()> {
    let RowState::Bounds {
        target,
        information_level,
        block_dimension,
        ambient_dimension,
        strength,
        share,
        rank,
        projector_error,
        reference_displacement,
        motion: claimed_motion,
        accuracy_target_attained,
    } = &row.state
    else {
        return Err(bad("readout checker: missing bounds row"));
    };
    let p = request.history.observations.features.len();
    let q = expected.dimension;
    if q == 0 || q > p {
        return Err(bad("readout checker: invalid source block dimension"));
    }
    let identity_matches = match (target, expected.target) {
        (
            Target::OrderedBlock {
                start,
                end,
                origins,
                persistent,
            },
            SourceTarget::OrderedBlock {
                start: a,
                end: b,
                origins: original,
                persistent: kept,
            },
        ) => *start == a && *end == b && origins.as_slice() == original && *persistent == kept,
        (Target::AnchorAxis { axis }, SourceTarget::AnchorAxis(original)) => *axis == original,
        _ => false,
    };
    let level = if q == p {
        InfoLevel::WholeSpace
    } else if q == 1 {
        InfoLevel::Line
    } else {
        InfoLevel::ProperSubspace
    };
    let attained = q < p && expected.projector_error < request.history.observations.target;
    if !identity_matches
        || *information_level != level
        || *block_dimension != q
        || *ambient_dimension != p
        || strength != expected.strength
        || share != expected.share
        || *rank != expected.rank
        || projector_error.to_bits() != expected.projector_error.to_bits()
        || reference_displacement != expected.reference
        || *claimed_motion != motion(expected.reference, q, p, request.delta)?
        || *accuracy_target_attained != attained
    {
        return Err(bad(
            "readout checker: identity, bounds, precision or motion differs",
        ));
    }
    Ok(())
}

fn next_row<'a>(
    report: &'a Report,
    cursor: &mut usize,
    configuration: usize,
    query: usize,
    time: f64,
    cancel: &Cancellation,
) -> Result<&'a Row> {
    cancel.poll()?;
    let row = report
        .rows
        .get(*cursor)
        .ok_or_else(|| bad("readout checker: omitted row"))?;
    *cursor = cursor.checked_add(1).ok_or_else(overflow)?;
    if row.configuration != configuration
        || row.query != query
        || row.time.to_bits() != time.to_bits()
    {
        return Err(bad("readout checker: row order or query binding differs"));
    }
    Ok(row)
}

pub fn check(request: &Request, report: &Report, cancel: &Cancellation) -> Result<()> {
    cancel.poll()?;
    let reserved = reservation(request)?;
    let maximum_rows = request
        .history
        .configurations
        .len()
        .checked_mul(request.history.query_times.len())
        .and_then(|n| n.checked_mul(request.history.observations.features.len()))
        .ok_or_else(overflow)?;
    if report.rows.len() > maximum_rows {
        return Err(bad(
            "readout checker: row count exceeds the admitted universe",
        ));
    }
    let retained = crate::retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.history.observations.memory_bytes, retained, || {
        cancel.poll()?;
        if report.schema != SCHEMA
            || !report.completed
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.managed_reservation != reserved
            || report.resource_qualification != RESOURCE_SCOPE
        {
            return Err(bad(
                "readout checker: schema, input, completeness or resources",
            ));
        }
        // Numerical bounds, every competitor and feasibility are checked from the
        // intended request before any row is allowed to refer to this history.
        crate::families::check(&request.history, &report.history, cancel)?;
        let mut cursor = 0usize;
        for (configuration, (input, source)) in request
            .history
            .configurations
            .iter()
            .zip(&report.history.configurations)
            .enumerate()
        {
            cancel.poll()?;
            match &input.class {
                PathClass::Continuous => {
                    if source.continuous_workflow.len() != 1 {
                        return Err(bad("readout checker: continuous workflow universe"));
                    }
                    let workflow = &source.continuous_workflow[0];
                    if workflow.queries.len() != request.history.query_times.len() {
                        return Err(bad("readout checker: continuous query universe"));
                    }
                    for (query, sample) in workflow.queries.iter().enumerate() {
                        let time = request.history.query_times[query];
                        match &sample.state {
                            continuous_queries::QueryState::InconsistentOuterBounds => {
                                let row = next_row(
                                    report,
                                    &mut cursor,
                                    configuration,
                                    query,
                                    time,
                                    cancel,
                                )?;
                                if !matches!(row.state, RowState::InconsistentOuterBounds) {
                                    return Err(bad(
                                        "readout checker: inconsistent bounds were hidden",
                                    ));
                                }
                            }
                            continuous_queries::QueryState::Bounds { blocks, .. } => {
                                for block in blocks {
                                    let row = next_row(
                                        report,
                                        &mut cursor,
                                        configuration,
                                        query,
                                        time,
                                        cancel,
                                    )?;
                                    let q =
                                        block.end.checked_sub(block.start).ok_or_else(|| {
                                            bad("readout checker: source block range")
                                        })?;
                                    bounds(
                                        row,
                                        Expected {
                                            target: SourceTarget::OrderedBlock {
                                                start: block.start,
                                                end: block.end,
                                                origins: &block.origins,
                                                persistent: block.persistent,
                                            },
                                            dimension: q,
                                            strength: &block.strength,
                                            share: &block.share,
                                            rank: block.rank,
                                            projector_error: block.projector_error,
                                            reference: &block.reference_displacement,
                                        },
                                        request,
                                    )?;
                                }
                            }
                        }
                    }
                }
                PathClass::CommonFrame { .. } => {
                    if source.common_frame_queries.len() != 1 || source.common_frame.len() != 1 {
                        return Err(bad("readout checker: common-frame workflow universe"));
                    }
                    let workflow = &source.common_frame_queries[0];
                    if workflow.queries.len() != request.history.query_times.len() {
                        return Err(bad("readout checker: common-frame query universe"));
                    }
                    for (query, sample) in workflow.queries.iter().enumerate() {
                        let time = request.history.query_times[query];
                        match &sample.state {
                            common_frame_queries::QueryState::UnresolvedAnchor => {
                                let row = next_row(
                                    report,
                                    &mut cursor,
                                    configuration,
                                    query,
                                    time,
                                    cancel,
                                )?;
                                if !matches!(row.state, RowState::UnresolvedAnchor) {
                                    return Err(bad(
                                        "readout checker: unresolved anchor was hidden",
                                    ));
                                }
                            }
                            common_frame_queries::QueryState::InconsistentOuterBounds => {
                                let row = next_row(
                                    report,
                                    &mut cursor,
                                    configuration,
                                    query,
                                    time,
                                    cancel,
                                )?;
                                if !matches!(row.state, RowState::InconsistentOuterBounds) {
                                    return Err(bad(
                                        "readout checker: inconsistent common-frame bounds",
                                    ));
                                }
                            }
                            common_frame_queries::QueryState::Bounds { summaries, .. } => {
                                let CommonFrameAssessment::Resolved { report: frame } =
                                    &source.common_frame[0]
                                else {
                                    return Err(bad(
                                        "readout checker: axis summaries need a resolved anchor",
                                    ));
                                };
                                // The exact true axis is fixed by the declared model;
                                // this says nothing about accuracy of its estimate.
                                let fixed = Quantity::Interval(Range::closed(0., 0.));
                                for (axis, summary) in summaries.iter().enumerate() {
                                    let row = next_row(
                                        report,
                                        &mut cursor,
                                        configuration,
                                        query,
                                        time,
                                        cancel,
                                    )?;
                                    bounds(
                                        row,
                                        Expected {
                                            target: SourceTarget::AnchorAxis(axis),
                                            dimension: 1,
                                            strength: &summary.strength,
                                            share: &summary.share,
                                            rank: summary.rank,
                                            projector_error: frame.frame.axis_error,
                                            reference: &fixed,
                                        },
                                        request,
                                    )?;
                                }
                            }
                        }
                    }
                }
                _ => return Err(bad("readout checker: unsupported model")),
            }
        }
        if cursor != report.rows.len() {
            return Err(bad("readout checker: extra rows"));
        }
        Ok(())
    })
}
