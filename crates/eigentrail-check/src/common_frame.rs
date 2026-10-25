//! Independent weighted-anchor and persistent-label checks.
use eigentrail_core::{
    common_frame::{CommonFrameAssessment, CommonFrameModel, CommonFrameReport, SCOPE},
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
fn bad(reason: &str) -> Error {
    Error::Invalid(format!("common-frame checker: {reason}"))
}
fn anchor(
    request: &TraceRequest,
    model: &CommonFrameModel,
    eigensystems: &[Eigensystem],
    radii: &[f64],
    calibration_scope: &str,
    axes: &Eigensystem,
    cancel: &Cancellation,
) -> Result<(Vec<Matrix>, f64, f64)> {
    memory::scope(|| {
        let m = request.times.len();
        let p = request.features.len();
        if model.weights.len() != m || model.weights.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(bad("nonnegative weights over all intended dates"));
        }
        let total = model.weights.iter().fold(point(0.), |s, &w| s + point(w));
        if total.inf() <= 0. {
            return Err(bad("positive weight total required"));
        }
        upper(total)?;
        let matrices =
            crate::calibration::inspect(request, eigensystems, radii, calibration_scope, cancel)?;
        let mut anchor = Matrix {
            n: p,
            data: resources::zeros(p * p)?,
        };
        let mut rounding = point(0.);
        for (i, value) in anchor.data.iter_mut().enumerate() {
            cancel.poll()?;
            let mut enclosed = point(0.);
            for (date, &weight) in model.weights.iter().enumerate() {
                enclosed += point(weight) / total * point(matrices[date].data[i]);
            }
            *value = enclosed.inf() / 2. + enclosed.sup() / 2.;
            if !value.is_finite() {
                return Err(Error::Exhausted("checked weighted anchor overflow".into()));
            }
            rounding += square(enclosed - point(*value));
        }
        crate::eigensystem::inspect(&anchor, axes, cancel)?;
        let noise = model
            .weights
            .iter()
            .zip(radii)
            .fold(rounding.sqrt(), |s, (&w, &r)| {
                s + point(w) / total * point(r)
            });
        let r = upper(noise + point(axes.backward))?;
        let mut gap = f64::MAX;
        for adjacent in axes.values.windows(2) {
            let lower = (point(adjacent[0]) - point(adjacent[1]) - point(2.) * point(r)).inf();
            if !lower.is_finite() {
                return Err(Error::Exhausted(
                    "checked weighted anchor gap overflow".into(),
                ));
            }
            gap = gap.min(lower);
        }
        Ok((matrices, gap, r))
    })
}

pub fn check_assessment(
    request: &TraceRequest,
    model: &CommonFrameModel,
    assessment: &CommonFrameAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        model.heap_bytes()?,
        assessment.heap_bytes()?,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || match assessment {
        CommonFrameAssessment::Resolved { report } => check(request, model, report, cancel),
        CommonFrameAssessment::UnresolvedAnchor {
            request_hash,
            model_hash,
            eigensystems,
            radii,
            calibration_scope,
            axes,
        } => {
            cancel.poll()?;
            validate(request)?;
            if !model.common_frame_assumed {
                return Err(bad("common-frame structural premise absent"));
            }
            if *request_hash != serialization::digest_with_cancel(request, cancel)?
                || *model_hash != serialization::digest_with_cancel(model, cancel)?
            {
                return Err(bad("unresolved anchor input/model binding"));
            }
            let (_, gap, _) = anchor(
                request,
                model,
                eigensystems,
                radii,
                calibration_scope,
                axes,
                cancel,
            )?;
            if request.features.len() == 1 || gap > 0. {
                return Err(bad("unresolved anchor requires a nonpositive checked gap"));
            }
            cancel.poll()
        }
    })
}

pub fn check(
    request: &TraceRequest,
    model: &CommonFrameModel,
    report: &CommonFrameReport,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        model.heap_bytes()?,
        report.heap_bytes()?,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        let reservation = validate(request)?;
        if !model.common_frame_assumed {
            return Err(Error::Exhausted(
                "common-frame structural premise absent".into(),
            ));
        }
        if report.schema != eigentrail_core::common_frame::SCHEMA
            || !report.completed
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.model_hash != serialization::digest_with_cancel(model, cancel)?
            || report.scope != SCOPE
            || report.managed_reservation != reservation
            || report.resource_qualification
                != "pending audit of native allocations and interruption latency"
        {
            return Err(bad(
                "schema, input/model binding, scope, completion or resources",
            ));
        }
        let m = request.times.len();
        let p = request.features.len();
        let axes = &report.frame.axes;
        let (matrices, gap, r) = anchor(
            request,
            model,
            &report.eigensystems,
            &report.radii,
            &report.calibration_scope,
            axes,
            cancel,
        )?;
        if p > 1 && gap <= 0. {
            return Err(bad(
                "individual axes require strict separation of the complete weighted anchor",
            ));
        }
        let d = point(axes.polar);
        let error = if p == 1 {
            0.
        } else {
            (point(2.) * point(r) / point(gap) + (point(2.) + d) * d)
                .sup()
                .min(1.)
        };
        let frame = &report.frame;
        if frame.axis_error != error
            || frame.labelled_strengths.len() != m
            || frame.possible_ranks.len() != m
        {
            return Err(bad("axis bound or missing dates"));
        }
        for (date, matrix) in matrices.iter().enumerate() {
            memory::scope(|| {
                cancel.poll()?;
                if frame.labelled_strengths[date].len() != p
                    || frame.possible_ranks[date].len() != p
                {
                    return Err(bad("missing persistent labels"));
                }
                let e = &report.eigensystems[date];
                let norm =
                    point(e.values.iter().fold(0f64, |s, x| s.max(x.abs()))) + point(e.backward);
                let statistical = point(report.radii[date])
                    + (norm + point(report.radii[date])) * square(point(error));
                let display = (point(2.) * d + square(d)) * norm;
                let mut row = memory::capacity(p)?;
                for label in 0..p {
                    let mut rayleigh = point(0.);
                    for x in 0..p {
                        for y in 0..p {
                            rayleigh += point(axes.vectors.at(x, label))
                                * point(matrix.at(x, y))
                                * point(axes.vectors.at(y, label));
                        }
                    }
                    row.push(Range::closed(
                        (rayleigh - statistical - display).inf().max(0.),
                        upper(rayleigh + statistical + display)?,
                    ));
                }
                if frame.labelled_strengths[date] != row {
                    return Err(bad("labelled Rayleigh or polar/numerical bound"));
                }
                for (label, range) in row.iter().enumerate() {
                    let lower = 1 + row
                        .iter()
                        .enumerate()
                        .filter(|(other, x)| *other != label && x.lower > range.upper)
                        .count();
                    let upper = p - row
                        .iter()
                        .enumerate()
                        .filter(|(other, x)| *other != label && x.upper < range.lower)
                        .count();
                    if frame.possible_ranks[date][label] != [lower, upper] {
                        return Err(bad("persistent rank or omitted rank competitor"));
                    }
                }

                Ok(())
            })?;
        }
        cancel.poll()?;
        Ok(())
    })
}
