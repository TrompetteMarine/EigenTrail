//! Independent conditional conversion from operator tubes to SPD geometry.
use eigentrail_core::{
    Error, Result,
    geometry::{SpdTubeEnvelope, SpdTubeRequest},
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::{ln, point, upper},
    serialization,
};
use serde::Serialize;

pub const SCHEMA: &str = "eigentrail.spd-tube.v1";
pub const SCOPE: &str = "conditional deterministic conversion in fixed feature coordinates; the supplied centre_floor is a lower bound for the minimum eigenvalue of an unprovided SPD centre, and radius is an operator-norm error premise; neither eigenvalues, the probability law nor an entire curve are verified here; affine-invariant metric g_A(U,V)=tr(A^-1 U A^-1 V), unscaled by the Gaussian Fisher factor one half; radius at or above the floor leaves intrinsic-SPD and inverse bounds unresolved; no jitter or target change";

fn refusal(message: &str) -> Error {
    Error::Invalid(format!("SPD tube checker: {message}"))
}

pub fn check_spd(
    request: &SpdTubeRequest,
    envelope: &SpdTubeEnvelope,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        cancel.poll()?;
        if request.dimension == 0
            || !request.centre_floor.is_finite()
            || request.centre_floor <= 0.
            || !request.radius.is_finite()
            || request.radius < 0.
        {
            return Err(refusal(
                "positive dimension and floor, nonnegative finite radius required",
            ));
        }
        if request.radius >= request.centre_floor {
            if envelope.resolved
                || envelope.affine_invariant_radius.is_some()
                || envelope.precision_radius.is_some()
            {
                return Err(refusal("SPD margin is unresolved"));
            }
            return Ok(());
        }
        let (intrinsic, inverse) = if request.radius == 0. {
            (0., 0.)
        } else {
            let floor = point(request.centre_floor);
            let gap = floor - point(request.radius);
            let log_ratio = ln(floor)? - ln(gap)?;
            (
                upper(point(f64::from(request.dimension)).sqrt() * log_ratio)?,
                upper((point(request.radius) / floor) / gap)?,
            )
        };
        if !envelope.resolved
            || envelope.affine_invariant_radius != Some(intrinsic)
            || envelope.precision_radius != Some(inverse)
        {
            return Err(refusal("resolved flag or numerical bounds differ"));
        }
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(untagged)]
enum WireBound {
    Finite(f64),
    Undefined {
        kind: &'static str,
        value: &'static str,
    },
}

impl From<Option<f64>> for WireBound {
    fn from(value: Option<f64>) -> Self {
        match value {
            Some(value) => Self::Finite(value),
            None => Self::Undefined {
                kind: "Undefined",
                value: "SPD margin unresolved",
            },
        }
    }
}

#[derive(Serialize)]
struct WireEnvelope {
    resolved: bool,
    affine_invariant_radius: WireBound,
    precision_radius: WireBound,
}

#[derive(Serialize)]
struct CheckedSpd<'a> {
    schema: &'static str,
    scope: &'static str,
    request: &'a SpdTubeRequest,
    envelope: WireEnvelope,
}

pub fn encode_checked_spd(request: &SpdTubeRequest, cancel: &Cancellation) -> Result<Vec<u8>> {
    memory::ensure_operation(resources::DEFAULT_BYTES, request.heap_bytes()?, || {
        let envelope = eigentrail_core::geometry::analyze_spd(request, cancel)?;
        check_spd(request, &envelope, cancel)?;
        serialization::encode_with_cancel(
            &CheckedSpd {
                schema: SCHEMA,
                scope: SCOPE,
                request,
                envelope: WireEnvelope {
                    resolved: envelope.resolved,
                    affine_invariant_radius: envelope.affine_invariant_radius.into(),
                    precision_radius: envelope.precision_radius.into(),
                },
            },
            cancel,
        )
    })
}
