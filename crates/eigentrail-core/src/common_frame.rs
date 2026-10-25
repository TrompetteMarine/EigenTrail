//! Common-frame inference is conditional on a supplied common-eigenframe model.
use crate::{resources::Cancellation, scalar::*, *};
use serde::{Deserialize, Serialize};
pub const SCHEMA: &str = "eigentrail.common-frame.v1";
pub const SCOPE: &str = "conditional on a common eigenframe throughout the declared continuous covariance path; a simple weighted anchor does not establish that premise; labels are anchor axes, not instantaneous PC ranks";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFrameModel {
    pub weights: Vec<f64>,
    pub common_frame_assumed: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFrameReport {
    pub schema: String,
    pub request_hash: String,
    pub model_hash: String,
    pub eigensystems: Vec<Eigensystem>,
    pub radii: Vec<f64>,
    pub calibration_scope: String,
    pub frame: CommonFrame,
    pub scope: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CommonFrameAssessment {
    Resolved {
        report: CommonFrameReport,
    },
    UnresolvedAnchor {
        request_hash: String,
        model_hash: String,
        eigensystems: Vec<Eigensystem>,
        radii: Vec<f64>,
        calibration_scope: String,
        axes: Eigensystem,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFrame {
    pub axes: Eigensystem,
    pub axis_error: f64,
    pub labelled_strengths: Vec<Vec<Range>>,
    pub possible_ranks: Vec<Vec<[usize; 2]>>,
}
pub fn common_frame(
    request: &TraceRequest,
    weights: &[f64],
    model_assumed: bool,
    cancel: &Cancellation,
) -> Result<CommonFrame> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?
            .checked_add(
                weights
                    .len()
                    .checked_mul(std::mem::size_of::<f64>())
                    .ok_or_else(|| Error::Resource("borrowed weights size overflow".into()))?,
            )
            .ok_or_else(|| Error::Resource("borrowed weights size overflow".into()))?,
        || {
            if !model_assumed {
                return Err(Error::Exhausted(
                    "common frame is a structural premise, not inferred from an anchor".into(),
                ));
            }
            validate(request)?;
            if weights.len() != request.times.len() {
                return Err(Error::Invalid("one anchor weight per date required".into()));
            }
            let mut owned = resources::zeros(weights.len())?;
            owned.copy_from_slice(weights);
            Ok(analyze(
                request,
                &CommonFrameModel {
                    weights: owned,
                    common_frame_assumed: model_assumed,
                },
                cancel,
            )?
            .frame)
        },
    )
}
pub fn analyze(
    request: &TraceRequest,
    model: &CommonFrameModel,
    cancel: &Cancellation,
) -> Result<CommonFrameReport> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?
            .checked_add(crate::resources::memory::HeapBytes::heap_bytes(model)?)
            .ok_or_else(|| Error::Resource("borrowed model size overflow".into()))?,
        || match assess(request, model, cancel)? {
            CommonFrameAssessment::Resolved { report } => Ok(report),
            CommonFrameAssessment::UnresolvedAnchor { .. } => Err(Error::Exhausted(
                "common-frame anchor has unresolved axes; retain its block".into(),
            )),
        },
    )
}

pub fn validate_model(model: &CommonFrameModel, dates: usize) -> Result<()> {
    if !model.common_frame_assumed {
        return Err(Error::Exhausted(
            "common frame is a structural premise, not inferred from an anchor".into(),
        ));
    }
    if model.weights.len() != dates || model.weights.iter().any(|w| !w.is_finite() || *w < 0.) {
        return Err(Error::Invalid("nonnegative anchor weights required".into()));
    }
    let total = model.weights.iter().fold(point(0.), |s, &w| s + point(w));
    if total.inf() <= 0. {
        return Err(Error::Invalid("positive total anchor weight".into()));
    }
    upper(total)?;
    Ok(())
}

pub fn assess(
    request: &TraceRequest,
    model: &CommonFrameModel,
    cancel: &Cancellation,
) -> Result<CommonFrameAssessment> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?
            .checked_add(crate::resources::memory::HeapBytes::heap_bytes(model)?)
            .ok_or_else(|| Error::Resource("borrowed model size overflow".into()))?,
        || {
            cancel.poll()?;
            let reservation = validate(request)?;
            validate_model(model, request.times.len())?;
            let weights = &model.weights;
            let m = request.times.len();
            let p = request.features.len();
            let total = weights.iter().fold(point(0.), |s, &w| s + point(w));
            let matrices = calibration::normalized(request)?;
            let mut eigen = resources::memory::capacity(m)?;
            for a in &matrices {
                let e = numerics::eigensystem(a, cancel)?;
                if !histories::try_verified_psd(a, &e)? {
                    return Err(Error::Exhausted("PSD anchor inputs unverified".into()));
                }
                eigen.push(e);
            }
            let (radii, calibration_scope) = calibration::radii(request, &eigen)?;
            let mut anchor = Matrix {
                n: p,
                data: resources::zeros(p * p)?,
            };
            let mut round = point(0.);
            for i in 0..p * p {
                cancel.poll()?;
                let exact = (0..m).fold(point(0.), |s, j| {
                    s + point(weights[j]) / total * point(matrices[j].data[i])
                });
                anchor.data[i] = exact.inf() / 2. + exact.sup() / 2.;
                if !anchor.data[i].is_finite() {
                    return Err(Error::Exhausted("weighted anchor overflow".into()));
                }
                round += square(exact - point(anchor.data[i]));
            }
            let axes = numerics::eigensystem(&anchor, cancel)?;
            let noise = (0..m).fold(round.sqrt(), |s, j| {
                s + point(weights[j]) / total * point(radii[j])
            });
            let r = upper(noise + point(axes.backward))?;
            let mut gap = f64::MAX;
            for i in 0..p - 1 {
                let lower =
                    (point(axes.values[i]) - point(axes.values[i + 1]) - point(2.) * point(r))
                        .inf();
                if !lower.is_finite() {
                    return Err(Error::Exhausted("weighted anchor gap overflow".into()));
                }
                gap = gap.min(lower);
            }
            if p > 1 && gap <= 0. {
                cancel.poll()?;
                return Ok(CommonFrameAssessment::UnresolvedAnchor {
                    request_hash: serialization::digest_with_cancel(request, cancel)?,
                    model_hash: serialization::digest_with_cancel(model, cancel)?,
                    eigensystems: eigen,
                    radii,
                    calibration_scope,
                    axes,
                });
            }
            let polar = (point(2.) + point(axes.polar)) * point(axes.polar);
            let axis_error = if p == 1 {
                0.
            } else {
                (point(2.) * point(r) / point(gap) + polar).sup().min(1.)
            };
            let mut strengths = resources::memory::capacity(m)?;
            let mut ranks = resources::memory::capacity(m)?;
            for j in 0..m {
                cancel.poll()?;
                let a = &matrices[j];
                let e = &eigen[j];
                let norm =
                    point(e.values.iter().fold(0f64, |s, v| s.max(v.abs()))) + point(e.backward);
                let statistical =
                    point(radii[j]) + (norm + point(radii[j])) * square(point(axis_error));
                let display = (point(2.) * point(axes.polar) + square(point(axes.polar))) * norm;
                let mut row = resources::memory::capacity(p)?;
                for mode in 0..p {
                    let mut rayleigh = point(0.);
                    for x in 0..p {
                        for y in 0..p {
                            rayleigh += point(axes.vectors.at(x, mode))
                                * point(a.at(x, y))
                                * point(axes.vectors.at(y, mode));
                        }
                    }
                    row.push(Range::closed(
                        (rayleigh - statistical - display).inf().max(0.),
                        upper(rayleigh + statistical + display)?,
                    ));
                }
                let mut rank_row = resources::memory::capacity(p)?;
                for s in &row {
                    rank_row.push([
                        1 + row.iter().filter(|x| x.lower > s.upper).count(),
                        p - row.iter().filter(|x| x.upper < s.lower).count(),
                    ]);
                }
                ranks.push(rank_row);
                strengths.push(row);
            }
            cancel.poll()?;
            Ok(CommonFrameAssessment::Resolved {
                report: CommonFrameReport {
                    schema: resources::memory::text(SCHEMA)?,
                    request_hash: serialization::digest_with_cancel(request, cancel)?,
                    model_hash: serialization::digest_with_cancel(model, cancel)?,
                    eigensystems: eigen,
                    radii,
                    calibration_scope,
                    frame: CommonFrame {
                        axes,
                        axis_error,
                        labelled_strengths: strengths,
                        possible_ranks: ranks,
                    },
                    scope: resources::memory::text(SCOPE)?,
                    completed: true,
                    managed_reservation: reservation,
                    resource_qualification: resources::memory::text(
                        "pending audit of native allocations and interruption latency",
                    )?,
                },
            })
        },
    )
}
