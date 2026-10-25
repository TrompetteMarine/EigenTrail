use eigentrail_core::{
    resources::{Cancellation, memory},
    scalar::*,
    *,
};
/// Compatibility wrapper; qualified checking uses psd_checked.
pub fn psd(a: &Matrix, e: &Eigensystem) -> bool {
    psd_checked(a, e, &Cancellation::default()).expect("fallible PSD checking requires psd_checked")
}
pub fn psd_checked(a: &Matrix, e: &Eigensystem, cancel: &Cancellation) -> Result<bool> {
    memory::scope(|| {
        cancel.poll()?;
        if (point(e.values[a.n - 1]) - point(e.backward)).inf() >= 0. {
            return Ok(true);
        }
        let n = a.n;
        let mut work = memory::capacity(a.data.len())?;
        for &x in &a.data {
            work.push(point(x));
        }
        for pivot_index in 0..n {
            cancel.poll()?;
            let pivot = work[pivot_index * (n + 1)];
            if pivot.inf() == 0. && pivot.sup() == 0. {
                for row in pivot_index + 1..n {
                    let entry = work[row + pivot_index * n];
                    if entry.inf() != 0. || entry.sup() != 0. {
                        return Ok(false);
                    }
                }
            } else if pivot.inf() > 0. {
                for row in pivot_index + 1..n {
                    for col in pivot_index + 1..=row {
                        let value = work[row + col * n]
                            - work[row + pivot_index * n] * work[col + pivot_index * n] / pivot;
                        work[row + col * n] = value;
                        work[col + row * n] = value;
                    }
                }
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    })
}
/// Checker evaluates the full reconstruction independently of the producer.
fn normal_bounds(a: &Matrix, e: &Eigensystem, cancel: &Cancellation) -> Result<Option<(f64, f64)>> {
    let p = a.n;
    let mut norm_squared = point(0.);
    for &entry in &e.vectors.data {
        norm_squared += square(point(entry));
    }
    let product = point((2 * p + 4) as f64) * point(f64::EPSILON);
    let rounding = product / (point(1.) - product) * norm_squared;
    let scale = e.values.iter().fold(0f64, |s, x| s.max(x.abs()));
    let mut gram_error = point(0.);
    let mut reconstruction_error = point(0.);
    for row in 0..p {
        cancel.poll()?;
        for col in 0..p {
            let Some(gram) = normal_product_sum(
                (0..p).map(|k| (e.vectors.at(k, row), e.vectors.at(k, col), 1.)),
            ) else {
                return Ok(None);
            };
            let Some(rebuilt) = normal_product_sum(
                (0..p).map(|k| (e.values[k], e.vectors.at(row, k), e.vectors.at(col, k))),
            ) else {
                return Ok(None);
            };
            gram_error += square(point(gram) - point(f64::from(row == col)));
            reconstruction_error += square(point(rebuilt) - point(a.at(row, col)));
        }
    }
    let defect = gram_error.sqrt() + rounding;
    if defect.sup() >= 1. {
        return Err(Error::Invalid("checker: polar gate".into()));
    }
    let polar = defect / (point(1.) + (point(1.) - defect).sqrt());
    let residual = reconstruction_error.sqrt() + rounding * point(scale);
    let bound = residual + point(scale) * polar * ((point(1.) + defect).sqrt() + point(1.));
    Ok(Some((upper(bound)?, upper(polar)?)))
}

pub fn inspect(a: &Matrix, e: &Eigensystem, cancel: &Cancellation) -> Result<(f64, f64)> {
    let p = a.n;
    if e.values.len() != p
        || e.vectors.n != p
        || e.vectors.data.len() != p * p
        || e.values.windows(2).any(|v| v[0] < v[1])
        || e.values
            .iter()
            .chain(&e.vectors.data)
            .any(|x| !x.is_finite())
    {
        return Err(Error::Invalid(
            "checker: malformed or unordered eigensystem".into(),
        ));
    }
    if p > 16
        && p <= 256
        && let Some((bound, correction)) = normal_bounds(a, e, cancel)?
    {
        if !e.backward.is_finite()
            || !e.polar.is_finite()
            || e.backward < bound
            || e.polar < correction
        {
            return Err(Error::Invalid(
                "checker: understated residual/polar bound".into(),
            ));
        }
        return Ok((bound, correction));
    }
    let mut orth = point(0.);
    let mut residual = point(0.);
    for row in 0..p {
        cancel.poll()?;
        for col in 0..p {
            let gram = (0..p).fold(point(0.), |sum, k| {
                sum + point(e.vectors.at(k, row)) * point(e.vectors.at(k, col))
            });
            orth += square(gram - point(f64::from(row == col)));
            let rebuilt = (0..p).fold(point(0.), |sum, k| {
                sum + point(e.values[k]) * point(e.vectors.at(row, k)) * point(e.vectors.at(col, k))
            });
            residual += square(rebuilt - point(a.at(row, col)));
        }
    }
    let tau = orth.sqrt();
    if tau.sup() >= 1. {
        return Err(Error::Invalid("checker: polar gate".into()));
    }
    let correction = tau / (point(1.) + (point(1.) - tau).sqrt());
    let scale = e.values.iter().fold(0f64, |m, v| m.max(v.abs()));
    let bound = (residual.sqrt()
        + point(scale) * correction * ((point(1.) + tau).sqrt() + point(1.)))
    .sup();
    // Different interval evaluation order is allowed; producer bounds must cover this evaluation too.
    if !e.backward.is_finite()
        || !e.polar.is_finite()
        || e.backward < bound
        || e.polar < correction.sup()
    {
        return Err(Error::Invalid(
            "checker: understated residual/polar bound".into(),
        ));
    }
    Ok((bound, correction.sup()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn psd_allocation_and_cancellation_errors_are_preserved() {
        let matrix = Matrix {
            n: 2,
            data: vec![1., 0., 0., 1.],
        };
        let proposal = Eigensystem {
            values: vec![1., 1.],
            vectors: matrix.clone(),
            backward: 2.,
            polar: 0.,
        };
        let cancel = Cancellation::default();
        let baseline = memory::Operation::new(resources::DEFAULT_BYTES)
            .unwrap()
            .stats()
            .live;
        let tight = memory::Operation::new(baseline).unwrap();
        assert!(matches!(
            tight.run(|| psd_checked(&matrix, &proposal, &cancel)),
            Err(Error::Resource(_))
        ));
        cancel.cancel();
        let ordinary = memory::Operation::new(resources::DEFAULT_BYTES).unwrap();
        assert!(matches!(
            ordinary.run(|| psd_checked(&matrix, &proposal, &cancel)),
            Err(Error::Cancelled)
        ));
    }
}
