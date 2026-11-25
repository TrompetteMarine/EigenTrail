//! Conditional Thompson-region bounds about checked SPD reference frames.
//!
//! These helpers neither calibrate a sampling event nor decide graph edges.
//! Their frame inputs have already passed the independent SPD/numerical checks.
use super::numerical::ReferenceFrame;
use crate::{
    Error, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    scalar::{self, Interval, interval, point},
    types::Matrix,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupBound {
    WholeSpace,
    Separated {
        total_rho: f64,
        external_gap: [f64; 2],
        beta_radians: [f64; 2],
    },
    MergeRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferReason {
    CentreFloor,
    ConditioningCut,
    ArithmeticRange,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransferBound {
    Bound {
        rho: f64,
        t: [f64; 2],
        sqrt_condition: [f64; 2],
    },
    Unsupported {
        reason: TransferReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanarSign {
    Positive,
    Negative,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarBound {
    pub centre_correlation: [f64; 2],
    pub population_correlation: [f64; 2],
    pub cutoff: [f64; 2],
    pub sign: PlanarSign,
}

impl HeapBytes for GroupBound {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for TransferBound {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for PlanarBound {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}

fn invalid(message: &str) -> Error {
    match memory::text(message) {
        Ok(message) => Error::Invalid(message),
        Err(error) => error,
    }
}

fn exhausted(message: &str) -> Error {
    match memory::text(message) {
        Ok(message) => Error::Exhausted(message),
        Err(error) => error,
    }
}

fn nonnegative(value: f64) -> Result<()> {
    if value.is_finite() && value >= 0. {
        Ok(())
    } else {
        Err(invalid(
            "regional bounds require finite nonnegative radii and errors",
        ))
    }
}

fn finite(value: Interval) -> Result<Interval> {
    if value.is_empty() || !value.inf().is_finite() || !value.sup().is_finite() {
        Err(exhausted(
            "regional numerical enclosure exceeds finite range",
        ))
    } else {
        Ok(value)
    }
}

fn endpoints(value: Interval) -> [f64; 2] {
    [value.inf(), value.sup()]
}

fn checked_matrix(matrix: &Matrix, cancel: &Cancellation) -> Result<()> {
    cancel.poll()?;
    if matrix.n > 256 {
        return Err(Error::Resource(String::new()));
    }
    if matrix.n == 0 || matrix.n.checked_mul(matrix.n) != Some(matrix.data.len()) {
        return Err(invalid("regional matrix requires finite square data"));
    }
    for j in 0..matrix.n {
        cancel.poll()?;
        for i in 0..matrix.n {
            if !matrix.at(i, j).is_finite() || i < j && matrix.at(i, j) != matrix.at(j, i) {
                return Err(invalid(
                    "regional matrix must be finite and exactly symmetric",
                ));
            }
        }
    }
    Ok(())
}

fn checked_frame(frame: &ReferenceFrame, cancel: &Cancellation) -> Result<()> {
    checked_matrix(&frame.matrix, cancel)?;
    nonnegative(frame.matrix_error)?;
    nonnegative(frame.numerical_rho)?;
    let n = frame.matrix.n;
    let e = &frame.eigensystem;
    nonnegative(e.backward)?;
    nonnegative(e.polar)?;
    if e.values.len() != n || e.vectors.n != n || e.vectors.data.len() != frame.matrix.data.len() {
        return Err(invalid("regional frame dimensions differ"));
    }
    for (index, &value) in e.values.iter().enumerate() {
        cancel.poll()?;
        if !value.is_finite() || value <= 0. || index > 0 && e.values[index - 1] < value {
            return Err(invalid(
                "regional reference spectrum must be positive and descending",
            ));
        }
    }
    for column in e.vectors.data.chunks(n) {
        cancel.poll()?;
        if column.iter().any(|value| !value.is_finite()) {
            return Err(invalid("regional frame contains nonfinite vectors"));
        }
    }
    Ok(())
}

fn separated_beta(gap: Interval, total: f64) -> Result<[f64; 2]> {
    if total == 0. {
        return Ok([0., 0.]);
    }
    let half_rho = finite(point(total) / point(2.))?;
    let half_gap = finite((gap - point(total)) / point(2.))?;
    if half_gap.inf() <= 0. {
        return Err(exhausted("regional angular denominator unresolved"));
    }
    let (numerator, _) = scalar::sinh_cosh_nonnegative(half_rho)?;
    let (denominator, _) = scalar::sinh_cosh_nonnegative(half_gap)?;
    if denominator.inf() <= 0. {
        return Err(exhausted("regional angular denominator unresolved"));
    }
    let sine = finite(numerator / denominator)?;
    let lower = sine.inf().max(0.);
    let upper = sine.sup().min(1.);
    if lower > upper {
        return Err(exhausted("regional angular sine unresolved"));
    }
    Ok(endpoints(scalar::asin(interval(lower, upper)?)?))
}

/// Adds this frame's numerical alignment radius exactly once, then tests both
/// exterior boundaries. A failed strict gap requires merging, not an exact tie.
pub fn group_bound(
    frame: &ReferenceFrame,
    start: usize,
    end: usize,
    declared_rho: f64,
    cancel: &Cancellation,
) -> Result<GroupBound> {
    cancel.poll()?;
    nonnegative(declared_rho)?;
    checked_frame(frame, cancel)?;
    let n = frame.eigensystem.values.len();
    if start >= end || end > n {
        return Err(invalid(
            "regional rank interval must be nonempty and within the frame",
        ));
    }
    if start == 0 && end == n {
        return Ok(GroupBound::WholeSpace);
    }
    let total = point(declared_rho) + point(frame.numerical_rho);
    if !total.sup().is_finite() {
        return Ok(GroupBound::MergeRequired);
    }
    let total_rho = total.sup();
    let values = &frame.eigensystem.values;
    let mut external: Option<Interval> = None;
    for boundary in [start, end] {
        cancel.poll()?;
        if boundary == 0 || boundary == n {
            continue;
        }
        let gap = finite(
            scalar::ln(point(values[boundary - 1]))? - scalar::ln(point(values[boundary]))?,
        )?;
        external = Some(match external {
            None => gap,
            Some(previous) => {
                interval(previous.inf().min(gap.inf()), previous.sup().min(gap.sup()))?
            }
        });
    }
    let gap = external.ok_or_else(|| invalid("proper regional group has no exterior boundary"))?;
    if gap.inf() <= (point(2.) * point(total_rho)).sup() {
        return Ok(GroupBound::MergeRequired);
    }
    let beta_radians = match memory::scope(|| separated_beta(gap, total_rho)) {
        Ok(value) => value,
        // Separation is already proved; the universal angle is still valid.
        Err(Error::Exhausted(_)) => [0., Interval::FRAC_PI_2.sup()],
        Err(error) => return Err(error),
    };
    cancel.poll()?;
    Ok(GroupBound::Separated {
        total_rho,
        external_gap: endpoints(gap),
        beta_radians,
    })
}

fn transferred_radius(frame: &ReferenceFrame, root_rho: f64) -> Result<TransferBound> {
    let e = &frame.eigensystem;
    let error = finite(point(e.backward) + point(frame.matrix_error))?;
    let lower = finite(point(e.values[e.values.len() - 1]) - point(error.sup()))?;
    let upper = finite(point(e.values[0]) + point(error.sup()))?;
    if lower.inf() <= 0. {
        return Ok(TransferBound::Unsupported {
            reason: TransferReason::CentreFloor,
        });
    }
    let sqrt_condition = finite(upper.sqrt() / lower.sqrt())?;
    if root_rho == 0. {
        return Ok(TransferBound::Bound {
            rho: 0.,
            t: [0., 0.],
            sqrt_condition: endpoints(sqrt_condition),
        });
    }
    let quarter = finite(point(root_rho) / point(4.))?;
    let (sinh, cosh) = scalar::sinh_cosh_nonnegative(quarter)?;
    let expm1_half = finite(point(2.) * sinh * (cosh + sinh))?;
    let twice = finite(point(2.) * point(root_rho))?;
    let exp_twice = scalar::exp_nonnegative(twice)?;
    let t = finite(exp_twice * sqrt_condition * expm1_half)?;
    if t.sup() >= 1. {
        return Ok(TransferBound::Unsupported {
            reason: TransferReason::ConditioningCut,
        });
    }
    let denominator = finite(point(1.) - t)?;
    if denominator.inf() <= 0. {
        return Ok(TransferBound::Unsupported {
            reason: TransferReason::ConditioningCut,
        });
    }
    let radius = finite(point(root_rho) - point(2.) * scalar::ln(denominator)?)?;
    Ok(TransferBound::Bound {
        rho: radius.sup(),
        t: endpoints(t),
        sqrt_condition: endpoints(sqrt_condition),
    })
}

/// Radius about the exact C/Q centre. `group_bound` adds only that centre's
/// numerical radius later; the root numerical radius is not a transfer input.
pub fn normalisation_radius(
    frame: &ReferenceFrame,
    root_rho: f64,
    cancel: &Cancellation,
) -> Result<TransferBound> {
    cancel.poll()?;
    nonnegative(root_rho)?;
    checked_frame(frame, cancel)?;
    let result = memory::scope(|| transferred_radius(frame, root_rho));
    cancel.poll()?;
    match result {
        Err(Error::Exhausted(_)) => Ok(TransferBound::Unsupported {
            reason: TransferReason::ArithmeticRange,
        }),
        other => other,
    }
}

/// Sharp closed planar correlation interval, evaluated directly from the same
/// SPD root input; it does not add a covariance eigensolver radius.
pub fn planar_correlation(
    root: &Matrix,
    root_rho: f64,
    cancel: &Cancellation,
) -> Result<PlanarBound> {
    cancel.poll()?;
    nonnegative(root_rho)?;
    checked_matrix(root, cancel)?;
    if root.n != 2 || root.at(0, 0) <= 0. || root.at(1, 1) <= 0. {
        return Err(invalid(
            "sharp planar correlation requires two positive variances",
        ));
    }
    let centre =
        finite(point(root.at(0, 1)) / point(root.at(0, 0)).sqrt() / point(root.at(1, 1)).sqrt())?;
    if centre.inf() >= 1. || centre.sup() <= -1. {
        return Err(invalid(
            "sharp planar correlation requires a strictly SPD root",
        ));
    }
    let centre = interval(centre.inf().max(-1.), centre.sup().min(1.))?;
    let cutoff = scalar::tanh_nonnegative(point(root_rho))?;
    let product = finite(centre * cutoff)?;
    let minus_denominator = finite(point(1.) - product)?;
    let plus_denominator = finite(point(1.) + product)?;
    if minus_denominator.inf() <= 0. || plus_denominator.inf() <= 0. {
        return Err(exhausted(
            "sharp planar normalisation denominator unresolved",
        ));
    }
    let lower = finite((centre - cutoff) / minus_denominator)?;
    let upper = finite((centre + cutoff) / plus_denominator)?;
    let lo = lower.inf().max(-1.);
    let hi = upper.sup().min(1.);
    if lo > hi {
        return Err(exhausted("sharp planar correlation range unresolved"));
    }
    let sign = if lo > 0. {
        PlanarSign::Positive
    } else if hi < 0. {
        PlanarSign::Negative
    } else {
        PlanarSign::Unresolved
    };
    cancel.poll()?;
    Ok(PlanarBound {
        centre_correlation: endpoints(centre),
        population_correlation: [lo, hi],
        cutoff: endpoints(cutoff),
        sign,
    })
}

#[cfg(test)]
mod tests {
    use super::super::numerical;
    use super::*;

    fn diagonal(values: &[f64]) -> Matrix {
        let mut data = vec![0.; values.len() * values.len()];
        for (index, &value) in values.iter().enumerate() {
            data[index + index * values.len()] = value;
        }
        Matrix {
            n: values.len(),
            data,
        }
    }
    fn planar(correlation: f64) -> Matrix {
        Matrix {
            n: 2,
            data: vec![1., correlation, correlation, 1.],
        }
    }
    fn contains(bounds: [f64; 2], value: f64) {
        assert!(
            bounds[0] <= value && value <= bounds[1],
            "{bounds:?} excludes {value:?}"
        );
    }

    #[test]
    fn strict_exterior_gaps_and_whole_space_are_distinct() {
        let cancel = Cancellation::default();
        let frame = numerical::root_frame(&diagonal(&[4., 1.]), &cancel).unwrap();
        assert_eq!(
            group_bound(&frame, 0, 2, f64::MAX, &cancel).unwrap(),
            GroupBound::WholeSpace
        );
        let GroupBound::Separated {
            total_rho,
            external_gap,
            beta_radians,
        } = group_bound(&frame, 0, 1, 0., &cancel).unwrap()
        else {
            panic!("separated point line required")
        };
        assert_eq!(total_rho, 0.);
        contains(external_gap, 4.0_f64.ln());
        assert_eq!(beta_radians, [0., 0.]);
        assert_eq!(
            group_bound(&frame, 0, 1, std::f64::consts::LN_2, &cancel).unwrap(),
            GroupBound::MergeRequired
        );
        assert!(matches!(
            group_bound(&frame, 0, 1, std::f64::consts::LN_2 - 1e-12, &cancel).unwrap(),
            GroupBound::Separated { .. }
        ));
        assert_eq!(
            group_bound(&frame, 0, 1, std::f64::consts::LN_2 + 1e-12, &cancel).unwrap(),
            GroupBound::MergeRequired
        );
    }

    #[test]
    fn numerical_alignment_is_added_once() {
        let cancel = Cancellation::default();
        let mut frame = numerical::root_frame(&diagonal(&[4., 1.]), &cancel).unwrap();
        let expected = group_bound(&frame, 0, 1, 0.2, &cancel).unwrap();
        frame.numerical_rho = 0.1;
        assert_eq!(group_bound(&frame, 0, 1, 0.1, &cancel).unwrap(), expected);
        let GroupBound::Separated { beta_radians, .. } = expected else {
            panic!("separated group required")
        };
        let beta = (0.1_f64.sinh() / ((4.0_f64.ln() - 0.2) / 2.).sinh()).asin();
        contains(beta_radians, beta);
    }

    #[test]
    fn transfer_zero_stable_formula_and_unsupported_cuts() {
        let cancel = Cancellation::default();
        let mut frame = numerical::root_frame(&diagonal(&[4., 1.]), &cancel).unwrap();
        assert_eq!(
            normalisation_radius(&frame, 0., &cancel).unwrap(),
            TransferBound::Bound {
                rho: 0.,
                t: [0., 0.],
                sqrt_condition: [2., 2.]
            }
        );
        let TransferBound::Bound {
            rho,
            t,
            sqrt_condition,
        } = normalisation_radius(&frame, 0.01, &cancel).unwrap()
        else {
            panic!("small radius transfer required")
        };
        let reference_t = (0.02_f64).exp() * 2. * (0.005_f64).exp_m1();
        contains(t, reference_t);
        contains(sqrt_condition, 2.);
        assert!(rho >= 0.01 - 2. * (-reference_t).ln_1p());
        assert_eq!(
            normalisation_radius(&frame, 1., &cancel).unwrap(),
            TransferBound::Unsupported {
                reason: TransferReason::ConditioningCut
            }
        );
        assert_eq!(
            normalisation_radius(&frame, 1e308, &cancel).unwrap(),
            TransferBound::Unsupported {
                reason: TransferReason::ArithmeticRange
            }
        );
        frame.matrix_error = 1.;
        assert_eq!(
            normalisation_radius(&frame, 0., &cancel).unwrap(),
            TransferBound::Unsupported {
                reason: TransferReason::CentreFloor
            }
        );
    }

    #[test]
    fn sharp_planar_intervals_keep_zero_and_sign() {
        let cancel = Cancellation::default();
        for (r, sign) in [
            (0.5, PlanarSign::Positive),
            (-0.5, PlanarSign::Negative),
            (0., PlanarSign::Unresolved),
        ] {
            let bound = planar_correlation(&planar(r), 0., &cancel).unwrap();
            assert_eq!(bound.centre_correlation, [r, r]);
            assert_eq!(bound.population_correlation, [r, r]);
            assert_eq!(bound.cutoff, [0., 0.]);
            assert_eq!(bound.sign, sign);
        }
        let result = planar_correlation(&planar(0.), 0.1, &cancel).unwrap();
        contains(result.population_correlation, -0.1_f64.tanh());
        contains(result.population_correlation, 0.1_f64.tanh());
        assert_eq!(result.sign, PlanarSign::Unresolved);
        let straddling = planar_correlation(&planar(0.6), std::f64::consts::LN_2, &cancel).unwrap();
        contains(straddling.population_correlation, 0.);
        assert_eq!(straddling.sign, PlanarSign::Unresolved);
    }

    #[test]
    fn malformed_inputs_cancellation_and_refusal_propagate() {
        let cancel = Cancellation::default();
        let frame = numerical::root_frame(&diagonal(&[4., 1.]), &cancel).unwrap();
        for rho in [-1., f64::NAN, f64::INFINITY] {
            assert!(matches!(
                group_bound(&frame, 0, 1, rho, &cancel),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                normalisation_radius(&frame, rho, &cancel),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                planar_correlation(&planar(0.5), rho, &cancel),
                Err(Error::Invalid(_))
            ));
        }
        assert!(group_bound(&frame, 1, 1, 0., &cancel).is_err());
        assert!(group_bound(&frame, 0, 3, 0., &cancel).is_err());
        assert!(planar_correlation(&diagonal(&[1., 1., 1.]), 0., &cancel).is_err());
        assert!(planar_correlation(&planar(2.), 0., &cancel).is_err());
        let operation = memory::Operation::new(8192).unwrap();
        assert!(matches!(
            operation.run(|| group_bound(&frame, 0, 1, f64::NAN, &cancel)),
            Err(Error::Resource(_))
        ));
        cancel.cancel();
        assert_eq!(
            group_bound(&frame, 0, 1, 0., &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(
            normalisation_radius(&frame, 0., &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(
            planar_correlation(&planar(0.5), 0., &cancel),
            Err(Error::Cancelled)
        );
    }
}
