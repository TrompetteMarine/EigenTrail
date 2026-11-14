//! Independently reconstruct the deterministic window-to-instantaneous envelope.
use eigentrail_core::{
    Error, Range, Result,
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::{point, upper},
    serialization,
    window::{WindowEnvelope, WindowRequest},
};
use serde::Serialize;

pub const SCHEMA: &str = "eigentrail.window-envelope.v1";
pub const SCOPE: &str = "conditional deterministic transfer from the declared window target to instantaneous covariance in fixed feature coordinates; raw weights are normalized as real numbers; target mismatch and operator-norm regularity remain premises; no temporal calibration or simultaneous source event is created; included effects must not be charged again through TRACE correction";

fn refusal(message: &'static str) -> Error {
    Error::Invalid(format!("window envelope checker: {message}"))
}

/// Validate both the observation contract and all numerical output components.
///
/// This routine does not invoke the producer or its validator. It checks the
/// numerical implication, not the sampling law or truth of the supplied bounds.
pub fn check(
    request: &WindowRequest,
    envelope: &WindowEnvelope,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        cancel.poll()?;
        let n = request.sample_times.len();
        if n == 0 || n > 10_000 || request.weights.len() != n {
            return Err(refusal("equal array lengths in 1..=10000 required"));
        }
        if !request.target_time.is_finite()
            || [
                request.window_radius,
                request.lipschitz,
                request.target_mismatch_bound,
            ]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.)
            || [&request.target_description, &request.assumptions]
                .iter()
                .any(|text| text.trim().is_empty() || text.len() > 4096)
        {
            return Err(refusal("invalid time, bounds or observation contract"));
        }
        let mut maximum = 0_f64;
        for (&time, &weight) in request.sample_times.iter().zip(&request.weights) {
            cancel.poll()?;
            if !time.is_finite() || !weight.is_finite() || weight < 0. {
                return Err(refusal(
                    "finite times and nonnegative finite weights required",
                ));
            }
            maximum = maximum.max(weight);
        }
        if maximum == 0. {
            return Err(refusal("at least one positive weight required"));
        }

        let mut mass = point(0.);
        let mut weighted_distance = point(0.);
        for index in 0..n {
            cancel.poll()?;
            if request.weights[index] == 0. {
                continue;
            }
            let scaled = point(request.weights[index]) / point(maximum);
            mass += scaled;
            weighted_distance +=
                scaled * (point(request.sample_times[index]) - point(request.target_time)).abs();
        }
        upper(mass)?;
        upper(weighted_distance)?;
        if mass.inf() <= 0. {
            return Err(Error::Exhausted(
                "positive weight normalization unresolved".into(),
            ));
        }
        let distance = weighted_distance / mass;
        let lower = distance.inf().max(0.);
        let high = upper(distance)?;
        if !lower.is_finite() || lower > high {
            return Err(Error::Exhausted("invalid distance enclosure".into()));
        }
        let drift = upper(point(request.lipschitz) * point(high))?;
        let total = upper(
            point(request.window_radius) + point(request.target_mismatch_bound) + point(drift),
        )?;
        if envelope.mean_absolute_offset != Range::closed(lower, high)
            || envelope.drift_bound != drift
            || envelope.total_radius != total
        {
            return Err(refusal("distance, drift or total radius differs"));
        }
        Ok(())
    })
}

#[derive(Serialize)]
struct CheckedWindow<'a> {
    schema: &'static str,
    scope: &'static str,
    request: &'a WindowRequest,
    envelope: &'a WindowEnvelope,
}

/// Analyze and independently check under the default managed-memory budget.
/// The complete supplied target description and assumptions remain in the output.
pub fn encode_checked(request: &WindowRequest, cancel: &Cancellation) -> Result<Vec<u8>> {
    memory::ensure_operation(resources::DEFAULT_BYTES, request.heap_bytes()?, || {
        let envelope = eigentrail_core::window::analyze(request, cancel)?;
        check(request, &envelope, cancel)?;
        serialization::encode_with_cancel(
            &CheckedWindow {
                schema: SCHEMA,
                scope: SCOPE,
                request,
                envelope: &envelope,
            },
            cancel,
        )
    })
}
