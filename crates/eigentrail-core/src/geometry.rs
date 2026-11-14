//! Conditional intrinsic geometry of an operator-norm covariance tube.
//!
//! The centre's positive spectral floor is supplied, not computed here. These
//! scalar implications preserve an existing joint event; they create neither
//! a statistical event nor a verified continuous path.
use crate::{
    Error, Result,
    resources::{Cancellation, memory},
    scalar::{ln, nonnegative, point, upper},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpdTubeRequest {
    pub dimension: u32,
    /// Supplied positive lower bound for the centre's smallest eigenvalue.
    pub centre_floor: f64,
    /// Supplied operator-norm error bound about that centre.
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpdTubeEnvelope {
    pub resolved: bool,
    /// Affine-invariant distance, using tr(A^-1 U A^-1 V) as the metric.
    pub affine_invariant_radius: Option<f64>,
    pub precision_radius: Option<f64>,
}

/// Validate scalar inputs without verifying the unprovided covariance matrix.
pub fn validate_spd(request: &SpdTubeRequest) -> Result<()> {
    if request.dimension == 0 || !request.centre_floor.is_finite() || request.centre_floor <= 0. {
        return Err(Error::Invalid(
            "positive dimension and finite positive centre spectral floor required".into(),
        ));
    }
    nonnegative(request.radius, "covariance tube radius")?;
    Ok(())
}

/// Convert an operator tube to affine-invariant and precision error bounds.
///
/// If the radius reaches the supplied spectral floor, no positive target floor
/// or finite universal intrinsic bound follows. This unresolved result does not
/// assert singularity of any actual target or invalidate PSD spectral analysis.
pub fn analyze_spd(request: &SpdTubeRequest, cancel: &Cancellation) -> Result<SpdTubeEnvelope> {
    memory::scope(|| {
        cancel.poll()?;
        validate_spd(request)?;
        if request.radius >= request.centre_floor {
            return Ok(SpdTubeEnvelope {
                resolved: false,
                affine_invariant_radius: None,
                precision_radius: None,
            });
        }
        if request.radius == 0. {
            return Ok(SpdTubeEnvelope {
                resolved: true,
                affine_invariant_radius: Some(0.),
                precision_radius: Some(0.),
            });
        }
        cancel.poll()?;
        let gap = point(request.centre_floor) - point(request.radius);
        if gap.inf() <= 0. {
            return Err(Error::Exhausted(
                "positive covariance tube floor unresolved numerically".into(),
            ));
        }
        // Subtract logs instead of forming a potentially overflowing ratio.
        let logratio = ln(point(request.centre_floor))? - ln(gap)?;
        let affine_invariant_radius = upper(point(f64::from(request.dimension)).sqrt() * logratio)?;
        // Sequential division avoids under/overflow of a*(a-r).
        let precision_radius = upper((point(request.radius) / point(request.centre_floor)) / gap)?;
        cancel.poll()?;
        Ok(SpdTubeEnvelope {
            resolved: true,
            affine_invariant_radius: Some(affine_invariant_radius),
            precision_radius: Some(precision_radius),
        })
    })
}
