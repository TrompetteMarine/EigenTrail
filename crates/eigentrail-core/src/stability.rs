//! Tolerance decisions conditional on existing checked projector-displacement enclosures.
//!
//! This module adds no calibration, association, rounding allowance or feasibility
//! claim. Use `eigentrail_check::stability` at an analysis boundary to check the
//! source history and the derived assessment together.
use crate::{
    Error, Quantity, Result, continuous_queries,
    families::{self, HistoryReport, PathClass},
    resources::{Cancellation, memory},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    StableWithinTolerance,
    ChangedBeyondTolerance,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub tolerance: f64,
    pub configurations: Vec<ConfigurationAssessment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationAssessment {
    pub id: String,
    pub workflows: Vec<WorkflowAssessment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowAssessment {
    pub queries: Vec<QueryAssessment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryAssessment {
    pub time: f64,
    pub state: QueryState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum QueryState {
    Bounds { blocks: Vec<BlockAssessment> },
    InconsistentOuterBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockAssessment {
    pub start: usize,
    pub end: usize,
    /// The ambient-space projector is always the identity: its stability is trivial.
    pub full_space: bool,
    pub local: Decision,
    pub reference: Decision,
}

/// Validate a tolerance in operator-projector distance units, not angle units.
pub fn validate_tolerance(tolerance: f64) -> Result<()> {
    if !tolerance.is_finite() || !(0. ..=1.).contains(&tolerance) {
        return Err(Error::Invalid(
            "finite projector stability tolerance in [0,1] required".into(),
        ));
    }
    Ok(())
}

/// Classify a supplied enclosure without establishing its scientific validity.
///
/// Equality at the upper endpoint establishes stability within tolerance;
/// change requires a strictly larger lower endpoint. Undefined identity stays
/// unresolved even at tolerance one. Existing displacement bounds already
/// contain their numerical and covariance uncertainty: no term is added here.
pub fn classify_projector_displacement(
    displacement: &Quantity,
    tolerance: f64,
) -> Result<Decision> {
    validate_tolerance(tolerance)?;
    let Quantity::Interval(range) = displacement else {
        return Ok(Decision::Unresolved);
    };
    if !range.lower.is_finite()
        || !range.upper.is_finite()
        || range.lower < 0.
        || range.upper > 1.
        || range.lower > range.upper
        || !range.lower_closed
        || !range.upper_closed
    {
        return Err(Error::Invalid(
            "closed finite projector displacement interval inside [0,1] required".into(),
        ));
    }
    Ok(if range.upper <= tolerance {
        Decision::StableWithinTolerance
    } else if range.lower > tolerance {
        Decision::ChangedBeyondTolerance
    } else {
        Decision::Unresolved
    })
}

/// Derive decisions from a history already checked against its request.
///
/// Only history-v2 continuous configurations are supported. Positions in the
/// returned arrays align with the source report. Keep that report alongside the
/// assessment: it contains the enclosures, undefined reasons, lineage,
/// calibration scope and distinct original/widened/unresolved feasibility.
/// A stable projector does not imply a constant covariance or temporal law.
pub fn assess(
    history: &HistoryReport,
    tolerance: f64,
    cancel: &Cancellation,
) -> Result<Assessment> {
    memory::scope(|| {
        cancel.poll()?;
        validate_tolerance(tolerance)?;
        if history.schema != families::SCHEMA_V2
            || !history.completed
            || history.configurations.is_empty()
            || history.configurations.iter().any(|configuration| {
                configuration.class != PathClass::Continuous
                    || configuration.continuous_workflow.len() != 1
            })
        {
            return Err(Error::Invalid(
                "stability assessment requires completed history-v2 continuous configurations"
                    .into(),
            ));
        }
        let mut configurations = memory::capacity(history.configurations.len())?;
        for configuration in &history.configurations {
            cancel.poll()?;
            let p = configuration
                .continuous_conditional
                .eigensystems
                .first()
                .map(|system| system.values.len())
                .filter(|dimension| *dimension > 0)
                .ok_or_else(|| Error::Invalid("missing source eigensystem dimension".into()))?;
            let mut workflows = memory::capacity(configuration.continuous_workflow.len())?;
            for source in &configuration.continuous_workflow {
                let mut queries = memory::capacity(source.queries.len())?;
                for query in &source.queries {
                    cancel.poll()?;
                    let state = match &query.state {
                        continuous_queries::QueryState::InconsistentOuterBounds => {
                            QueryState::InconsistentOuterBounds
                        }
                        continuous_queries::QueryState::Bounds { blocks, .. } => {
                            let mut decisions = memory::capacity(blocks.len())?;
                            for block in blocks {
                                cancel.poll()?;
                                if block.start >= block.end || block.end > p {
                                    return Err(Error::Invalid(
                                        "invalid source projector block range".into(),
                                    ));
                                }
                                decisions.push(BlockAssessment {
                                    start: block.start,
                                    end: block.end,
                                    full_space: block.start == 0 && block.end == p,
                                    local: classify_projector_displacement(
                                        &block.local_displacement,
                                        tolerance,
                                    )?,
                                    reference: classify_projector_displacement(
                                        &block.reference_displacement,
                                        tolerance,
                                    )?,
                                });
                            }
                            QueryState::Bounds { blocks: decisions }
                        }
                    };
                    queries.push(QueryAssessment {
                        time: query.time,
                        state,
                    });
                }
                workflows.push(WorkflowAssessment { queries });
            }
            configurations.push(ConfigurationAssessment {
                id: memory::text(&configuration.id)?,
                workflows,
            });
        }
        Ok(Assessment {
            tolerance,
            configurations,
        })
    })
}
