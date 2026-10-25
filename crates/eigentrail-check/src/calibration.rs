//! Shared independent nodal checking for continuous and jump routes.
use super::{eigensystem, refusal};
use eigentrail_core::{
    resources::{Cancellation, memory},
    scalar::*,
    *,
};

pub(crate) fn inspect(
    request: &TraceRequest,
    eigensystems: &[Eigensystem],
    radii: &[f64],
    calibration_scope: &str,
    cancel: &Cancellation,
) -> Result<Vec<Matrix>> {
    memory::scope(|| {
        let matrices = eigentrail_core::calibration::normalized(request)?;
        let expected_scope = match &request.calibration {
            Calibration::Conditional { assumptions, .. } => {
                memory::format(format_args!("conditional: {assumptions}"))?
            }
            Calibration::Gaussian {
                estimated_mean,
                alpha,
                auxiliary_risk,
                ..
            } => memory::format(format_args!(
                "Gaussian within-date iid homogeneous observations; {} mean; arbitrary cross-date dependence; simultaneous failure <= {} including auxiliary risk {}",
                if *estimated_mean {
                    "estimated"
                } else {
                    "known"
                },
                alpha,
                auxiliary_risk
            ))?,
        };
        if calibration_scope != expected_scope {
            return Err(refusal("unsupported calibration promotion"));
        }
        let m = matrices.len();
        let p = request.features.len();
        if eigensystems.len() != m || radii.len() != m {
            return Err(refusal("incomplete records"));
        }
        for (j, matrix) in matrices.iter().enumerate() {
            eigensystem::inspect(matrix, &eigensystems[j], cancel)?;
            let e = &eigensystems[j];
            if !eigensystem::psd_checked(matrix, e, cancel)? {
                return Err(refusal("PSD not checked"));
            }
        }
        // Independently reconstruct the calibration radius from the declared actual law.
        for j in 0..m {
            let statistical = match &request.calibration {
                Calibration::Conditional { radii, .. } => *radii
                    .get(j)
                    .ok_or_else(|| refusal("missing conditional radius"))?,
                Calibration::Gaussian {
                    observations,
                    estimated_mean,
                    alpha,
                    auxiliary_risk,
                    ..
                } => {
                    nonnegative(*auxiliary_risk, "auxiliary risk")?;
                    if !alpha.is_finite() || *alpha <= *auxiliary_risk || *alpha >= 1. {
                        return Err(refusal("invalid Gaussian event budget"));
                    }
                    let nu = observations
                        .checked_sub(u64::from(*estimated_mean))
                        .filter(|n| *n > 0)
                        .ok_or_else(|| refusal("Gaussian degrees"))?;
                    let delta = (point(*alpha) - point(*auxiliary_risk)) / point(m as f64);
                    if delta.inf() <= 0. || delta.sup() >= 1. {
                        return Err(refusal("event budget"));
                    }
                    let a = (point(p as f64).sqrt() + (point(2.) * ln(point(2.) / delta)?).sqrt())
                        / point(nu as f64).sqrt();
                    if a.sup() >= 1. {
                        return Err(refusal("calibration inversion gate"));
                    }
                    let e = &eigensystems[j];
                    let norm = e.values.iter().fold(0f64, |s, x| s.max(x.abs()));
                    ((a * (point(2.) + a) / square(point(1.) - a))
                        * (point(norm)
                            + point(e.backward)
                            + point(eigentrail_core::calibration::normalized_error(request, j)?)
                            + point(request.correction[j])))
                    .sup()
                }
            };
            let radius = (point(statistical)
                + point(request.correction[j])
                + point(eigentrail_core::calibration::normalized_error(request, j)?))
            .sup();
            if radii[j] != radius || !radius.is_finite() {
                return Err(refusal("incorrect radius or uncharged correction"));
            }
        }
        Ok(matrices)
    })
}
