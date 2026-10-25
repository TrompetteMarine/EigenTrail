use crate::{Error, Result, resources::memory, scalar::*, types::*};

pub fn degrees(observations: u64, estimated_mean: bool) -> Result<u64> {
    observations
        .checked_sub(u64::from(estimated_mean))
        .filter(|n| *n > 0 && *n <= 1u64 << 53)
        .ok_or_else(|| Error::Invalid("invalid Gaussian degrees of freedom".into()))
}
pub fn multiplier(p: usize, nu: u64, delta: Interval) -> Result<Interval> {
    if p == 0 || nu == 0 || nu > 1u64 << 53 || delta.inf() <= 0. || delta.sup() >= 1. {
        return Err(Error::Invalid(
            "invalid Gaussian calibration parameters".into(),
        ));
    }
    let a = (point(p as f64).sqrt() + (point(2.) * ln(point(2.) / delta)?).sqrt())
        / point(nu as f64).sqrt();
    if a.sup() >= 1. {
        return Err(Error::Exhausted(
            "Gaussian inversion gate a<1 unresolved".into(),
        ));
    }
    let ell = square(point(1.) - a);
    // (1+a)^2-1 = a(2+a) avoids cancellation for large degrees of freedom.
    Ok(a * (point(2.) + a) / ell)
}
pub fn scale_envelope(p: usize, dates: usize, nu: u64, alpha: f64, scale: f64) -> Result<f64> {
    nonnegative(scale, "scale")?;
    let delta = point(alpha) / point(dates as f64);
    let a = (point(p as f64).sqrt() + (point(2.) * ln(point(2.) / delta)?).sqrt())
        / point(nu as f64).sqrt();
    upper(multiplier(p, nu, delta)? * square(point(1.) + a) * point(scale))
}
pub fn normalized(request: &TraceRequest) -> Result<Vec<Matrix>> {
    crate::resources::memory::scope(|| {
        let factor = match request.calibration {
            Calibration::Gaussian {
                observations,
                estimated_mean,
                divisor,
                ..
            } => {
                if !divisor.is_finite() || divisor <= 0. {
                    return Err(Error::Invalid("positive scatter divisor required".into()));
                }
                divisor / degrees(observations, estimated_mean)? as f64
            }
            _ => 1.,
        };
        if !factor.is_finite() {
            return Err(Error::Invalid("normalization factor overflow".into()));
        }
        // Normalization rounding is charged by normalized_error, rather than silently exact.
        let mut out = memory::capacity(request.covariances.len())?;
        for a in &request.covariances {
            let mut data = memory::capacity(a.data.len())?;
            for &x in &a.data {
                memory::push(&mut data, x * factor)?;
            }
            memory::push(&mut out, Matrix { n: a.n, data })?;
        }
        if out.iter().any(|a| a.data.iter().any(|x| !x.is_finite())) {
            return Err(Error::Invalid("normalization overflow".into()));
        }
        Ok(out)
    })
}
pub fn normalized_error(request: &TraceRequest, j: usize) -> Result<f64> {
    match request.calibration {
        Calibration::Conditional { .. } => return Ok(0.),
        Calibration::Gaussian {
            observations,
            estimated_mean,
            divisor,
            ..
        } if divisor == degrees(observations, estimated_mean)? as f64 => return Ok(0.),
        _ => {}
    }
    let (factor, rounded_factor) = match request.calibration {
        Calibration::Gaussian {
            observations,
            estimated_mean,
            divisor,
            ..
        } => {
            let nu = degrees(observations, estimated_mean)? as f64;
            (point(divisor) / point(nu), divisor / nu)
        }
        _ => (point(1.), 1.),
    };
    let mut sum = point(0.);
    for &x in &request.covariances[j].data {
        let exact = point(x) * factor;
        let rounded = x * rounded_factor;
        if !rounded.is_finite() {
            return Err(Error::Invalid("normalization overflow".into()));
        }
        let e = magnitude(exact - point(rounded));
        sum += square(point(e));
    }
    upper(sum.sqrt())
}
pub fn radii(request: &TraceRequest, eig: &[Eigensystem]) -> Result<(Vec<f64>, String)> {
    crate::resources::memory::scope(|| {
        let (mut out, scope) = match &request.calibration {
            Calibration::Conditional { radii, assumptions } => {
                if radii.len() != request.times.len() || assumptions.trim().is_empty() {
                    return Err(Error::Invalid(
                        "conditional radii and assumptions required".into(),
                    ));
                }
                (
                    memory::copy(radii)?,
                    memory::format(format_args!("conditional: {assumptions}"))?,
                )
            }
            Calibration::Gaussian {
                observations,
                estimated_mean,
                alpha,
                auxiliary_risk,
                ..
            } => {
                nonnegative(*auxiliary_risk, "auxiliary risk")?;
                if !alpha.is_finite() || *alpha <= *auxiliary_risk || *alpha >= 1. {
                    return Err(Error::Invalid(
                        "alpha must exceed all auxiliary event risks and be <1".into(),
                    ));
                }
                let delta = (point(*alpha) - point(*auxiliary_risk)) / point(eig.len() as f64);
                let mult = multiplier(
                    request.features.len(),
                    degrees(*observations, *estimated_mean)?,
                    delta,
                )?;
                let mut out = memory::capacity(eig.len())?;
                for (j, e) in eig.iter().enumerate() {
                    let norm = e.values.iter().map(|x| x.abs()).fold(0., f64::max);
                    // Charge the norm perturbation inside the data-dependent radius too.
                    out.push(upper(
                        mult * (point(norm)
                            + point(e.backward)
                            + point(normalized_error(request, j)?)
                            + point(request.correction[j])),
                    )?);
                }
                (
                    out,
                    memory::format(format_args!(
                        "Gaussian within-date iid homogeneous observations; {} mean; arbitrary cross-date dependence; simultaneous failure <= {} including auxiliary risk {}",
                        if *estimated_mean {
                            "estimated"
                        } else {
                            "known"
                        },
                        alpha,
                        auxiliary_risk
                    ))?,
                )
            }
        };
        for (j, r) in out.iter_mut().enumerate() {
            nonnegative(*r, "radius")?;
            nonnegative(request.correction[j], "correction")?;
            *r = upper(
                point(*r) + point(request.correction[j]) + point(normalized_error(request, j)?),
            )?;
        }
        Ok((out, scope))
    })
}
