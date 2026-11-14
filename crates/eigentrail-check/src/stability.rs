//! Independently verify tolerance decisions against a checked source history.
use eigentrail_core::{
    Error, Quantity, Result, continuous_queries,
    families::{HistoryReport, HistoryRequest, PathClass, SCHEMA_V2},
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    serialization,
    stability::{self, Assessment, Decision, QueryState},
};
use serde::Serialize;

/// Schema of the additive envelope; the complete history retains its own schema.
pub const SCHEMA: &str = "eigentrail.stability.v1";
/// Distance between corresponding equal-rank projectors in fixed Euclidean coordinates.
pub const METRIC: &str = "projector_operator_distance";
/// Qualifications attached to every public stability output.
pub const SCOPE: &str = "conditional single-query projector-distance decisions in fixed Euclidean feature coordinates; source history retains intervals, identity qualifications, calibration and feasibility; no new statistical coverage or whole-path existence claim; stable projectors do not imply equal covariance matrices or temporal dynamics; self-comparisons and whole-space projector stability are trivial";

fn invalid(message: &'static str) -> Error {
    Error::Invalid(format!("projector stability checker: {message}"))
}

// This deliberately does not call the producer's classifier or assessment.
fn expected_decision(quantity: &Quantity, tolerance: f64) -> Result<Decision> {
    match quantity {
        Quantity::Undefined(_) => Ok(Decision::Unresolved),
        Quantity::Interval(range) => {
            if !range.lower.is_finite()
                || !range.upper.is_finite()
                || !range.lower_closed
                || !range.upper_closed
                || range.lower < 0.
                || range.upper > 1.
                || range.lower > range.upper
            {
                return Err(invalid(
                    "finite closed distance enclosure in [0,1] required",
                ));
            }
            if range.upper <= tolerance {
                Ok(Decision::StableWithinTolerance)
            } else if tolerance < range.lower {
                Ok(Decision::ChangedBeyondTolerance)
            } else {
                Ok(Decision::Unresolved)
            }
        }
    }
}

/// Check source evidence first, then independently reconstruct every decision.
///
/// Success establishes consistency with the source history's conditional
/// enclosures and identity guards. It does not prove their sampling assumptions,
/// the existence of a complete admissible path, or a new coverage theorem.
pub fn check(
    request: &HistoryRequest,
    report: &HistoryReport,
    assessment: &Assessment,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        report.heap_bytes()?,
        assessment.heap_bytes()?,
    ])?;
    memory::ensure_operation(request.observations.memory_bytes, retained, || {
        cancel.poll()?;
        crate::families::check(request, report, cancel)?;
        if report.schema != SCHEMA_V2
            || report
                .configurations
                .iter()
                .any(|configuration| !matches!(configuration.class, PathClass::Continuous))
        {
            return Err(invalid(
                "only histories.v2 continuous configurations are supported",
            ));
        }
        let tolerance = assessment.tolerance;
        if !tolerance.is_finite() || !(0. ..=1.).contains(&tolerance) {
            return Err(invalid("finite tolerance in [0,1] required"));
        }
        if assessment.configurations.len() != report.configurations.len() {
            return Err(invalid(
                "configuration universe differs from source history",
            ));
        }
        let dimension = request.observations.features.len();
        for (actual, source) in assessment.configurations.iter().zip(&report.configurations) {
            cancel.poll()?;
            if actual.id != source.id || actual.workflows.len() != source.continuous_workflow.len()
            {
                return Err(invalid(
                    "configuration identity or workflow universe differs",
                ));
            }
            for (workflow, source_workflow) in
                actual.workflows.iter().zip(&source.continuous_workflow)
            {
                cancel.poll()?;
                if workflow.queries.len() != source_workflow.queries.len() {
                    return Err(invalid("query universe differs from source history"));
                }
                for (query, source_query) in workflow.queries.iter().zip(&source_workflow.queries) {
                    cancel.poll()?;
                    if query.time.to_bits() != source_query.time.to_bits() {
                        return Err(invalid("query time differs from source history"));
                    }
                    match (&query.state, &source_query.state) {
                        (
                            QueryState::InconsistentOuterBounds,
                            continuous_queries::QueryState::InconsistentOuterBounds,
                        ) => {}
                        (
                            QueryState::Bounds { blocks },
                            continuous_queries::QueryState::Bounds {
                                blocks: source_blocks,
                                ..
                            },
                        ) => {
                            if blocks.len() != source_blocks.len() {
                                return Err(invalid("block universe differs from source history"));
                            }
                            for (block, source_block) in blocks.iter().zip(source_blocks) {
                                cancel.poll()?;
                                if block.start != source_block.start
                                    || block.end != source_block.end
                                    || block.full_space
                                        != (source_block.start == 0
                                            && source_block.end == dimension)
                                    || block.local
                                        != expected_decision(
                                            &source_block.local_displacement,
                                            tolerance,
                                        )?
                                    || block.reference
                                        != expected_decision(
                                            &source_block.reference_displacement,
                                            tolerance,
                                        )?
                                {
                                    return Err(invalid(
                                        "block identity, scope or decision differs",
                                    ));
                                }
                            }
                        }
                        _ => return Err(invalid("source inconsistency state was changed")),
                    }
                }
            }
        }
        Ok(())
    })
}

#[derive(Serialize)]
struct Envelope<'a> {
    schema: &'static str,
    metric: &'static str,
    scope: &'static str,
    history: &'a HistoryReport,
    assessment: &'a Assessment,
}

/// Produce checked canonical JSON with the complete source history attached.
///
/// No source enclosure or qualifier is removed, reinterpreted, or charged twice.
/// The additive envelope does not modify either existing history schema.
pub fn encode_checked(
    request: &HistoryRequest,
    report: &HistoryReport,
    tolerance: f64,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    let retained = crate::retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.observations.memory_bytes, retained, || {
        memory::scope(|| {
            cancel.poll()?;
            let assessment = stability::assess(report, tolerance, cancel)?;
            check(request, report, &assessment, cancel)?;
            serialization::encode_with_cancel(
                &Envelope {
                    schema: SCHEMA,
                    metric: METRIC,
                    scope: SCOPE,
                    history: report,
                    assessment: &assessment,
                },
                cancel,
            )
        })
    })
}
