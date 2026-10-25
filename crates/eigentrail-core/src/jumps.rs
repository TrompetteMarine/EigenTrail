use crate::{Error, Result, scalar::*, types::Range};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "eigentrail.jumps.v1";
pub const SCOPE: &str = "cadlag covariance paths; declared between-jump K and at-most-one jump per half-open cell; unflagged cells retain jumps unless a candidate-wide minimum signal proves exclusion; no continuous reconstruction across retained jump cells";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JumpCell {
    pub flagged: bool,
    pub amplitude: Range,
    pub timing: Range,
    pub alternative: String,
}
/// d_minus <= ||S_right-S_left|| <= d_minus+zeta, independently checked.
pub fn cell(
    left: f64,
    right: f64,
    d_minus: f64,
    zeta: f64,
    e_left: f64,
    e_right: f64,
    k: f64,
) -> Result<JumpCell> {
    for (x, n) in [
        (d_minus, "norm lower"),
        (zeta, "norm width"),
        (e_left, "left radius"),
        (e_right, "right radius"),
        (k, "drift"),
    ] {
        nonnegative(x, n)?;
    }
    if !left.is_finite() || !right.is_finite() || left >= right {
        return Err(Error::Invalid("ordered jump cell".into()));
    }
    let drift = point(k) * (point(right) - point(left));
    let noise = point(e_left) + point(e_right);
    let threshold = upper(drift + noise)?;
    let flagged = d_minus > threshold;
    let amplitude = Range::closed(
        (point(d_minus) - noise - drift).inf().max(0.),
        upper(point(d_minus) + point(zeta) + noise + drift)?,
    );
    Ok(JumpCell {
        flagged,
        amplitude,
        timing: Range {
            lower: left,
            upper: right,
            lower_closed: false,
            upper_closed: true,
        },
        alternative: crate::resources::memory::text(if flagged {
            "at least one jump under declared spacing model"
        } else {
            "smooth and hidden-jump candidates retained"
        })?,
    })
}
pub fn minimum_signal_power(
    jump: f64,
    k: f64,
    dt: f64,
    e_left: f64,
    e_right: f64,
    zeta: f64,
) -> bool {
    if [jump, k, dt, e_left, e_right, zeta]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return false;
    }
    jump > (point(2.) * point(k) * point(dt)
        + point(2.) * (point(e_left) + point(e_right))
        + point(zeta))
    .sup()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JumpModel {
    pub at_most_one_per_cell: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_jump: Option<f64>,
    pub minimum_applies_to_all_candidates: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JumpFamily {
    pub cells: Vec<JumpCell>,
    pub mandatory: Vec<bool>,
    pub optional: Vec<bool>,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JumpReport {
    pub schema: String,
    pub request_hash: String,
    pub model_hash: String,
    pub eigensystems: Vec<crate::Eigensystem>,
    pub radii: Vec<f64>,
    pub differences: Vec<crate::Eigensystem>,
    pub family: JumpFamily,
    pub calibration_scope: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}
pub fn analyze(
    request: &crate::TraceRequest,
    model: &JumpModel,
    cancel: &crate::resources::Cancellation,
) -> Result<JumpFamily> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?,
        || Ok(analyze_with_cancel(request, model, cancel)?.family),
    )
}
pub fn analyze_with_cancel(
    request: &crate::TraceRequest,
    model: &JumpModel,
    cancel: &crate::resources::Cancellation,
) -> Result<JumpReport> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?,
        || {
            cancel.poll()?;
            let reservation = crate::validate(request)?;
            if !model.at_most_one_per_cell {
                return Err(Error::Invalid(
                    "jump detection requires at most one jump per observation cell".into(),
                ));
            }
            if let Some(j) = model.minimum_jump {
                nonnegative(j, "minimum jump")?;
            }
            let matrices = crate::calibration::normalized(request)?;
            let mut eig = crate::resources::memory::capacity(matrices.len())?;
            for a in &matrices {
                let e = crate::numerics::eigensystem(a, cancel)?;
                if !crate::histories::try_verified_psd(a, &e)? {
                    return Err(Error::Exhausted("jump PSD input".into()));
                }
                eig.push(e);
            }
            let (radii, calibration_scope) = crate::calibration::radii(request, &eig)?;
            let mut differences = crate::resources::memory::capacity(matrices.len() - 1)?;
            let mut cells = crate::resources::memory::capacity(matrices.len() - 1)?;
            let mut mandatory = crate::resources::memory::capacity(matrices.len() - 1)?;
            let mut optional = crate::resources::memory::capacity(matrices.len() - 1)?;
            for j in 0..matrices.len() - 1 {
                cancel.poll()?;
                let a = &matrices[j];
                let b = &matrices[j + 1];
                let n = a.n;
                let mut data = crate::resources::memory::capacity(n * n)?;
                let mut round = point(0.);
                for (&x, &y) in a.data.iter().zip(&b.data) {
                    let d = y - x;
                    if !d.is_finite() {
                        return Err(Error::Exhausted("jump difference overflow".into()));
                    }
                    round += square(point(y) - point(x) - point(d));
                    data.push(d);
                }
                let d = crate::numerics::eigensystem(&crate::Matrix { n, data }, cancel)?;
                let norm = d.values.iter().fold(0f64, |s, x| s.max(x.abs()));
                let lower = (point(norm) - point(d.backward) - round.sqrt())
                    .inf()
                    .max(0.);
                let upper = upper(point(norm) + point(d.backward) + round.sqrt())?;
                let zeta = (point(upper) - point(lower)).sup();
                let c = cell(
                    request.times[j],
                    request.times[j + 1],
                    lower,
                    zeta,
                    radii[j],
                    radii[j + 1],
                    request.lipschitz,
                )?;
                let power = if let Some(signal) = model.minimum_jump {
                    model.minimum_applies_to_all_candidates
                        && minimum_signal_power(
                            signal,
                            request.lipschitz,
                            (point(request.times[j + 1]) - point(request.times[j])).sup(),
                            radii[j],
                            radii[j + 1],
                            zeta,
                        )
                } else {
                    false
                };
                mandatory.push(c.flagged);
                optional.push(!c.flagged && !power);
                cells.push(c);
                differences.push(d);
            }
            cancel.poll()?;
            Ok(JumpReport {
                schema: crate::resources::memory::text(SCHEMA)?,
                request_hash: crate::serialization::digest_with_cancel(request, cancel)?,
                model_hash: crate::serialization::digest_with_cancel(model, cancel)?,
                eigensystems: eig,
                radii,
                differences,
                family: JumpFamily {
                    cells,
                    mandatory,
                    optional,
                    scope: crate::resources::memory::text(SCOPE)?,
                },
                calibration_scope,
                completed: true,
                managed_reservation: reservation,
                resource_qualification: crate::resources::memory::text(
                    "pending audit of native allocations and interruption latency",
                )?,
            })
        },
    )
}

pub struct OneSidedEnvelope {
    pub left_radius: f64,
    pub right_radius: f64,
    pub combination: String,
}
pub fn inside_cell(
    t: f64,
    left: f64,
    right: f64,
    e_left: f64,
    e_right: f64,
    k: f64,
    jump_possible: bool,
) -> Result<OneSidedEnvelope> {
    for (v, n) in [(e_left, "left error"), (e_right, "right error"), (k, "K")] {
        nonnegative(v, n)?;
    }
    if !t.is_finite()
        || !left.is_finite()
        || !right.is_finite()
        || left >= right
        || t < left
        || t > right
    {
        return Err(Error::Invalid("time inside a valid cell required".into()));
    }
    Ok(OneSidedEnvelope {
        left_radius: upper(point(e_left) + point(k) * (point(t) - point(left)))?,
        right_radius: upper(point(e_right) + point(k) * (point(right) - point(t)))?,
        combination: crate::resources::memory::text(if jump_possible {
            "union of PSD balls"
        } else {
            "intersection of PSD balls"
        })?,
    })
}
