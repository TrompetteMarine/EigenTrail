//! Deterministic transfer from a declared window target to instantaneous covariance.
//!
//! The supplied radius concerns the explicitly described estimator target. This
//! module neither constructs its sampling event nor identifies that target with
//! a pooled or sample-centred covariance. Raw input weights denote exact real
//! numbers; normalization is enclosed with outward arithmetic.
use crate::{
    Error, Range, Result,
    resources::{Cancellation, memory},
    scalar::{nonnegative, point, upper},
};
use serde::{Deserialize, Serialize};

pub const MAX_POINTS: usize = 10_000;
pub const MAX_TEXT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRequest {
    pub sample_times: Vec<f64>,
    pub weights: Vec<f64>,
    pub target_time: f64,
    /// Supplied error bound around the declared estimator target.
    pub window_radius: f64,
    /// Operator-norm regularity in the fixed covariance and time coordinates.
    pub lipschitz: f64,
    /// Bound between the declared target and weighted within-time covariances.
    pub target_mismatch_bound: f64,
    pub target_description: String,
    pub assumptions: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowEnvelope {
    pub mean_absolute_offset: Range,
    pub drift_bound: f64,
    pub total_radius: f64,
}

/// Validate the numerical and descriptive contract, not its scientific premises.
///
/// Duplicate or unordered sample times are valid. Extrapolation is allowed only
/// under the user's declared regularity on every connecting time interval.
pub fn validate(request: &WindowRequest) -> Result<()> {
    if !(1..=MAX_POINTS).contains(&request.sample_times.len())
        || request.weights.len() != request.sample_times.len()
    {
        return Err(Error::Invalid(
            "equal window time/weight lengths in 1..=10000 required".into(),
        ));
    }
    if !request.target_time.is_finite() || request.sample_times.iter().any(|time| !time.is_finite())
    {
        return Err(Error::Invalid(
            "finite window and target times required".into(),
        ));
    }
    if request
        .weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight < 0.)
        || !request.weights.iter().any(|weight| *weight > 0.)
    {
        return Err(Error::Invalid(
            "finite nonnegative window weights with positive total required".into(),
        ));
    }
    nonnegative(request.window_radius, "window radius")?;
    nonnegative(request.lipschitz, "window covariance regularity")?;
    nonnegative(
        request.target_mismatch_bound,
        "window target mismatch bound",
    )?;
    for text in [&request.target_description, &request.assumptions] {
        if text.len() > MAX_TEXT_BYTES || text.trim().is_empty() {
            return Err(Error::Invalid(
                "window target description and assumptions must be nonempty and <=4096 UTF-8 bytes"
                    .into(),
            ));
        }
    }
    Ok(())
}

/// Enclose `r + beta + L * sum(a_i * |s_i-t|) / sum(a_i)`.
///
/// This implication holds on the supplied source event. A collection of outputs
/// is simultaneous only when the supplied source radii hold on a common event.
/// The target mismatch is charged once in `total_radius`; do not charge that
/// same effect again through a downstream TRACE correction.
pub fn analyze(request: &WindowRequest, cancel: &Cancellation) -> Result<WindowEnvelope> {
    memory::scope(|| {
        cancel.poll()?;
        validate(request)?;
        let maximum = request.weights.iter().copied().fold(0., f64::max);
        let mut total = point(0.);
        let mut numerator = point(0.);
        for (&time, &weight) in request.sample_times.iter().zip(&request.weights) {
            cancel.poll()?;
            if weight == 0. {
                continue;
            }
            // At least one scaled weight is exactly one. Scaling by the maximum
            // avoids raw-weight sum overflow and preserves subnormal enclosures.
            let scaled = point(weight) / point(maximum);
            let offset = (point(time) - point(request.target_time)).abs();
            upper(offset)?;
            numerator += scaled * offset;
            total += scaled;
            upper(total)?;
            upper(numerator)?;
        }
        let distance = numerator / total;
        let mean_absolute_offset = Range::closed(distance.inf().max(0.), upper(distance)?);
        let drift_bound = upper(point(request.lipschitz) * point(mean_absolute_offset.upper))?;
        let total_radius = upper(
            point(request.window_radius)
                + point(request.target_mismatch_bound)
                + point(drift_bound),
        )?;
        Ok(WindowEnvelope {
            mean_absolute_offset,
            drift_bound,
            total_radius,
        })
    })
}
