//! Independent simultaneous-event comparisons and fixed outer-binomial decisions.
//!
//! These numerical and Monte Carlo decisions do not verify a sampling premise.
use eigentrail_core::{
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverageOutcome {
    Covered,
    Undercovered,
    Undecided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignDecision {
    NotRejected,
    Rejected,
    Incomplete,
}

fn invalid(reason: &str) -> Error {
    Error::Invalid(format!("campaign oracle: {reason}"))
}

/// Compare a checked error-norm enclosure with the exact reported binary64 radius.
/// The failure event is strict: equality alone never establishes undercoverage.
pub fn classify(error: &Range, radius: f64) -> Result<CoverageOutcome> {
    nonnegative(radius, "campaign radius")?;
    if !error.lower.is_finite()
        || !error.upper.is_finite()
        || !error.lower_closed
        || !error.upper_closed
        || error.lower < 0.
        || error.lower > error.upper
    {
        return Err(invalid(
            "finite nonnegative closed error enclosure required",
        ));
    }
    Ok(if error.upper <= radius {
        CoverageOutcome::Covered
    } else if error.lower > radius {
        CoverageOutcome::Undercovered
    } else {
        CoverageOutcome::Undecided
    })
}

/// Enclose the operator error against every intended matrix in the population
/// interval representation, without calling production calibration or eigensolvers.
pub fn error_bounds(
    sample: &Matrix,
    population: &[Interval],
    cancel: &Cancellation,
) -> Result<Range> {
    cancel.poll()?;
    numerics::validate_matrix(sample)?;
    if sample.n > 256 {
        return Err(Error::Resource("campaign dimension ceiling 256".into()));
    }
    if population.len() != sample.data.len()
        || population
            .iter()
            .any(|value| value.is_empty() || !value.inf().is_finite() || !value.sup().is_finite())
    {
        return Err(invalid(
            "complete finite population matrix enclosure required",
        ));
    }
    let population_bytes = population
        .len()
        .checked_mul(std::mem::size_of::<Interval>())
        .ok_or_else(|| Error::Resource("campaign input byte overflow".into()))?;
    let retained = crate::retained_bytes(&[sample.heap_bytes()?, population_bytes])?;
    let limit = memory::active_limit().unwrap_or(resources::DEFAULT_BYTES);
    memory::ensure_operation(limit, retained, || {
        let n = sample.n;
        let mut columns_squared = memory::capacity(n)?;
        columns_squared.resize(n, point(0.));
        let mut columns_absolute = memory::capacity(n)?;
        columns_absolute.resize(n, point(0.));
        let mut squared_frobenius = point(0.);
        let mut maximum_row_sum = 0f64;
        for row in 0..n {
            cancel.poll()?;
            let mut row_absolute = point(0.);
            for col in 0..n {
                let index = row + col * n;
                let difference = point(sample.data[index]) - population[index];
                let squared = square(difference);
                squared_frobenius += squared;
                columns_squared[col] += squared;
                let absolute = difference.abs();
                row_absolute += absolute;
                columns_absolute[col] += absolute;
            }
            maximum_row_sum = maximum_row_sum.max(upper(row_absolute)?);
        }
        let frobenius = squared_frobenius.sqrt();
        let mut lower = (frobenius / point(n as f64).sqrt()).inf().max(0.);
        let mut maximum_column_sum = 0f64;
        for col in 0..n {
            cancel.poll()?;
            lower = lower.max(columns_squared[col].sqrt().inf());
            maximum_column_sum = maximum_column_sum.max(upper(columns_absolute[col])?);
        }
        let induced_upper = upper((point(maximum_row_sum) * point(maximum_column_sum)).sqrt())?;
        let high = upper(frobenius)?.min(induced_upper);
        if !lower.is_finite() || lower > high {
            return Err(Error::Exhausted(
                "campaign error enclosure unresolved".into(),
            ));
        }
        Ok(Range::closed(lower, high))
    })
}

/// Apply the registered n=2000, reject-at-131 rule only to complete independent
/// outer trials. Incomplete records remain visible and cannot become successes.
pub fn summarize_counts(
    covered: usize,
    undercovered: usize,
    undecided: usize,
    unfinished: usize,
    expected: usize,
) -> Result<CampaignDecision> {
    if expected != 2000 {
        return Err(invalid(
            "the registered binomial decision requires exactly 2000 outer trials",
        ));
    }
    let total = [covered, undercovered, undecided, unfinished]
        .into_iter()
        .try_fold(0usize, |sum, value| {
            sum.checked_add(value)
                .ok_or_else(|| invalid("outer count overflow"))
        })?;
    if total > expected {
        return Err(invalid("outer counts exceed the registered trial universe"));
    }
    Ok(if total != expected || undecided != 0 || unfinished != 0 {
        CampaignDecision::Incomplete
    } else if undercovered >= 131 {
        CampaignDecision::Rejected
    } else {
        CampaignDecision::NotRejected
    })
}
