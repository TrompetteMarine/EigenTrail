use crate::{Error, Result, resources::*, scalar::*, types::*};
use faer::diag::DiagMut;
use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::{MatMut, MatRef, Par};

pub fn validate_matrix(a: &Matrix) -> Result<()> {
    if a.n == 0
        || a.data.len()
            != a.n
                .checked_mul(a.n)
                .ok_or_else(|| Error::Invalid("matrix size overflow".into()))?
        || a.data.iter().any(|v| !v.is_finite())
    {
        return Err(Error::Invalid("finite square matrix required".into()));
    }
    for j in 0..a.n {
        for i in 0..j {
            if a.at(i, j) != a.at(j, i) {
                return Err(Error::Invalid(
                    "matrix is not exactly symmetric; declare and charge any correction".into(),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn frame_polar(a: &Matrix, cancel: &Cancellation) -> Result<f64> {
    let mut gram = point(0.);
    for i in 0..a.n {
        cancel.poll()?;
        for j in 0..a.n {
            let dot = (0..a.n).fold(point(0.), |s, k| s + point(a.at(k, i)) * point(a.at(k, j)));
            gram += square(dot - point(f64::from(i == j)));
        }
    }
    let tau = gram.sqrt();
    if tau.sup() >= 1. {
        return Err(Error::Exhausted("transport frame polar gate".into()));
    }
    upper(tau / (point(1.) + (point(1.) - tau).sqrt()))
}

fn normal_residuals(
    a: &Matrix,
    values: &[f64],
    v: &Matrix,
    cancel: &Cancellation,
) -> Result<Option<(Interval, Interval)>> {
    let n = a.n;
    let norm_squared = v.data.iter().fold(point(0.), |s, &x| s + square(point(x)));
    let g = point((2 * n + 4) as f64) * point(f64::EPSILON);
    let allowance = g / (point(1.) - g) * norm_squared;
    let scale = values.iter().fold(0f64, |s, x| s.max(x.abs()));
    let mut orth = point(0.);
    let mut residual = point(0.);
    for i in 0..n {
        cancel.poll()?;
        for j in 0..n {
            let Some(gram) = normal_product_sum((0..n).map(|k| (v.at(k, i), v.at(k, j), 1.)))
            else {
                return Ok(None);
            };
            let Some(rebuilt) =
                normal_product_sum((0..n).map(|k| (v.at(i, k), values[k], v.at(j, k))))
            else {
                return Ok(None);
            };
            orth += square(point(gram) - point(f64::from(i == j)));
            residual += square(point(a.at(i, j)) - point(rebuilt));
        }
    }
    Ok(Some((
        orth.sqrt() + allowance,
        residual.sqrt() + allowance * point(scale),
    )))
}

pub fn verify_eigensystem(
    a: &Matrix,
    values: &[f64],
    v: &Matrix,
    cancel: &Cancellation,
) -> Result<(f64, f64)> {
    let n = a.n;
    if values.len() != n
        || v.n != n
        || v.data.len() != n * n
        || values.iter().chain(v.data.iter()).any(|x| !x.is_finite())
        || values.windows(2).any(|s| s[0] < s[1])
    {
        return Err(Error::Invalid("ordered full eigensystem required".into()));
    }
    if n > 16
        && n <= 256
        && let Some((tau, residual)) = normal_residuals(a, values, v, cancel)?
    {
        if tau.sup() >= 1. {
            return Err(Error::Exhausted(
                "orthogonality/polar gate unresolved".into(),
            ));
        }
        let dv = tau / (point(1.) + (point(1.) - tau).sqrt());
        let scale = values.iter().fold(0f64, |s, x| s.max(x.abs()));
        let b = residual + point(scale) * dv * ((point(1.) + tau).sqrt() + point(1.));
        return Ok((upper(point(4.) * b)?, upper(point(2.) * dv)?));
    }
    let mut ortho = point(0.);
    let mut resid = point(0.);
    for i in 0..n {
        cancel.poll()?;
        for j in 0..n {
            let mut dot = point(0.);
            let mut reconstruction = point(0.);
            for (k, &value) in values.iter().enumerate() {
                dot += point(v.at(k, i)) * point(v.at(k, j));
                reconstruction += point(v.at(i, k)) * point(value) * point(v.at(j, k));
            }
            ortho += square(dot - point(if i == j { 1. } else { 0. }));
            resid += square(point(a.at(i, j)) - reconstruction);
        }
    }
    let tau = ortho.sqrt();
    if tau.sup() >= 1. {
        return Err(Error::Exhausted(
            "orthogonality/polar gate unresolved".into(),
        ));
    }
    let dv = tau / (point(1.) + (point(1.) - tau).sqrt());
    let d = values.iter().map(|x| x.abs()).fold(0., f64::max);
    let b = resid.sqrt() + point(d) * dv * ((point(1.) + tau).sqrt() + point(1.));
    // A second independently associated residual calculation must fit this guard.
    Ok((upper(point(2.) * b)?, upper(dv)?))
}

pub fn eigensystem(a: &Matrix, cancel: &Cancellation) -> Result<Eigensystem> {
    crate::resources::memory::scope(|| {
        validate_matrix(a)?;
        if a.n > 256 {
            return Err(Error::Resource("eigensystem dimension ceiling 256".into()));
        }
        cancel.poll()?;
        let n = a.n;
        let mut values = zeros(n)?;
        let mut data = zeros(n * n)?;
        if n == 1 {
            // A scalar is already its exact eigensystem; no native scratch is needed.
            values[0] = a.data[0];
            data[0] = 1.;
            return Ok(Eigensystem {
                values,
                vectors: Matrix { n, data },
                backward: 0.,
                polar: 0.,
            });
        }
        let mut parameters = faer::Spec::<faer::linalg::evd::SelfAdjointEvdParams, f64>::default();
        parameters.recursion_threshold = 257;
        let requirement = if n <= 16 {
            faer::linalg::evd::self_adjoint_evd_scratch::<f64>(
                n,
                faer::linalg::evd::ComputeEigenvectors::Yes,
                Par::Seq,
                Default::default(),
            )
        } else {
            // Single-column Householders and QR avoid GEMM's untracked packing/cache allocations.
            StackReq::any_of(&[
                faer::linalg::evd::tridiag::tridiag_in_place_scratch::<f64>(
                    n,
                    Par::Seq,
                    parameters.tridiag.into(),
                ),
                faer::linalg::temp_mat_scratch::<f64>(n, 1).array(2),
                faer::linalg::householder::apply_block_householder_sequence_on_the_left_in_place_scratch::<f64>(n - 1, 1, n),
            ])
        };
        if requirement.unaligned_bytes_required() > n * n * 8 * 64 {
            return Err(Error::Resource(
                "eigensolver scratch exceeds its reserved envelope".into(),
            ));
        }
        let _scratch_charge = memory::external(requirement.unaligned_bytes_required())?;
        let mut scratch = MemBuffer::try_new(requirement)
            .map_err(|_| Error::Resource("eigensolver scratch allocation refused".into()))?;
        if n <= 16 {
            faer::linalg::evd::self_adjoint_evd(
                MatRef::from_column_major_slice(&a.data, n, n),
                DiagMut::from_slice_mut(&mut values),
                Some(MatMut::from_column_major_slice_mut(&mut data, n, n)),
                Par::Seq,
                MemStack::new(&mut scratch),
                Default::default(),
            )
            .map_err(|_| Error::Exhausted("eigenvalue proposal did not converge".into()))?;
        } else {
            let mut tridiagonal = memory::copy(&a.data)?;
            let mut factors = zeros(n - 1)?;
            faer::linalg::evd::tridiag::tridiag_in_place(
                MatMut::from_column_major_slice_mut(&mut tridiagonal, n, n),
                MatMut::from_column_major_slice_mut(&mut factors, 1, n - 1),
                Par::Seq,
                MemStack::new(&mut scratch),
                parameters.tridiag.into(),
            );
            cancel.poll()?;
            let tridiagonal = MatRef::from_column_major_slice(&tridiagonal, n, n);
            faer::linalg::evd::tridiagonal_self_adjoint_evd(
                tridiagonal.diagonal(),
                tridiagonal.submatrix(1, 0, n - 1, n - 1).diagonal(),
                DiagMut::from_slice_mut(&mut values),
                Some(MatMut::from_column_major_slice_mut(&mut data, n, n)),
                Par::Seq,
                MemStack::new(&mut scratch),
                parameters,
            )
            .map_err(|_| Error::Exhausted("eigenvalue proposal did not converge".into()))?;
            cancel.poll()?;
            faer::linalg::householder::apply_block_householder_sequence_on_the_left_in_place_with_conj(
                tridiagonal.submatrix(1, 0, n - 1, n - 1),
                MatRef::from_column_major_slice(&factors, 1, n - 1),
                faer::Conj::No,
                MatMut::from_column_major_slice_mut(&mut data, n, n).subrows_mut(1, n - 1),
                Par::Seq,
                MemStack::new(&mut scratch),
            );
        }
        cancel.poll()?;
        values.reverse();
        for j in 0..n / 2 {
            for i in 0..n {
                data.swap(i + j * n, i + (n - 1 - j) * n);
            }
        }
        let vectors = Matrix { n, data };
        let (backward, polar) = verify_eigensystem(a, &values, &vectors, cancel)?;
        Ok(Eigensystem {
            values,
            vectors,
            backward,
            polar,
        })
    })
}

/// Approximate display matrix; its roundoff and polar correction are not a certificate.
pub fn projector(e: &Eigensystem, first: usize, end: usize) -> Matrix {
    try_projector(e, first, end).expect("fallible projector requires try_projector")
}
pub fn try_projector(e: &Eigensystem, first: usize, end: usize) -> Result<Matrix> {
    memory::scope(|| {
        let n = e.vectors.n;
        let mut data = zeros(n * n)?;
        for j in 0..n {
            for i in 0..n {
                for k in first..end {
                    data[i + j * n] += e.vectors.at(i, k) * e.vectors.at(j, k);
                }
            }
        }
        Ok(Matrix { n, data })
    })
}
pub fn distance_bounds(
    a: &Eigensystem,
    b: &Eigensystem,
    first: usize,
    end: usize,
) -> Result<(f64, f64)> {
    let n = a.vectors.n;
    let mut sum = point(0.);
    for j in 0..n {
        for i in 0..n {
            let mut x = point(0.);
            for k in first..end {
                x = x + point(a.vectors.at(i, k)) * point(a.vectors.at(j, k))
                    - point(b.vectors.at(i, k)) * point(b.vectors.at(j, k));
            }
            sum += square(x);
        }
    }
    let correction = (point(2.) + point(a.polar)) * point(a.polar)
        + (point(2.) + point(b.polar)) * point(b.polar);
    // ||X||_F/sqrt(n) <= ||X||_op <= ||X||_F; polar corrections by triangle inequality.
    Ok((
        (sum.sqrt() / point(n as f64).sqrt() - correction)
            .inf()
            .max(0.),
        (sum.sqrt() + correction).sup().min(1.),
    ))
}

pub fn difference_norm_upper(a: &Matrix, b: &Matrix, cancel: &Cancellation) -> Result<f64> {
    crate::resources::memory::scope(|| {
        if a.n != b.n || a.data.len() != b.data.len() {
            return Err(Error::Invalid("matrix difference dimensions".into()));
        }
        if a.n == 1 {
            return upper((point(a.data[0]) - point(b.data[0])).abs());
        }
        if a.n == 2 {
            let x = point(a.data[0]) - point(b.data[0]);
            let z = point(a.data[3]) - point(b.data[3]);
            let y = point(a.data[1]) - point(b.data[1]);
            let disc = (square(x - z) + point(4.) * square(y)).sqrt();
            return upper(((x + z).abs() + disc) / point(2.));
        }
        let mut data = memory::capacity(a.data.len())?;
        let mut round = point(0.);
        for (&x, &y) in a.data.iter().zip(&b.data) {
            let d = x - y;
            if !d.is_finite() {
                return Err(Error::Exhausted("difference overflow".into()));
            }
            round += square(point(x) - point(y) - point(d));
            data.push(d);
        }
        let e = eigensystem(&Matrix { n: a.n, data }, cancel)?;
        upper(
            point(e.values.iter().fold(0f64, |s, x| s.max(x.abs())))
                + point(e.backward)
                + round.sqrt(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_large_proposals_enclose_the_analytic_tridiagonal_spectrum() {
        for n in [17, 64] {
            let mut data = vec![0.; n * n];
            for i in 0..n {
                data[i + i * n] = 2.;
                if i + 1 < n {
                    data[i + 1 + i * n] = 0.25;
                    data[i + (i + 1) * n] = 0.25;
                }
            }
            let a = Matrix { n, data };
            let cancel = Cancellation::default();
            let operation = memory::Operation::new(DEFAULT_BYTES).unwrap();
            let result = operation.run(|| eigensystem(&a, &cancel)).unwrap();
            for (index, value) in result.values.iter().enumerate() {
                let exact =
                    2. + 0.5 * (((index + 1) as f64 * std::f64::consts::PI) / (n + 1) as f64).cos();
                assert!((value - exact).abs() <= result.backward + 1e-12);
            }
            assert!(operation.stats().peak <= DEFAULT_BYTES);
        }
    }

    #[test]
    fn serial_large_proposal_preserves_a_dense_repeated_eigenspace() {
        let n = 17;
        let mut data = vec![0.125; n * n];
        for i in 0..n {
            data[i + i * n] += 1.;
        }
        let a = Matrix { n, data };
        let operation = memory::Operation::new(DEFAULT_BYTES).unwrap();
        let result = operation
            .run(|| eigensystem(&a, &Cancellation::default()))
            .unwrap();
        assert!((result.values[0] - (1. + n as f64 / 8.)).abs() <= result.backward + 1e-12);
        assert!(
            result.values[1..]
                .iter()
                .all(|value| { (value - 1.).abs() <= result.backward + 1e-12 })
        );
    }
}
