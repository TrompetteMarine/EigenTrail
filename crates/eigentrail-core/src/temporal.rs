//! Finite-sample source bounds under a declared temporal Gaussian model.
//!
//! Observations are jointly Gaussian within each prespecified window after
//! subtracting their known true means. The block covariance envelope is an
//! external premise, not a fitted quantity validated by this scalar routine.
use crate::{
    Error, Result,
    resources::{Cancellation, memory},
    scalar::{ln, nonnegative, point, upper},
};
use serde::{Deserialize, Serialize};

pub const MAX_TEXT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalRequest {
    pub dimension: u32,
    pub observations: u32,
    pub windows: u32,
    /// External upper bound on the lag-zero covariance operator norm.
    pub covariance_bound: f64,
    /// External geometric block-covariance envelope in observation-index lag.
    pub correlation_bound: f64,
    /// Failure budget for the complete prespecified collection of windows.
    pub alpha: f64,
    pub assumptions: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalEnvelope {
    pub log_tail: f64,
    pub frobenius_bound: f64,
    pub operator_bound: f64,
    pub radius: f64,
}

/// Check the encoded contract, not the observation law or external envelope.
pub fn validate(request: &TemporalRequest) -> Result<()> {
    if request.dimension == 0 || request.observations == 0 || request.windows == 0 {
        return Err(Error::Invalid(
            "positive dimension, observation and window counts required".into(),
        ));
    }
    nonnegative(request.covariance_bound, "temporal covariance bound")?;
    if !request.correlation_bound.is_finite() || !(0. ..=1.).contains(&request.correlation_bound) {
        return Err(Error::Invalid(
            "finite temporal correlation envelope in [0,1] required".into(),
        ));
    }
    if !request.alpha.is_finite() || request.alpha <= 0. || request.alpha >= 1. {
        return Err(Error::Invalid(
            "finite temporal failure budget strictly between zero and one required".into(),
        ));
    }
    if request.assumptions.trim().is_empty() || request.assumptions.len() > MAX_TEXT_BYTES {
        return Err(Error::Invalid(
            "nonempty temporal assumptions of at most 4096 UTF-8 bytes required".into(),
        ));
    }
    Ok(())
}

/// Enclose a joint radius for uniform, divisor-n, known-mean second moments.
///
/// Each invocation must use the same total window count and failure budget for
/// a simultaneous collection. Window overlap is allowed; estimated means,
/// adaptively selected windows and data-fitted envelope constants need their
/// own validity argument and are outside this calculation.
pub fn analyze(request: &TemporalRequest, cancel: &Cancellation) -> Result<TemporalEnvelope> {
    memory::scope(|| {
        cancel.poll()?;
        validate(request)?;
        cancel.poll()?;
        // Add logarithms so even the smallest positive alpha remains usable.
        let log_tail = upper(
            (point(f64::from(request.dimension)) * ln(point(9.))? + ln(point(2.))?)
                + ln(point(f64::from(request.windows)))?
                - ln(point(request.alpha))?,
        )?;
        let (frobenius_bound, operator_bound) = if request.covariance_bound == 0. {
            (0., 0.)
        } else if request.correlation_bound == 1. {
            // Perfect dependence supplies no gain from increasing n.
            (request.covariance_bound, request.covariance_bound)
        } else {
            let rho = point(request.correlation_bound);
            let rho_squared = rho * rho;
            let n = point(f64::from(request.observations));
            // PSD trace bounds permit the cap before requiring finite output.
            let frobenius_factor = ((point(1.) + rho_squared) / (n * (point(1.) - rho_squared)))
                .sup()
                .min(1.);
            let operator_factor = ((point(1.) + rho) / (n * (point(1.) - rho))).sup().min(1.);
            (
                upper(point(request.covariance_bound) * point(frobenius_factor).sqrt())?,
                upper(point(request.covariance_bound) * point(operator_factor))?,
            )
        };
        let radius = if request.covariance_bound == 0. {
            0.
        } else {
            upper(
                point(4.)
                    * (point(frobenius_bound) * point(log_tail).sqrt()
                        + point(operator_bound) * point(log_tail)),
            )?
        };
        cancel.poll()?;
        Ok(TemporalEnvelope {
            log_tail,
            frobenius_bound,
            operator_bound,
            radius,
        })
    })
}
