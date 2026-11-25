//! Independently reconstruct reference angles from polar frames and sparse norm proofs.
//! The caller binds frames, columns, witness stream/ordinal and reported edge type.
//! Centre residuals, target regions and node angular radii are checked by the caller;
//! this module reconstructs only the polar-frame point geometry.
use eigentrail_core::{
    Eigensystem, Error, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    rolling_lineage::report::{NormKind, SparseNormWitness},
    scalar::{self, Interval, interval, point, square},
};

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}

fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.
}

/// Cache this independent O(N^3) polar reconstruction once per supplied frame.
#[derive(Debug)]
pub struct CheckerFrame<'a> {
    system: &'a Eigensystem,
    polar: f64,
}

impl HeapBytes for CheckerFrame<'_> {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}

impl<'a> CheckerFrame<'a> {
    pub fn new(system: &'a Eigensystem, cancel: &Cancellation) -> Result<Self> {
        cancel.poll()?;
        let n = system.vectors.n;
        if n == 0 || n > 256 || system.values.len() != n || system.vectors.data.len() != n * n {
            return Err(invalid("angle checker: malformed frame dimensions"));
        }
        if !nonnegative(system.backward)
            || !nonnegative(system.polar)
            || system.values.iter().any(|value| !value.is_finite())
            || system.values.windows(2).any(|pair| pair[0] < pair[1])
            || system.vectors.data.iter().any(|value| !value.is_finite())
        {
            return Err(invalid("angle checker: invalid frame values or guards"));
        }
        let mut defect_squared = point(0.);
        for first in 0..n {
            cancel.poll()?;
            for second in 0..n {
                let gram = (0..n).fold(point(0.), |sum, row| {
                    sum + point(system.vectors.at(row, first))
                        * point(system.vectors.at(row, second))
                });
                defect_squared += square(gram - point(f64::from(first == second)));
            }
        }
        let defect = defect_squared.sqrt();
        if defect.sup() >= 1. {
            return Err(invalid("angle checker: polar gate unresolved"));
        }
        let polar = scalar::upper(defect / (point(1.) + (point(1.) - defect).sqrt()))?;
        Ok(Self { system, polar })
    }

    pub fn dimension(&self) -> usize {
        self.system.vectors.n
    }

    pub fn polar_bound(&self) -> f64 {
        self.polar
    }
}

fn reject_witness(witness: Option<&SparseNormWitness>) -> Result<()> {
    if witness.is_some() {
        Err(invalid("angle checker: unused norm witness"))
    } else {
        Ok(())
    }
}

fn unit(value: Interval) -> Result<Interval> {
    let enclosed = value.intersection(interval(0., 1.)?);
    if enclosed.is_empty() {
        Err(invalid("angle checker: inconsistent unit enclosure"))
    } else {
        Ok(enclosed)
    }
}

fn degrees(value: Interval) -> Result<[f64; 2]> {
    let enclosed = (value * (point(180.) / Interval::PI)).intersection(interval(0., 90.)?);
    if enclosed.is_empty() || !enclosed.inf().is_finite() || !enclosed.sup().is_finite() {
        return Err(invalid("angle checker: invalid angular enclosure"));
    }
    Ok([enclosed.inf() + 0., enclosed.sup() + 0.])
}

fn radius(value: f64) -> Result<Interval> {
    interval(-value, value)
}

fn projector_error(frame: &CheckerFrame<'_>) -> Interval {
    (point(2.) + point(frame.polar)) * point(frame.polar)
}

fn sine_angle(value: Interval, correction: Interval) -> Result<[f64; 2]> {
    let uncertainty = radius(scalar::upper(correction)?)?;
    degrees(scalar::asin(unit(value + uncertainty)?)?)
}

fn projector(
    frame: &CheckerFrame<'_>,
    columns: [usize; 2],
    cancel: &Cancellation,
) -> Result<Vec<Interval>> {
    let n = frame.dimension();
    let mut entries = memory::capacity(n * n)?;
    entries.resize(n * n, point(0.));
    for col in 0..n {
        cancel.poll()?;
        for row in 0..=col {
            let value = (columns[0]..columns[1]).fold(point(0.), |sum, index| {
                sum + point(frame.system.vectors.at(row, index))
                    * point(frame.system.vectors.at(col, index))
            });
            entries[row + col * n] = value;
            entries[col + row * n] = value;
        }
    }
    Ok(entries)
}

fn midpoint(value: Interval) -> Result<f64> {
    let (lower, upper) = (value.inf(), value.sup());
    if value.is_empty() || !lower.is_finite() || !upper.is_finite() {
        return Err(invalid("angle checker: nonfinite norm source"));
    }
    let proposed = 0.5 * lower + 0.5 * upper;
    Ok(
        if proposed.is_finite() && lower <= proposed && proposed <= upper {
            proposed + 0.
        } else {
            lower + 0.
        },
    )
}

/// Check the canonical source midpoint and its complete entry-rounding allowance.
fn norm_proof<'a>(
    witness: Option<&'a SparseNormWitness>,
    kind: NormKind,
    n: usize,
    source: &[Interval],
    cancel: &Cancellation,
) -> Result<&'a SparseNormWitness> {
    let witness = witness.ok_or_else(|| invalid("angle checker: missing norm witness"))?;
    if witness.kind != kind
        || witness.matrix.n != n
        || witness.matrix.data.len() != n * n
        || !nonnegative(witness.entry_error)
    {
        return Err(invalid("angle checker: wrong norm source, shape or error"));
    }
    let mut error_squared = point(0.);
    for col in 0..n {
        cancel.poll()?;
        for row in 0..=col {
            let expected = midpoint(source[row + col * n])?;
            if witness.matrix.at(row, col).to_bits() != expected.to_bits()
                || witness.matrix.at(col, row).to_bits() != expected.to_bits()
            {
                return Err(invalid(
                    "angle checker: norm midpoint differs from its source",
                ));
            }
            error_squared += square(source[row + col * n] - point(expected));
            if row != col {
                error_squared += square(source[col + row * n] - point(expected));
            }
        }
    }
    let expected_error = scalar::upper(error_squared.sqrt())?;
    if witness.entry_error.to_bits() != expected_error.to_bits() {
        return Err(invalid(
            "angle checker: norm entry error differs from its source",
        ));
    }
    crate::eigensystem::inspect(&witness.matrix, &witness.eigensystem, cancel)?;
    Ok(witness)
}

fn line_angle(
    a: &CheckerFrame<'_>,
    ac: [usize; 2],
    b: &CheckerFrame<'_>,
    bc: [usize; 2],
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    cancel.poll()?;
    let (mut overlap, mut difference, mut sum) = (point(0.), point(0.), point(0.));
    for row in 0..a.dimension() {
        let (x, y) = (
            point(a.system.vectors.at(row, ac[0])),
            point(b.system.vectors.at(row, bc[0])),
        );
        overlap += x * y;
        difference += square(x - y);
        sum += square(x + y);
    }
    let uncertainty =
        scalar::upper(point(a.polar) + point(b.polar) + point(a.polar) * point(b.polar))?;
    let cosine = degrees(scalar::acos(unit((overlap + radius(uncertainty)?).abs())?)?)?;
    // The polar-column chord is well conditioned when the acute angle is near zero.
    let (difference, sum) = (difference.sqrt(), sum.sqrt());
    let chord = interval(
        difference.inf().min(sum.inf()),
        difference.sup().min(sum.sup()),
    )?;
    let chord_error = scalar::upper(point(a.polar) + point(b.polar))?;
    let chord =
        degrees(point(2.) * scalar::asin(unit((chord + radius(chord_error)?) / point(2.))?)?)?;
    let result = [cosine[0].max(chord[0]), cosine[1].min(chord[1])];
    if result[0] > result[1] {
        return Err(invalid(
            "angle checker: inconsistent cosine and chord certificates",
        ));
    }
    Ok(result)
}

fn equal_angle(
    a: &CheckerFrame<'_>,
    ac: [usize; 2],
    b: &CheckerFrame<'_>,
    bc: [usize; 2],
    witness: Option<&SparseNormWitness>,
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    let n = a.dimension();
    let mut difference = projector(a, ac, cancel)?;
    let other = projector(b, bc, cancel)?;
    for col in 0..n {
        cancel.poll()?;
        for row in 0..n {
            let index = row + col * n;
            difference[index] -= other[index];
        }
    }
    drop(other);
    let witness = norm_proof(witness, NormKind::Difference, n, &difference, cancel)?;
    let e = &witness.eigensystem;
    let uncertainty = scalar::upper(point(e.backward) + point(witness.entry_error))?;
    let centre = e.values[0].abs().max(e.values[n - 1].abs());
    sine_angle(
        point(centre) + radius(uncertainty)?,
        projector_error(a) + projector_error(b),
    )
}

fn line_containment(
    small: &CheckerFrame<'_>,
    sc: [usize; 2],
    large: &CheckerFrame<'_>,
    lc: [usize; 2],
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    let n = small.dimension();
    let mut overlaps = memory::capacity(lc[1] - lc[0])?;
    for column in lc[0]..lc[1] {
        cancel.poll()?;
        let value = (0..n).fold(point(0.), |sum, row| {
            sum + point(large.system.vectors.at(row, column))
                * point(small.system.vectors.at(row, sc[0]))
        });
        memory::push(&mut overlaps, value)?;
    }
    let (mut source_norm, mut residual_norm) = (point(0.), point(0.));
    for row in 0..n {
        cancel.poll()?;
        let original = point(small.system.vectors.at(row, sc[0]));
        let projected = overlaps
            .iter()
            .enumerate()
            .fold(point(0.), |sum, (index, coefficient)| {
                sum + point(large.system.vectors.at(row, lc[0] + index)) * *coefficient
            });
        source_norm += square(original);
        residual_norm += square(original - projected);
    }
    let (small_error, large_error) = (projector_error(small), projector_error(large));
    sine_angle(
        source_norm.sqrt() * residual_norm.sqrt(),
        small_error + large_error + small_error * large_error,
    )
}

fn block_containment(
    small: &CheckerFrame<'_>,
    sc: [usize; 2],
    large: &CheckerFrame<'_>,
    lc: [usize; 2],
    witness: Option<&SparseNormWitness>,
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    let n = small.dimension();
    let ps = projector(small, sc, cancel)?;
    let pl = projector(large, lc, cancel)?;
    let mut residual = memory::capacity(n * n)?;
    residual.resize(n * n, point(0.));
    for col in 0..n {
        cancel.poll()?;
        for row in 0..n {
            let mut value = ps[row + col * n];
            for inner in 0..n {
                value -= pl[row + inner * n] * ps[inner + col * n];
            }
            residual[row + col * n] = value;
        }
    }
    drop(ps);
    drop(pl);
    let mut gram = memory::capacity(n * n)?;
    gram.resize(n * n, point(0.));
    for col in 0..n {
        cancel.poll()?;
        for row in 0..=col {
            let value = (0..n).fold(point(0.), |sum, inner| {
                sum + residual[inner + row * n] * residual[inner + col * n]
            });
            gram[row + col * n] = value;
            gram[col + row * n] = value;
        }
    }
    drop(residual);
    let witness = norm_proof(witness, NormKind::Gram, n, &gram, cancel)?;
    let e = &witness.eigensystem;
    let uncertainty = scalar::upper(point(e.backward) + point(witness.entry_error))?;
    let eigenvalue = point(e.values[0]) + radius(uncertainty)?;
    if eigenvalue.is_empty() || !eigenvalue.sup().is_finite() || eigenvalue.sup() < 0. {
        return Err(invalid(
            "angle checker: norm Gram contradicts positive semidefiniteness",
        ));
    }
    let norm = interval(eigenvalue.inf().max(0.), eigenvalue.sup())?.sqrt();
    let (small_error, large_error) = (projector_error(small), projector_error(large));
    sine_angle(norm, small_error + large_error + small_error * large_error)
}

/// Reconstruct a point-reference angle. The caller binds sparse stream/ordinal IDs.
pub fn check_angle_frames(
    a: &CheckerFrame<'_>,
    ac: [usize; 2],
    b: &CheckerFrame<'_>,
    bc: [usize; 2],
    witness: Option<&SparseNormWitness>,
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    cancel.poll()?;
    let n = a.dimension();
    let valid = |columns: [usize; 2]| columns[0] < columns[1] && columns[1] <= n;
    if b.dimension() != n || !valid(ac) || !valid(bc) {
        return Err(invalid("angle checker: invalid frame or column domain"));
    }
    let (aw, bw) = (ac[1] - ac[0], bc[1] - bc[0]);
    if aw == n || bw == n || std::ptr::eq(a.system, b.system) && ac == bc {
        reject_witness(witness)?;
        return Ok([0., 0.]);
    }
    if aw == 1 && bw == 1 {
        reject_witness(witness)?;
        return line_angle(a, ac, b, bc, cancel);
    }
    if aw == bw {
        return memory::scope(|| equal_angle(a, ac, b, bc, witness, cancel));
    }
    let (small, sc, large, lc) = if aw < bw {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    };
    if sc[1] - sc[0] == 1 {
        reject_witness(witness)?;
        memory::scope(|| line_containment(small, sc, large, lc, cancel))
    } else {
        memory::scope(|| block_containment(small, sc, large, lc, witness, cancel))
    }
}

/// Convenience route; large candidate streams should reuse cached CheckerFrames.
pub fn check_angle(
    a: &Eigensystem,
    ac: [usize; 2],
    b: &Eigensystem,
    bc: [usize; 2],
    witness: Option<&SparseNormWitness>,
    cancel: &Cancellation,
) -> Result<[f64; 2]> {
    let source = CheckerFrame::new(a, cancel)?;
    let target = CheckerFrame::new(b, cancel)?;
    check_angle_frames(&source, ac, &target, bc, witness, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eigentrail_core::{Matrix, rolling_lineage::report::CandidateStream};

    fn identity(n: usize) -> Eigensystem {
        Eigensystem {
            values: (1..=n).rev().map(|value| value as f64).collect(),
            vectors: Matrix {
                n,
                data: (0..n * n)
                    .map(|index| f64::from(index / n == index % n))
                    .collect(),
            },
            backward: 0.,
            polar: 0.,
        }
    }

    fn proof(diagonal: &[f64], kind: NormKind) -> SparseNormWitness {
        let mut eigensystem = identity(diagonal.len());
        eigensystem.values = diagonal.to_vec();
        let mut matrix = Matrix {
            n: diagonal.len(),
            data: vec![0.; diagonal.len() * diagonal.len()],
        };
        for (index, &value) in diagonal.iter().enumerate() {
            matrix.data[index * (diagonal.len() + 1)] = value;
        }
        SparseNormWitness {
            stream: CandidateStream::Graph,
            ordinal: 0,
            kind,
            matrix,
            entry_error: 0.,
            eigensystem,
        }
    }

    #[test]
    fn difference_proof_is_bound_to_the_complete_raw_source() {
        let cancel = Cancellation::default();
        let e = identity(3);
        let frame = CheckerFrame::new(&e, &cancel).unwrap();
        let valid = proof(&[1., 0., -1.], NormKind::Difference);
        let bounds =
            check_angle_frames(&frame, [0, 2], &frame, [1, 3], Some(&valid), &cancel).unwrap();
        assert!(bounds[0] > 89.999 && bounds[1] == 90.);
        assert!(check_angle_frames(&frame, [0, 2], &frame, [1, 3], None, &cancel).is_err());
        let mut wrong = valid.clone();
        wrong.matrix.data[0] = 0.;
        assert!(check_angle_frames(&frame, [0, 2], &frame, [1, 3], Some(&wrong), &cancel).is_err());
        wrong = valid.clone();
        wrong.kind = NormKind::Gram;
        assert!(check_angle_frames(&frame, [0, 2], &frame, [1, 3], Some(&wrong), &cancel).is_err());
        wrong = valid.clone();
        wrong.entry_error = f64::EPSILON;
        assert!(check_angle_frames(&frame, [0, 2], &frame, [1, 3], Some(&wrong), &cancel).is_err());
        wrong = valid;
        wrong.eigensystem.values[0] = 0.5;
        assert!(check_angle_frames(&frame, [0, 2], &frame, [1, 3], Some(&wrong), &cancel).is_err());
    }

    #[test]
    fn gram_source_and_unused_witnesses_are_checked() {
        let cancel = Cancellation::default();
        let e = identity(4);
        let valid = proof(&[1., 0., 0., 0.], NormKind::Gram);
        let bounds = check_angle(&e, [0, 2], &e, [1, 4], Some(&valid), &cancel).unwrap();
        assert!(bounds[0] > 89.999 && bounds[1] == 90.);
        let mut wrong = valid.clone();
        wrong.matrix.data.swap(0, 5);
        assert!(check_angle(&e, [0, 2], &e, [1, 4], Some(&wrong), &cancel).is_err());
        assert_eq!(
            check_angle(&e, [0, 1], &e, [0, 3], None, &cancel).unwrap(),
            [0., 0.]
        );
        assert!(check_angle(&e, [0, 1], &e, [0, 3], Some(&valid), &cancel).is_err());
        assert!(check_angle(&e, [0, 4], &e, [0, 4], Some(&valid), &cancel).is_err());
    }

    #[test]
    fn polar_reconstruction_domains_and_cancellation_remain_independent() {
        let cancel = Cancellation::default();
        let base = identity(3);
        let mut raw = base.clone();
        raw.vectors.data = vec![1., 0., 0.25, 0., 1., 0., 0.25, 0., 1.];
        let a = CheckerFrame::new(&raw, &cancel).unwrap();
        let b = CheckerFrame::new(&base, &cancel).unwrap();
        assert!(a.polar_bound() >= 0.25);
        let bounds = check_angle_frames(&a, [0, 1], &b, [0, 2], None, &cancel).unwrap();
        assert_eq!(bounds[0], 0.);
        assert!(check_angle_frames(&a, [1, 1], &b, [0, 2], None, &cancel).is_err());
        raw.vectors.data[0] = f64::NAN;
        assert!(CheckerFrame::new(&raw, &cancel).is_err());
        cancel.cancel();
        assert_eq!(
            check_angle(&base, [0, 3], &base, [0, 3], None, &cancel),
            Err(Error::Cancelled)
        );
    }

    #[test]
    fn independent_chord_enclosure_resolves_nearly_parallel_and_antiparallel_lines() {
        let cancel = Cancellation::default();
        let a = identity(2);
        for slope in [0., 1e-12, 1e-8, 1e-5] {
            let mut b = identity(2);
            // The exact polar factor of this scaled orthogonal matrix rotates by atan(slope).
            b.vectors.data = vec![1., slope, -slope, 1.];
            // For x>=0, x-x^3/3 <= atan(x) <= x; no validation-only libm feature.
            let x = point(slope);
            let expected = interval((x - x * x * x / point(3.)).inf(), x.sup()).unwrap()
                * point(180.)
                / Interval::PI;
            for sign in [1., -1.] {
                let mut signed = b.clone();
                for value in &mut signed.vectors.data {
                    *value *= sign;
                }
                let enclosed = check_angle(&a, [0, 1], &signed, [0, 1], None, &cancel).unwrap();
                assert!(enclosed[0] <= expected.inf() && enclosed[1] >= expected.sup());
                assert!(enclosed[1] - enclosed[0] < 1e-7);
                if slope == 0. {
                    assert_eq!(enclosed, [0., 0.]);
                }
            }
        }
        let right = check_angle(&a, [0, 1], &a, [1, 2], None, &cancel).unwrap();
        assert!(right[0] > 89.999999 && right[1] == 90.);
    }
}
