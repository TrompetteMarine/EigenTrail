//! Standalone SPD reference-frame helpers for rolling lineage.
//!
//! Every frame describes an exact SPD target `A` through an exactly symmetric
//! binary64 midpoint `M` with `||A - M||_op <= matrix_error`, plus a verified
//! eigensystem of `M`.  `numerical_rho` charges the transform error and the
//! eigensystem residual once: `ln(lambda_min) - ln(lambda_min - total)`.
//! No report, graph, calibration or regional decision is made here.
use crate::numerics;
use crate::resources::memory::{self, HeapBytes};
use crate::resources::{Cancellation, zeros};
use crate::scalar::*;
use crate::types::{Eigensystem, Matrix};
use crate::{Error, Result};

pub struct ReferenceFrame {
    pub matrix: Matrix,
    pub eigensystem: Eigensystem,
    pub matrix_error: f64,
    pub numerical_rho: f64,
}

pub struct InverseProposal {
    pub matrix: Matrix,
    pub error: f64,
}

impl HeapBytes for ReferenceFrame {
    fn heap_bytes(&self) -> Result<usize> {
        self.matrix
            .heap_bytes()?
            .checked_add(self.eigensystem.heap_bytes()?)
            .ok_or_else(|| Error::Resource(String::new()))
    }
}

impl HeapBytes for InverseProposal {
    fn heap_bytes(&self) -> Result<usize> {
        self.matrix.heap_bytes()
    }
}

fn exhausted(what: &str) -> Error {
    match memory::text(what) {
        Ok(message) => Error::Exhausted(message),
        Err(error) => error,
    }
}

fn invalid(what: &str) -> Error {
    match memory::text(what) {
        Ok(message) => Error::Invalid(message),
        Err(error) => error,
    }
}

fn finite_bound(value: f64) -> Result<()> {
    if !value.is_finite() || value < 0. {
        return Err(invalid("finite nonnegative numerical bound required"));
    }
    Ok(())
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.
}

fn checked_matrix(matrix: &Matrix, cancel: &Cancellation) -> Result<()> {
    cancel.poll()?;
    if matrix.n > 256 {
        return Err(Error::Resource(String::new()));
    }
    numerics::validate_matrix(matrix)
}

fn squared_size(n: usize) -> Result<usize> {
    n.checked_mul(n)
        .ok_or_else(|| invalid("matrix size overflow"))
}

fn minimum_value(e: &Eigensystem) -> Result<f64> {
    if e.values.is_empty() || e.values.iter().any(|v| !v.is_finite()) {
        return Err(exhausted("nonfinite or empty spectrum"));
    }
    Ok(e.values.iter().copied().fold(f64::INFINITY, f64::min))
}

/// Charges `e.backward + matrix_error` once against the midpoint floor.
fn numerical_radius(eigensystem: &Eigensystem, matrix_error: f64) -> Result<f64> {
    finite_bound(matrix_error)?;
    finite_bound(eigensystem.backward)?;
    let lambda = minimum_value(eigensystem)?;
    let total = upper(point(eigensystem.backward) + point(matrix_error))?;
    let floor = point(lambda) - point(total);
    if !positive(floor.inf()) {
        return Err(exhausted("strict positive floor unresolved"));
    }
    if total == 0. {
        return Ok(0.);
    }
    upper(ln(point(lambda))? - ln(floor)?)
}

fn finish(matrix: Matrix, eigensystem: Eigensystem, matrix_error: f64) -> Result<ReferenceFrame> {
    let numerical_rho = numerical_radius(&eigensystem, matrix_error)?;
    Ok(ReferenceFrame {
        matrix,
        eigensystem,
        matrix_error,
        numerical_rho,
    })
}

/// Frame for an exact SPD target known only through `||A - midpoint||_op <= error`.
/// An unresolved floor is `Exhausted`; the midpoint alone never proves the target non-SPD.
pub fn frame_from_enclosure(
    midpoint: Matrix,
    error: f64,
    cancel: &Cancellation,
) -> Result<ReferenceFrame> {
    // The owned midpoint was charged by the caller; keep that ownership in its scope.
    finite_bound(error)?;
    checked_matrix(&midpoint, cancel)?;
    let eigensystem = numerics::eigensystem(&midpoint, cancel)?;
    finish(midpoint, eigensystem, error)
}

/// Frame of the binary64 root `S` itself (`matrix_error = 0`).
pub fn root_frame(matrix: &Matrix, cancel: &Cancellation) -> Result<ReferenceFrame> {
    memory::scope(|| {
        checked_matrix(matrix, cancel)?;
        for i in 0..matrix.n {
            if !positive(matrix.at(i, i)) {
                return Err(invalid("root covariance has a nonpositive diagonal entry"));
            }
        }
        let eigensystem = numerics::eigensystem(matrix, cancel)?;
        finite_bound(eigensystem.backward)?;
        let lambda = minimum_value(&eigensystem)?;
        // Weyl: lambda_min(S) lies in [lambda - backward, lambda + backward].
        let lower = point(lambda) - point(eigensystem.backward);
        let higher = point(lambda) + point(eigensystem.backward);
        if !positive(lower.inf()) {
            if higher.sup() <= 0. {
                return Err(invalid(
                    "root covariance is certified not strictly positive definite",
                ));
            }
            return Err(exhausted("root strict positive floor unresolved"));
        }
        let stored = Matrix {
            n: matrix.n,
            data: memory::copy(&matrix.data)?,
        };
        finish(stored, eigensystem, 0.)
    })
}

/// Normalises an entrywise enclosure `Y_ij in entry(i, j)` of an exact symmetric target
/// to `N(Y)_ij = Y_ij / sqrt(Y_ii) / sqrt(Y_jj)` with exact unit diagonal.
/// Returns an exactly symmetric midpoint built from `raw` and a Frobenius bound on
/// the distance of every admissible `N(Y)` from it.
fn normalised_enclosure(
    n: usize,
    entry: impl Fn(usize, usize) -> Result<Interval>,
    raw: impl Fn(usize, usize) -> f64,
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    let size = squared_size(n)?;
    let mut roots: Vec<Interval> = memory::capacity(n)?;
    let mut scales = zeros(n)?;
    for (i, stored_scale) in scales.iter_mut().enumerate() {
        let diagonal = entry(i, i)?;
        if !positive(diagonal.inf()) || !diagonal.sup().is_finite() {
            return Err(exhausted("normalisation diagonal floor unresolved"));
        }
        memory::push(&mut roots, diagonal.sqrt())?;
        let value = raw(i, i);
        let scale = value.sqrt();
        if !positive(value) || !positive(scale) {
            return Err(exhausted("normalisation proposal diagonal nonpositive"));
        }
        *stored_scale = scale;
    }
    let mut data = zeros(size)?;
    let mut sum = point(0.);
    for j in 0..n {
        cancel.poll()?;
        data[j + j * n] = 1.;
        for i in 0..j {
            // Sequential division avoids forming sqrt(Y_ii * Y_jj).
            let middle = raw(i, j) / scales[i] / scales[j];
            if !middle.is_finite() {
                return Err(exhausted("nonfinite normalised proposal"));
            }
            data[i + j * n] = middle;
            data[j + i * n] = middle;
            let exact = entry(i, j)? / roots[i] / roots[j];
            let difference = exact - point(middle);
            if !difference.inf().is_finite() || !difference.sup().is_finite() {
                return Err(exhausted("nonfinite normalised entry interval"));
            }
            sum += point(2.) * square(difference);
        }
    }
    let error = upper(sum.sqrt())?;
    Ok((Matrix { n, data }, error))
}

/// Frame of the exact correlation `C = N(S)` of the binary64 root.
pub fn correlation_frame(matrix: &Matrix, cancel: &Cancellation) -> Result<ReferenceFrame> {
    memory::scope(|| {
        checked_matrix(matrix, cancel)?;
        for i in 0..matrix.n {
            if !positive(matrix.at(i, i)) {
                return Err(invalid("correlation requires strictly positive variances"));
            }
        }
        let (midpoint, error) = normalised_enclosure(
            matrix.n,
            |i, j| Ok(point(matrix.at(i, j))),
            |i, j| matrix.at(i, j),
            cancel,
        )?;
        frame_from_enclosure(midpoint, error, cancel)
    })
}

fn same_root(matrix: &Matrix, root: &ReferenceFrame, cancel: &Cancellation) -> Result<()> {
    let n = matrix.n;
    let size = squared_size(n)?;
    let e = &root.eigensystem;
    if root.matrix.n != n
        || root.matrix.data.len() != size
        || root.matrix.data != matrix.data
        || root.matrix_error != 0.
        || e.values.len() != n
        || e.vectors.n != n
        || e.vectors.data.len() != size
    {
        return Err(invalid(
            "inverse proposal requires the root frame of this exact matrix",
        ));
    }
    finite_bound(e.backward)?;
    finite_bound(e.polar)?;
    finite_bound(root.numerical_rho)?;
    let (backward, polar) = numerics::verify_eigensystem(matrix, &e.values, &e.vectors, cancel)?;
    if e.backward < backward || e.polar < polar || root.numerical_rho < numerical_radius(e, 0.)? {
        return Err(invalid(
            "root frame understates a reconstructed numerical bound",
        ));
    }
    Ok(())
}

/// Symmetric `X ~ S^{-1}` with `||S^{-1} - X||_op <= error`, certified by `R = I - S X`.
pub fn inverse_proposal(
    matrix: &Matrix,
    root: &ReferenceFrame,
    cancel: &Cancellation,
) -> Result<InverseProposal> {
    memory::scope(|| {
        checked_matrix(matrix, cancel)?;
        same_root(matrix, root, cancel)?;
        let n = matrix.n;
        let e = &root.eigensystem;
        let lambda = minimum_value(e)?;
        let floor = (point(lambda) - point(e.backward)).inf();
        if !(lambda > 0. && floor > 0.) {
            return Err(exhausted("root strict positive floor unresolved"));
        }
        let mut inverse_values = zeros(n)?;
        for (k, &value) in e.values.iter().enumerate() {
            let reciprocal = 1. / value;
            if !(value > 0. && reciprocal.is_finite()) {
                return Err(exhausted("nonfinite reciprocal eigenvalue"));
            }
            inverse_values[k] = reciprocal;
        }
        let v = &e.vectors;
        let mut data = zeros(squared_size(n)?)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..=j {
                let mut value = 0.;
                for (k, &w) in inverse_values.iter().enumerate() {
                    value += v.at(i, k) * w * v.at(j, k);
                }
                if !value.is_finite() {
                    return Err(exhausted("nonfinite inverse proposal"));
                }
                data[i + j * n] = value;
                data[j + i * n] = value;
            }
        }
        let proposal = Matrix { n, data };
        // Residual against the original S charges eigenvector, polar and rounding errors.
        let mut sum = point(0.);
        for j in 0..n {
            cancel.poll()?;
            for i in 0..n {
                let mut product = point(0.);
                for k in 0..n {
                    product += point(matrix.at(i, k)) * point(proposal.at(k, j));
                }
                sum += square(point(f64::from(i == j)) - product);
            }
        }
        let error = upper(sum.sqrt() / point(floor))?;
        Ok(InverseProposal {
            matrix: proposal,
            error,
        })
    })
}

/// Frame of the exact normalised precision `Q = N(S^{-1})`.  The caller maps
/// `P = 2I - Q` separately; no spectrum is clamped here.
pub fn normalised_precision_frame(
    matrix: &Matrix,
    root: &ReferenceFrame,
    cancel: &Cancellation,
) -> Result<ReferenceFrame> {
    let (_, frame) = normalised_precision_with_inverse(matrix, root, cancel)?;
    Ok(frame)
}

/// Retain the exact inverse proposal used by the normalisation certificate.
pub fn normalised_precision_with_inverse(
    matrix: &Matrix,
    root: &ReferenceFrame,
    cancel: &Cancellation,
) -> Result<(InverseProposal, ReferenceFrame)> {
    memory::scope(|| {
        let proposal = inverse_proposal(matrix, root, cancel)?;
        let x = &proposal.matrix;
        let band = interval(-proposal.error, proposal.error)
            .map_err(|_| exhausted("inverse error band nonfinite"))?;
        let (midpoint, error) = normalised_enclosure(
            x.n,
            |i, j| Ok(point(x.at(i, j)) + band),
            |i, j| x.at(i, j),
            cancel,
        )?;
        let frame = frame_from_enclosure(midpoint, error, cancel)?;
        Ok((proposal, frame))
    })
}
