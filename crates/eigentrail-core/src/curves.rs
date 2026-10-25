use crate::{Result, resources::memory, scalar::*, types::*};
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentreEvaluation {
    pub schema: String,
    pub request_hash: String,
    pub segment: usize,
    pub time: f64,
    pub evaluation_error: f64,
    pub eigensystem: Eigensystem,
}
pub fn evaluate(
    request: &TraceRequest,
    segment: usize,
    time: f64,
    cancel: &crate::resources::Cancellation,
) -> Result<CentreEvaluation> {
    cancel.poll()?;
    crate::validate(request)?;
    crate::resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?,
        || {
            crate::validate(request)?;
            if !time.is_finite()
                || segment
                    .checked_add(1)
                    .is_none_or(|k| k >= request.times.len())
                || time < request.times[segment]
                || time > request.times[segment + 1]
            {
                return Err(crate::Error::Invalid(
                    "evaluation time inside a valid smooth segment required".into(),
                ));
            }
            let matrices = crate::calibration::normalized(request)?;
            let n = request.features.len();
            let fraction = (point(time) - point(request.times[segment]))
                / (point(request.times[segment + 1]) - point(request.times[segment]));
            let mut data = crate::resources::zeros(n * n)?;
            let mut error = point(0.);
            for (i, value) in data.iter_mut().enumerate() {
                let exact = (point(1.) - fraction) * point(matrices[segment].data[i])
                    + fraction * point(matrices[segment + 1].data[i]);
                *value = exact.inf() / 2. + exact.sup() / 2.;
                if !value.is_finite() {
                    return Err(crate::Error::Exhausted(
                        "interior evaluation overflow".into(),
                    ));
                }
                error += square(exact - point(*value));
            }
            let normalization = crate::calibration::normalized_error(request, segment)?
                .max(crate::calibration::normalized_error(request, segment + 1)?);
            let evaluation_error = upper(error.sqrt() + point(normalization))?;
            let eigensystem = crate::numerics::eigensystem(&Matrix { n, data }, cancel)?;
            Ok(CentreEvaluation {
                schema: memory::text(SCHEMA)?,
                request_hash: crate::serialization::digest_with_cancel(request, cancel)?,
                segment,
                time,
                evaluation_error,
                eigensystem,
            })
        },
    )
}
pub fn gap(a: f64, c: f64, k: f64, dt: f64) -> f64 {
    let drift = point(k) * point(dt);
    (point(a) - point(2.) * drift)
        .inf()
        .max((point(c) - point(2.) * drift).inf())
        .max(((point(a) + point(c)) / point(2.) - drift).inf())
}
pub fn tubes(
    a: &Eigensystem,
    b: &Eigensystem,
    ea: f64,
    eb: f64,
    k: f64,
    dt: f64,
    target: f64,
) -> Result<Vec<Tube>> {
    crate::resources::memory::scope(|| {
        let n = a.values.len();
        let eta = upper(point(ea.max(eb)) + point(k) * point(dt) / point(2.))?;
        let numerical = upper(point(a.backward.max(b.backward)))?;
        let total = upper(point(eta) + point(numerical))?;
        let mut boundaries = memory::capacity(n.saturating_sub(1))?;
        for i in 0..n - 1 {
            let ga = (point(a.values[i])
                - point(a.values[i + 1])
                - point(2.) * (point(ea) + point(a.backward)))
            .inf();
            let gb = (point(b.values[i])
                - point(b.values[i + 1])
                - point(2.) * (point(eb) + point(b.backward)))
            .inf();
            boundaries.push(gap(ga, gb, k, dt));
        }
        let mut out = memory::capacity(n)?;
        let mut start = 0;
        for end in 1..=n {
            if end < n && boundaries[end - 1] <= 2. * total {
                continue;
            }
            let g = if start == 0 && end == n {
                0.
            } else {
                let left = if start > 0 {
                    boundaries[start - 1]
                } else {
                    f64::MAX
                };
                let right = if end < n {
                    boundaries[end - 1]
                } else {
                    f64::MAX
                };
                left.min(right)
            };
            let bound = if start == 0 && end == n {
                0.
            } else {
                let polar = point(a.polar.max(b.polar));
                upper(
                    point(2.) * point((end - start) as f64).sqrt() * point(total) / point(g)
                        + (point(2.) + polar) * polar,
                )?
                .min(1.)
            };
            out.push(Tube {
                start,
                end,
                eta: total,
                gap: g,
                bound,
                informative: bound < target,
            });
            start = end;
        }
        Ok(out)
    })
}
