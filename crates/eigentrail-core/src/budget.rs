//! Scalar composition of numerical losses; not a producer decision function.
use crate::{scalar::*, *};

fn ideal_radius_lower(request: &TraceRequest, e: &Eigensystem, j: usize) -> Result<(f64, f64)> {
    let normalization = calibration::normalized_error(request, j)?;
    let radius = match &request.calibration {
        Calibration::Conditional { radii, .. } => point(radii[j]) + point(request.correction[j]),
        Calibration::Gaussian {
            observations,
            estimated_mean,
            alpha,
            auxiliary_risk,
            ..
        } => {
            let mu = calibration::multiplier(
                request.features.len(),
                calibration::degrees(*observations, *estimated_mean)?,
                (point(*alpha) - point(*auxiliary_risk)) / point(request.times.len() as f64),
            )?;
            let magnitude = e.values.iter().fold(0f64, |s, x| s.max(x.abs()));
            let norm = (point(magnitude) - point(e.backward) - point(normalization))
                .inf()
                .max(0.);
            mu * (point(norm) + point(request.correction[j])) + point(request.correction[j])
        }
    };
    Ok((radius.inf(), normalization))
}
pub fn numerical_component(
    request: &TraceRequest,
    eigen: &[Eigensystem],
    j: usize,
    tube: &Tube,
) -> Result<f64> {
    let p = request.features.len();
    if tube.start == 0 && tube.end == p {
        return Ok(0.);
    }
    let a = &eigen[j];
    let b = &eigen[j + 1];
    let (ra, za) = ideal_radius_lower(request, a, j)?;
    let (rb, zb) = ideal_radius_lower(request, b, j + 1)?;
    let dt = point(request.times[j + 1]) - point(request.times[j]);
    let drift = point(request.lipschitz) * dt;
    let boundary_upper = |k: usize| {
        let ga = point(a.values[k]) - point(a.values[k + 1])
            + point(2.) * (point(a.backward) + point(za))
            - point(2.) * point(ra);
        let gb = point(b.values[k]) - point(b.values[k + 1])
            + point(2.) * (point(b.backward) + point(zb))
            - point(2.) * point(rb);
        (ga - point(2.) * drift)
            .sup()
            .max((gb - point(2.) * drift).sup())
            .max(((ga + gb) / point(2.) - drift).sup())
    };
    let mut gap_upper = f64::MAX;
    if tube.start > 0 {
        gap_upper = gap_upper.min(boundary_upper(tube.start - 1));
    }
    if tube.end < p {
        gap_upper = gap_upper.min(boundary_upper(tube.end - 1));
    }
    if gap_upper <= 0. {
        return Err(Error::Exhausted(
            "numerical budget has no separated ideal comparison".into(),
        ));
    }
    let eta_lower = point(ra.max(rb)) + drift / point(2.);
    let ideal_lower = (point(2.) * point((tube.end - tube.start) as f64).sqrt() * eta_lower
        / point(gap_upper))
    .inf()
    .min(1.);
    upper((point(tube.bound) - point(ideal_lower)).max(point(0.)))
}
