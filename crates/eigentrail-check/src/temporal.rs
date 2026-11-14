//! Independent reconstruction of the conditional temporal Gaussian radius.
use eigentrail_core::{
    Error, Result,
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::{ln, point, upper},
    serialization,
    temporal::{TemporalEnvelope, TemporalRequest},
};
use serde::Serialize;

pub const SCHEMA: &str = "eigentrail.temporal-gaussian.v1";
pub const SCOPE: &str = "finite-sample conditional joint bound for a prespecified family of windows, jointly Gaussian within each window after subtracting known true means; uniform second moment with divisor n; deterministic block covariance envelope and observation ordering are premises, not inferred or validated here; use common windows count and alpha across the whole family; overlap is allowed; no sample centring, fitted dependence envelope, adaptive selection, instantaneous-target transfer, positive-definiteness or unique curve is certified";

fn refusal(message: &str) -> Error {
    Error::Invalid(format!("temporal Gaussian checker: {message}"))
}

/// Reconstruct without calling the producer or its validator. The statistical
/// premises are retained as premises; arithmetic checking cannot establish them.
pub fn check(
    request: &TemporalRequest,
    envelope: &TemporalEnvelope,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        cancel.poll()?;
        if request.dimension == 0
            || request.observations == 0
            || request.windows == 0
            || !request.covariance_bound.is_finite()
            || request.covariance_bound < 0.
            || !request.correlation_bound.is_finite()
            || !(0. ..=1.).contains(&request.correlation_bound)
            || !request.alpha.is_finite()
            || request.alpha <= 0.
            || request.alpha >= 1.
            || request.assumptions.trim().is_empty()
            || request.assumptions.len() > 4096
        {
            return Err(refusal(
                "invalid counts, bounds, risk or assumption declaration",
            ));
        }
        let tail = upper(
            (point(f64::from(request.dimension)) * ln(point(9.))? + ln(point(2.))?)
                + ln(point(f64::from(request.windows)))?
                - ln(point(request.alpha))?,
        )?;
        let (frobenius, operator) = if request.covariance_bound == 0. {
            (0., 0.)
        } else if request.correlation_bound == 1. {
            (request.covariance_bound, request.covariance_bound)
        } else {
            let rho = point(request.correlation_bound);
            let rho_squared = rho * rho;
            let n = point(f64::from(request.observations));
            let norm_squared = ((point(1.) + rho_squared) / (n * (point(1.) - rho_squared)))
                .sup()
                .min(1.);
            let norm = ((point(1.) + rho) / (n * (point(1.) - rho))).sup().min(1.);
            (
                upper(point(request.covariance_bound) * point(norm_squared).sqrt())?,
                upper(point(request.covariance_bound) * point(norm))?,
            )
        };
        let radius = if request.covariance_bound == 0. {
            0.
        } else {
            upper(
                point(4.) * (point(frobenius) * point(tail).sqrt() + point(operator) * point(tail)),
            )?
        };
        if envelope.log_tail != tail
            || envelope.frobenius_bound != frobenius
            || envelope.operator_bound != operator
            || envelope.radius != radius
        {
            return Err(refusal("tail parameter, norm bound or radius differs"));
        }
        Ok(())
    })
}

#[derive(Serialize)]
struct CheckedTemporal<'a> {
    schema: &'static str,
    scope: &'static str,
    request: &'a TemporalRequest,
    envelope: &'a TemporalEnvelope,
}

pub fn encode_checked(request: &TemporalRequest, cancel: &Cancellation) -> Result<Vec<u8>> {
    memory::ensure_operation(resources::DEFAULT_BYTES, request.heap_bytes()?, || {
        let envelope = eigentrail_core::temporal::analyze(request, cancel)?;
        check(request, &envelope, cancel)?;
        serialization::encode_with_cancel(
            &CheckedTemporal {
                schema: SCHEMA,
                scope: SCOPE,
                request,
                envelope: &envelope,
            },
            cancel,
        )
    })
}
