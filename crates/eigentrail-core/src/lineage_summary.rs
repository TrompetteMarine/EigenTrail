//! Read checked history information without promoting outer labels or witnesses.
use crate::{
    Error, Quantity, Range, Result,
    common_frame::CommonFrameAssessment,
    common_frame_queries, continuous_queries, families,
    resources::{Cancellation, memory, memory::HeapBytes},
    serialization,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "eigentrail.lineage-summary.v1";
pub const RESOURCE_SCOPE: &str = families::RESOURCE_SCOPE;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub history: families::HistoryRequest,
    pub delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub request_hash: String,
    pub history: families::HistoryReport,
    pub rows: Vec<Row>,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InfoLevel {
    Line,
    ProperSubspace,
    WholeSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Motion {
    WithinTolerance,
    BeyondTolerance,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Target {
    OrderedBlock {
        start: usize,
        end: usize,
        origins: Vec<usize>,
        persistent: bool,
    },
    AnchorAxis {
        axis: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // Inline rows avoid another allocation for every query target.
pub enum RowState {
    Bounds {
        target: Target,
        information_level: InfoLevel,
        block_dimension: usize,
        ambient_dimension: usize,
        strength: Range,
        share: Quantity,
        rank: [usize; 2],
        projector_error: f64,
        reference_displacement: Quantity,
        motion: Motion,
        accuracy_target_attained: bool,
    },
    UnresolvedAnchor,
    InconsistentOuterBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub configuration: usize,
    pub query: usize,
    pub time: f64,
    pub state: RowState,
}

fn invalid<T>(message: &str) -> Result<T> {
    Err(Error::Invalid(memory::text(message)?))
}

fn positive_delta(delta: f64) -> Result<()> {
    if !delta.is_finite() || delta <= 0. {
        return invalid("finite positive lineage-summary delta required");
    }
    Ok(())
}

/// Exact reservation policy frozen before implementation; not a process-RSS claim.
pub fn validate_request(request: &Request) -> Result<usize> {
    positive_delta(request.delta)?;
    if request.schema != SCHEMA || request.history.schema != families::SCHEMA_V2 {
        return invalid("lineage-summary v1 requires history v2");
    }
    if !(1..=16).contains(&request.history.configurations.len())
        || !(1..=10000).contains(&request.history.query_times.len())
    {
        return invalid("lineage-summary requires 1..16 configurations and 1..10000 queries");
    }
    for configuration in &request.history.configurations {
        if !matches!(
            configuration.class,
            families::PathClass::Continuous | families::PathClass::CommonFrame { .. }
        ) {
            return invalid("lineage-summary v1 supports only Continuous and CommonFrame");
        }
    }
    let history = families::validate_request(&request.history)?;
    let p = request.history.observations.features.len();
    let rows = request
        .history
        .configurations
        .len()
        .checked_mul(request.history.query_times.len())
        .and_then(|n| n.checked_mul(p))
        .ok_or_else(|| Error::Resource(String::new()))?;
    let row_bytes = p
        .checked_mul(16)
        .and_then(|n| n.checked_add(8192))
        .ok_or_else(|| Error::Resource(String::new()))?;
    let reservation = rows
        .checked_mul(row_bytes)
        .and_then(|n| n.checked_add(4096))
        .and_then(|n| n.checked_add(history))
        .ok_or_else(|| Error::Resource(String::new()))?;
    if reservation > request.history.observations.memory_bytes {
        return Err(Error::Resource(memory::text(
            "lineage-summary reservation exceeds managed budget",
        )?));
    }
    Ok(reservation)
}

/// Strict margins classify reference displacement, never hidden ancestry or path length.
pub fn classify_motion(
    displacement: &Quantity,
    block_dimension: usize,
    ambient_dimension: usize,
    delta: f64,
) -> Result<Motion> {
    positive_delta(delta)?;
    if block_dimension == 0 || ambient_dimension == 0 || block_dimension > ambient_dimension {
        return invalid("valid lineage-summary block/ambient dimensions required");
    }
    let Quantity::Interval(range) = displacement else {
        return Ok(Motion::Unresolved);
    };
    if !range.lower.is_finite()
        || !range.upper.is_finite()
        || range.lower > range.upper
        || (range.lower == range.upper && (!range.lower_closed || !range.upper_closed))
    {
        return invalid("finite ordered lineage-summary displacement interval required");
    }
    Ok(if block_dimension == ambient_dimension {
        Motion::Unresolved
    } else if range.upper < delta {
        Motion::WithinTolerance
    } else if range.lower > delta {
        Motion::BeyondTolerance
    } else {
        Motion::Unresolved
    })
}

fn quantity(value: &Quantity) -> Result<Quantity> {
    Ok(match value {
        Quantity::Interval(range) => Quantity::Interval(range.clone()),
        Quantity::Undefined(reason) => Quantity::Undefined(memory::text(reason)?),
    })
}

fn bounds(
    target: Target,
    q: usize,
    p: usize,
    summary: crate::Summary,
    projector_error: f64,
    reference: &Quantity,
    request: &Request,
) -> Result<RowState> {
    let motion = classify_motion(reference, q, p, request.delta)?;
    Ok(RowState::Bounds {
        target,
        information_level: if q == p {
            InfoLevel::WholeSpace
        } else if q == 1 {
            InfoLevel::Line
        } else {
            InfoLevel::ProperSubspace
        },
        block_dimension: q,
        ambient_dimension: p,
        strength: summary.strength,
        share: summary.share,
        rank: summary.rank,
        projector_error,
        reference_displacement: quantity(reference)?,
        motion,
        accuracy_target_attained: q < p && projector_error < request.history.observations.target,
    })
}

pub fn analyze(request: &Request, cancel: &Cancellation) -> Result<Report> {
    cancel.poll()?;
    let reservation = validate_request(request)?;
    memory::ensure_operation(
        request.history.observations.memory_bytes,
        request.heap_bytes()?,
        || {
            cancel.poll()?;
            let history = families::analyze(&request.history, cancel)?;
            let p = request.history.observations.features.len();
            let maximum = request
                .history
                .configurations
                .len()
                .checked_mul(request.history.query_times.len())
                .and_then(|n| n.checked_mul(p))
                .ok_or_else(|| Error::Resource(String::new()))?;
            let mut rows = memory::capacity(maximum)?;
            for (configuration, member) in history.configurations.iter().enumerate() {
                cancel.poll()?;
                match &member.class {
                    families::PathClass::Continuous => {
                        let Some(workflow) = member.continuous_workflow.first() else {
                            return invalid("continuous lineage-summary workflow absent");
                        };
                        for (query, item) in workflow.queries.iter().enumerate() {
                            cancel.poll()?;
                            match &item.state {
                                continuous_queries::QueryState::Bounds { blocks, .. } => {
                                    for block in blocks {
                                        cancel.poll()?;
                                        let target = Target::OrderedBlock {
                                            start: block.start,
                                            end: block.end,
                                            origins: memory::copy(&block.origins)?,
                                            persistent: block.persistent,
                                        };
                                        let summary = crate::Summary {
                                            strength: block.strength.clone(),
                                            share: quantity(&block.share)?,
                                            rank: block.rank,
                                        };
                                        let state = bounds(
                                            target,
                                            block.end - block.start,
                                            p,
                                            summary,
                                            block.projector_error,
                                            &block.reference_displacement,
                                            request,
                                        )?;
                                        rows.push(Row {
                                            configuration,
                                            query,
                                            time: item.time,
                                            state,
                                        });
                                    }
                                }
                                continuous_queries::QueryState::InconsistentOuterBounds => rows
                                    .push(Row {
                                        configuration,
                                        query,
                                        time: item.time,
                                        state: RowState::InconsistentOuterBounds,
                                    }),
                            }
                        }
                    }
                    families::PathClass::CommonFrame { .. } => {
                        let Some(workflow) = member.common_frame_queries.first() else {
                            return invalid("common-frame lineage-summary workflow absent");
                        };
                        for (query, item) in workflow.queries.iter().enumerate() {
                            cancel.poll()?;
                            let state = match &item.state {
                                common_frame_queries::QueryState::Bounds { summaries, .. } => {
                                    let Some(CommonFrameAssessment::Resolved { report }) =
                                        member.common_frame.first()
                                    else {
                                        return invalid(
                                            "common-frame axis summary without resolved anchor",
                                        );
                                    };
                                    let reference = Quantity::Interval(Range::closed(0., 0.));
                                    for (axis, summary) in summaries.iter().enumerate() {
                                        cancel.poll()?;
                                        let owned = crate::Summary {
                                            strength: summary.strength.clone(),
                                            share: quantity(&summary.share)?,
                                            rank: summary.rank,
                                        };
                                        let state = bounds(
                                            Target::AnchorAxis { axis },
                                            1,
                                            p,
                                            owned,
                                            report.frame.axis_error,
                                            &reference,
                                            request,
                                        )?;
                                        rows.push(Row {
                                            configuration,
                                            query,
                                            time: item.time,
                                            state,
                                        });
                                    }
                                    continue;
                                }
                                common_frame_queries::QueryState::UnresolvedAnchor => {
                                    RowState::UnresolvedAnchor
                                }
                                common_frame_queries::QueryState::InconsistentOuterBounds => {
                                    RowState::InconsistentOuterBounds
                                }
                            };
                            rows.push(Row {
                                configuration,
                                query,
                                time: item.time,
                                state,
                            });
                        }
                    }
                    _ => return invalid("unsupported lineage-summary configuration"),
                }
            }
            cancel.poll()?;
            Ok(Report {
                schema: memory::text(SCHEMA)?,
                request_hash: serialization::digest_with_cancel(request, cancel)?,
                history,
                rows,
                completed: true,
                managed_reservation: reservation,
                resource_qualification: memory::text(RESOURCE_SCOPE)?,
            })
        },
    )
}
