//! Reference point angles between polar-Q projectors of checked eigenvector frames.
//!
//! Scope: one generic point-reference comparison. Eigensystem residuals, target-region
//! uncertainty, beta/node radii and regional widening are charged later; no tolerance
//! classification happens here. The norm witnesses are generic proposals that an
//! independent checker inspects without re-running the platform eigensolver.
use crate::{
    Eigensystem, Error, Matrix, Result, numerics,
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
        zeros,
    },
    scalar::{self, Interval, point, square},
};

const MAX_DIMENSION: usize = 256;

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn exhausted(message: &str) -> Error {
    memory::text(message)
        .map(Error::Exhausted)
        .unwrap_or_else(|error| error)
}
fn overflow() -> Error {
    Error::Resource(String::new())
}
fn squared_len(n: usize) -> Result<usize> {
    n.checked_mul(n).ok_or_else(overflow)
}

/// A borrowed eigenvector frame whose polar distance was certified once at construction.
/// The borrow is private so the certified frame cannot be mutated afterwards.
#[derive(Debug)]
pub struct CheckedFrame<'a> {
    system: &'a Eigensystem,
    polar: f64,
}
impl<'a> CheckedFrame<'a> {
    pub fn new(system: &'a Eigensystem, cancel: &Cancellation) -> Result<CheckedFrame<'a>> {
        cancel.poll()?;
        let n = system.vectors.n;
        if n == 0 || n > MAX_DIMENSION {
            return Err(invalid("rolling angle frame dimension 1..256 required"));
        }
        if system.values.len() != n || system.vectors.data.len() != squared_len(n)? {
            return Err(invalid("rolling angle frame shape mismatch"));
        }
        if !system.backward.is_finite()
            || system.backward < 0.
            || !system.polar.is_finite()
            || system.polar < 0.
        {
            return Err(invalid(
                "rolling angle frame guards must be finite and nonnegative",
            ));
        }
        for (index, value) in system.values.iter().enumerate() {
            // Descending (not necessarily positive) eigenvalue order.
            if !value.is_finite() || (index > 0 && system.values[index - 1] < *value) {
                return Err(invalid(
                    "finite descending rolling angle frame values required",
                ));
            }
        }
        // Vectors are a general (not symmetric) matrix: finiteness only.
        for column in system.vectors.data.chunks(n) {
            cancel.poll()?;
            if column.iter().any(|x| !x.is_finite()) {
                return Err(invalid("finite rolling angle frame vectors required"));
            }
        }
        // O(N^3) once per frame; never repeated per edge.
        let polar = numerics::frame_polar(&system.vectors, cancel)?;
        if !polar.is_finite() || polar < 0. {
            return Err(exhausted("rolling angle frame polar bound unresolved"));
        }
        Ok(Self { system, polar })
    }
    pub fn dimension(&self) -> usize {
        self.system.vectors.n
    }
    /// Spectral-norm bound on the distance between the stored columns and their polar factor Q.
    pub fn polar_bound(&self) -> f64 {
        self.polar
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonKind {
    LineLine,
    EqualBlock,
    Containment,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmallerSide {
    Neither,
    Source,
    Target,
}
#[derive(Debug, PartialEq)]
pub enum NormWitness {
    None,
    /// Midpoint of the interval raw projector difference, its EVD proposal and
    /// the Frobenius distance of the interval difference from the midpoint.
    Symmetric {
        midpoint: Matrix,
        eigensystem: Eigensystem,
        entry_error: f64,
    },
    /// Midpoint of the interval Gram R^T R of the directed raw residual.
    Gram {
        midpoint: Matrix,
        eigensystem: Eigensystem,
        entry_error: f64,
    },
}
#[derive(Debug, PartialEq)]
pub struct AngleEvidence {
    pub kind: ComparisonKind,
    pub smaller_side: SmallerSide,
    /// Outward reference-point angle in degrees within [0,90].
    pub point_deg: [f64; 2],
    pub trivial_ambient: bool,
    pub norm: NormWitness,
}

fn checked_sum(values: &[usize]) -> Result<usize> {
    values
        .iter()
        .try_fold(0usize, |s, &v| s.checked_add(v).ok_or_else(overflow))
}
impl HeapBytes for ComparisonKind {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for SmallerSide {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for NormWitness {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::None => Ok(0),
            Self::Symmetric {
                midpoint,
                eigensystem,
                ..
            }
            | Self::Gram {
                midpoint,
                eigensystem,
                ..
            } => checked_sum(&[midpoint.heap_bytes()?, eigensystem.heap_bytes()?]),
        }
    }
}
impl HeapBytes for AngleEvidence {
    fn heap_bytes(&self) -> Result<usize> {
        self.norm.heap_bytes()
    }
}

fn exact_zero(kind: ComparisonKind, smaller_side: SmallerSide, trivial: bool) -> AngleEvidence {
    AngleEvidence {
        kind,
        smaller_side,
        point_deg: [0., 0.],
        trivial_ambient: trivial,
        norm: NormWitness::None,
    }
}

/// Intersect with the proven range [0,1]; an empty result is a contradiction, not clamped.
fn unit(x: Interval) -> Result<Interval> {
    let r = x.intersection(scalar::interval(0., 1.)?);
    if r.is_empty() {
        return Err(exhausted(
            "rolling angle cosine/sine enclosure contradicts [0,1]",
        ));
    }
    Ok(r)
}
fn degrees(radians: Interval) -> Result<[f64; 2]> {
    let d = (radians * (point(180.) / Interval::PI)).intersection(scalar::interval(0., 90.)?);
    if d.is_empty() || !d.inf().is_finite() || !d.sup().is_finite() {
        return Err(exhausted(
            "rolling angle degree enclosure contradicts [0,90]",
        ));
    }
    // Normalise a signed zero lower endpoint.
    Ok([d.inf() + 0., d.sup() + 0.])
}
/// Spectral bound on |Q Q^T - V V^T| for a column block with polar distance d.
fn projector_correction(d: f64) -> Interval {
    (point(2.) + point(d)) * point(d)
}
fn symmetric_radius(r: f64) -> Result<Interval> {
    scalar::interval(-r, r)
}

/// Any finite proposal is certified by the later Frobenius residual; prefer the midpoint.
fn proposal(x: Interval) -> Result<f64> {
    let (lo, hi) = (x.inf(), x.sup());
    if x.is_empty() || !lo.is_finite() || !hi.is_finite() {
        return Err(exhausted("rolling angle interval entry not finite"));
    }
    let m = 0.5 * lo + 0.5 * hi;
    Ok(if m.is_finite() && lo <= m && m <= hi {
        m + 0.
    } else {
        lo + 0.
    })
}

fn square_intervals(n: usize) -> Result<Vec<Interval>> {
    let nn = squared_len(n)?;
    let mut data = memory::capacity::<Interval>(nn)?;
    data.resize(nn, point(0.));
    Ok(data)
}

/// Interval enclosure of the raw projector V_block V_block^T (upper triangle mirrored).
fn raw_projector(v: &Matrix, rank: [usize; 2], cancel: &Cancellation) -> Result<Vec<Interval>> {
    let n = v.n;
    let mut p = square_intervals(n)?;
    for j in 0..n {
        cancel.poll()?;
        for i in 0..=j {
            let mut s = point(0.);
            for k in rank[0]..rank[1] {
                s += point(v.at(i, k)) * point(v.at(j, k));
            }
            p[i + j * n] = s;
            p[j + i * n] = s;
        }
    }
    Ok(p)
}

/// Exactly symmetric finite midpoint and Frobenius distance of the interval matrix from it.
fn midpoint(x: &[Interval], n: usize, cancel: &Cancellation) -> Result<(Matrix, f64)> {
    let mut data = zeros(squared_len(n)?)?;
    let mut error = point(0.);
    for j in 0..n {
        cancel.poll()?;
        for i in 0..=j {
            let m = proposal(x[i + j * n])?;
            data[i + j * n] = m;
            data[j + i * n] = m;
            error += square(x[i + j * n] - point(m));
            if i != j {
                error += square(x[j + i * n] - point(m));
            }
        }
    }
    let entry_error = scalar::upper(error.sqrt())?;
    Ok((Matrix { n, data }, entry_error))
}

fn guard(eigensystem: &Eigensystem) -> Result<f64> {
    let b = eigensystem.backward;
    if !b.is_finite() || b < 0. || eigensystem.values.is_empty() {
        return Err(exhausted("rolling angle norm witness unresolved"));
    }
    Ok(b)
}

fn line(
    a: &CheckedFrame,
    a_rank: [usize; 2],
    b: &CheckedFrame,
    b_rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<AngleEvidence> {
    let n = a.dimension();
    let (x, y) = (&a.system.vectors, &b.system.vectors);
    let (i, j) = (a_rank[0], b_rank[0]);
    cancel.poll()?;
    let (mut dot, mut minus, mut plus) = (point(0.), point(0.), point(0.));
    for k in 0..n {
        let (u, v) = (point(x.at(k, i)), point(y.at(k, j)));
        dot += u * v;
        minus += square(u - v);
        plus += square(u + v);
    }
    let (da, db) = (point(a.polar), point(b.polar));
    let d = scalar::upper(da + db + da * db)?;
    let cosine = unit((dot + symmetric_radius(d)?).abs())?;
    let cosine_angle = degrees(scalar::acos(cosine)?)?;
    // For unit polar columns, theta = 2 asin(min(||u-v||,||u+v||)/2).
    // The chord enclosure avoids acos losing square-root precision near zero.
    let (minus, plus) = (minus.sqrt(), plus.sqrt());
    let chord = scalar::interval(minus.inf().min(plus.inf()), minus.sup().min(plus.sup()))?;
    let half_chord = unit((chord + symmetric_radius(scalar::upper(da + db)?)?) / point(2.))?;
    let chord_angle = degrees(point(2.) * scalar::asin(half_chord)?)?;
    let point_deg = [
        cosine_angle[0].max(chord_angle[0]),
        cosine_angle[1].min(chord_angle[1]),
    ];
    if point_deg[0] > point_deg[1] {
        return Err(exhausted("rolling cosine and chord certificates disagree"));
    }
    Ok(AngleEvidence {
        kind: ComparisonKind::LineLine,
        smaller_side: SmallerSide::Neither,
        point_deg,
        trivial_ambient: false,
        norm: NormWitness::None,
    })
}

fn sine_degrees(norm: Interval, correction: Interval) -> Result<[f64; 2]> {
    let c = scalar::upper(correction)?;
    let sine = unit(norm + symmetric_radius(c)?)?;
    degrees(scalar::asin(sine)?)
}

fn equal_block(
    a: &CheckedFrame,
    a_rank: [usize; 2],
    b: &CheckedFrame,
    b_rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<AngleEvidence> {
    let n = a.dimension();
    let (midpoint, entry_error, eigensystem) = {
        let pa = raw_projector(&a.system.vectors, a_rank, cancel)?;
        let mut difference = raw_projector(&b.system.vectors, b_rank, cancel)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..n {
                let k = i + j * n;
                difference[k] = pa[k] - difference[k];
            }
        }
        drop(pa);
        let (m, e) = midpoint(&difference, n, cancel)?;
        drop(difference);
        let eigensystem = numerics::eigensystem(&m, cancel)?;
        (m, e, eigensystem)
    };
    let backward = guard(&eigensystem)?;
    let values = &eigensystem.values;
    let centre = values[0].abs().max(values[values.len() - 1].abs());
    let radius = scalar::upper(point(backward) + point(entry_error))?;
    let norm = point(centre) + symmetric_radius(radius)?;
    let point_deg = sine_degrees(
        norm,
        projector_correction(a.polar) + projector_correction(b.polar),
    )?;
    Ok(AngleEvidence {
        kind: ComparisonKind::EqualBlock,
        smaller_side: SmallerSide::Neither,
        point_deg,
        trivial_ambient: false,
        norm: NormWitness::Symmetric {
            midpoint,
            eigensystem,
            entry_error,
        },
    })
}

fn containment(
    small: (&CheckedFrame, [usize; 2]),
    large: (&CheckedFrame, [usize; 2]),
    side: SmallerSide,
    cancel: &Cancellation,
) -> Result<AngleEvidence> {
    let n = small.0.dimension();
    if small.1[1] - small.1[0] == 1 {
        return line_containment(small, large, side, cancel);
    }
    let (midpoint, entry_error, eigensystem) = {
        let ps = raw_projector(&small.0.system.vectors, small.1, cancel)?;
        let pl = raw_projector(&large.0.system.vectors, large.1, cancel)?;
        // R = (I - P_large) P_small = P_small - P_large P_small, all in interval arithmetic.
        let mut r = square_intervals(n)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..n {
                let mut s = ps[i + j * n];
                for k in 0..n {
                    s -= pl[i + k * n] * ps[k + j * n];
                }
                r[i + j * n] = s;
            }
        }
        drop(pl);
        drop(ps);
        // G = R^T R; upper triangle computed once and mirrored.
        let mut g = square_intervals(n)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..=j {
                let mut s = point(0.);
                for k in 0..n {
                    s += r[k + i * n] * r[k + j * n];
                }
                g[i + j * n] = s;
                g[j + i * n] = s;
            }
        }
        drop(r);
        let (m, e) = midpoint(&g, n, cancel)?;
        drop(g);
        let eigensystem = numerics::eigensystem(&m, cancel)?;
        (m, e, eigensystem)
    };
    let backward = guard(&eigensystem)?;
    // Largest algebraic eigenvalue; the exact Gram is PSD.
    let radius = scalar::upper(point(backward) + point(entry_error))?;
    let gram = point(eigensystem.values[0]) + symmetric_radius(radius)?;
    if gram.is_empty() || !gram.sup().is_finite() || gram.sup() < 0. {
        return Err(exhausted(
            "rolling angle Gram enclosure contradicts positive semidefiniteness",
        ));
    }
    let gram = scalar::interval(gram.inf().max(0.), scalar::upper(gram)?)?;
    let norm = gram.sqrt();
    let (cs, cl) = (
        projector_correction(small.0.polar),
        projector_correction(large.0.polar),
    );
    let point_deg = sine_degrees(norm, cs + cl + cs * cl)?;
    Ok(AngleEvidence {
        kind: ComparisonKind::Containment,
        smaller_side: side,
        point_deg,
        trivial_ambient: false,
        norm: NormWitness::Gram {
            midpoint,
            eigensystem,
            entry_error,
        },
    })
}

fn line_containment(
    small: (&CheckedFrame, [usize; 2]),
    large: (&CheckedFrame, [usize; 2]),
    side: SmallerSide,
    cancel: &Cancellation,
) -> Result<AngleEvidence> {
    // The raw residual is y*x^T, so its operator norm is exactly ||y||*||x||.
    let n = small.0.dimension();
    let (x, v) = (&small.0.system.vectors, &large.0.system.vectors);
    let column = small.1[0];
    let mut coefficients = memory::capacity(large.1[1] - large.1[0])?;
    for k in large.1[0]..large.1[1] {
        cancel.poll()?;
        let dot = (0..n).fold(point(0.), |sum, i| {
            sum + point(v.at(i, k)) * point(x.at(i, column))
        });
        memory::push(&mut coefficients, dot)?;
    }
    let (mut norm_x, mut norm_y) = (point(0.), point(0.));
    for i in 0..n {
        cancel.poll()?;
        let value = point(x.at(i, column));
        let projected = coefficients
            .iter()
            .enumerate()
            .fold(point(0.), |sum, (k, dot)| {
                sum + point(v.at(i, large.1[0] + k)) * *dot
            });
        norm_x += square(value);
        norm_y += square(value - projected);
    }
    let (cs, cl) = (
        projector_correction(small.0.polar),
        projector_correction(large.0.polar),
    );
    Ok(AngleEvidence {
        kind: ComparisonKind::Containment,
        smaller_side: side,
        point_deg: sine_degrees(norm_x.sqrt() * norm_y.sqrt(), cs + cl + cs * cl)?,
        trivial_ambient: false,
        norm: NormWitness::None,
    })
}

/// Reference-point principal angle between the polar-Q projectors of two selected blocks.
pub fn compare(
    a: &CheckedFrame,
    a_rank: [usize; 2],
    b: &CheckedFrame,
    b_rank: [usize; 2],
    cancel: &Cancellation,
) -> Result<AngleEvidence> {
    cancel.poll()?;
    let n = a.dimension();
    let valid = |rank: [usize; 2]| rank[0] < rank[1] && rank[1] <= n;
    if b.dimension() != n || !valid(a_rank) || !valid(b_rank) {
        return Err(invalid(
            "matching frame dimensions and half-open ranks within them required",
        ));
    }
    let (wa, wb) = (a_rank[1] - a_rank[0], b_rank[1] - b_rank[0]);
    // The whole-ambient projector I is basis independent: exact analytical zero.
    match (wa == n, wb == n) {
        (true, true) => {
            return Ok(exact_zero(
                ComparisonKind::EqualBlock,
                SmallerSide::Neither,
                true,
            ));
        }
        (true, false) => {
            return Ok(exact_zero(
                ComparisonKind::Containment,
                SmallerSide::Target,
                true,
            ));
        }
        (false, true) => {
            return Ok(exact_zero(
                ComparisonKind::Containment,
                SmallerSide::Source,
                true,
            ));
        }
        (false, false) => {}
    }
    let (kind, side) = if wa == 1 && wb == 1 {
        (ComparisonKind::LineLine, SmallerSide::Neither)
    } else if wa == wb {
        (ComparisonKind::EqualBlock, SmallerSide::Neither)
    } else if wa < wb {
        (ComparisonKind::Containment, SmallerSide::Source)
    } else {
        (ComparisonKind::Containment, SmallerSide::Target)
    };
    // Same borrowed frame and rank: identical polar projector, exact zero.
    if std::ptr::eq(a.system, b.system) && a_rank == b_rank {
        return Ok(exact_zero(kind, side, false));
    }
    match (kind, side) {
        (ComparisonKind::LineLine, _) => line(a, a_rank, b, b_rank, cancel),
        (ComparisonKind::EqualBlock, _) => {
            memory::scope(|| equal_block(a, a_rank, b, b_rank, cancel))
        }
        (ComparisonKind::Containment, SmallerSide::Source) => {
            memory::scope(|| containment((a, a_rank), (b, b_rank), side, cancel))
        }
        (ComparisonKind::Containment, _) => {
            memory::scope(|| containment((b, b_rank), (a, a_rank), side, cancel))
        }
    }
}
