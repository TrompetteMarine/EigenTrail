use eigentrail_core::{
    jumps::{self, JumpModel, JumpReport},
    resources::Cancellation,
    *,
};

fn fixture() -> (TraceRequest, JumpModel) {
    let mut request = TraceRequest::conditional(
        vec![0., 1., 2.],
        [2., 3.125, 3.25]
            .into_iter()
            .map(|x| Matrix {
                n: 2,
                data: vec![x, 0., 0., 1.],
            })
            .collect(),
        vec![0.0625; 3],
        0.125,
    );
    if let Calibration::Conditional { assumptions, .. } = &mut request.calibration {
        *assumptions = "supplied nodal covariance envelope; regularity between jumps".into();
    }
    (
        request,
        JumpModel {
            at_most_one_per_cell: true,
            minimum_jump: Some(1.),
            minimum_applies_to_all_candidates: true,
        },
    )
}

#[test]
fn et27_29_independently_checked_finite_jump_family() {
    let (request, model) = fixture();
    let report = jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&request, &model, &report).unwrap();
    // Exact rational oracle: jump norm 9/8, K=1/8, each error=1/16.
    assert_eq!(report.family.mandatory, [true, false]);
    assert_eq!(report.family.optional, [false, false]);
    assert_eq!(
        report.family.cells[0].amplitude,
        Range::closed(7. / 8., 11. / 8.)
    );
    assert_eq!(
        report.family.cells[0].timing,
        Range {
            lower: 0.,
            upper: 1.,
            lower_closed: false,
            upper_closed: true
        }
    );
    let encoded = serialization::encode(&report).unwrap();
    assert_eq!(
        serialization::decode::<JumpReport>(&encoded).unwrap(),
        report
    );
    let unrestricted = JumpModel {
        minimum_applies_to_all_candidates: false,
        ..model
    };
    let report =
        jumps::analyze_with_cancel(&request, &unrestricted, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&request, &unrestricted, &report).unwrap();
    assert_eq!(report.family.optional, [false, true]);
    let no_minimum = JumpModel {
        minimum_jump: None,
        ..unrestricted
    };
    let bytes = serialization::encode(&no_minimum).unwrap();
    assert_eq!(
        serialization::decode::<JumpModel>(&bytes).unwrap(),
        no_minimum
    );
    assert!(!String::from_utf8(bytes).unwrap().contains("null"));
}

#[test]
fn et36_43_jump_checker_rejects_tampering_and_model_rebinding() {
    let (request, mut model) = fixture();
    model.minimum_applies_to_all_candidates = false;
    let report = jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    let reject =
        |r: &JumpReport| assert!(eigentrail_check::jumps::check(&request, &model, r).is_err());
    let mut r = report.clone();
    r.family.optional[1] = false;
    reject(&r);
    let mut r = report.clone();
    r.family.cells.remove(1);
    reject(&r);
    let mut r = report.clone();
    r.differences.remove(1);
    reject(&r);
    let mut r = report.clone();
    r.differences[0].values[0] += 0.25;
    reject(&r);
    let mut r = report.clone();
    r.radii[0] = 0.;
    reject(&r);
    let mut r = report.clone();
    r.family.cells[0].timing.lower_closed = true;
    reject(&r);
    let mut r = report.clone();
    r.family.cells[0].amplitude.upper = 1.;
    reject(&r);
    let mut r = report.clone();
    r.completed = false;
    reject(&r);
    let mut r = report.clone();
    r.schema = types::SCHEMA.into();
    reject(&r);
    let mut r = report.clone();
    r.resource_qualification = "passed".into();
    reject(&r);
    let mut r = report.clone();
    r.calibration_scope = "finite-sample bootstrap certified".into();
    reject(&r);
    let rebound = JumpModel {
        minimum_applies_to_all_candidates: true,
        ..model
    };
    assert!(eigentrail_check::jumps::check(&request, &rebound, &report).is_err());
    let mut changed = request.clone();
    changed.covariances[0].data[0] += 0.25;
    assert!(eigentrail_check::jumps::check(&changed, &rebound, &report).is_err());
}

#[test]
fn et29_finite_norm_width_prevents_false_candidate_exclusion() {
    let (request, model) = fixture();
    let mut report =
        jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    // Valid coarse enclosure: norm=1/8, b=3/8 gives [0,1/2].
    // J=1 equals 2K+2r+zeta; the strict power gate must stay unresolved.
    report.differences[1].backward = 3. / 8.;
    report.family.optional[1] = true;
    report.family.cells[1].amplitude = Range::closed(0., 3. / 4.);
    eigentrail_check::jumps::check(&request, &model, &report).unwrap();
    report.family.optional[1] = false;
    assert!(eigentrail_check::jumps::check(&request, &model, &report).is_err());
}

#[test]
fn et27_nondiagonal_singular_input_checks_intended_increment_and_charges() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 3,
                data: vec![2., 0., 0., 0., 1., 0., 0., 0., 0.],
            },
            Matrix {
                n: 3,
                data: vec![2., 1., 0., 1., 2., 0., 0., 0., 0.],
            },
        ],
        vec![0.0625; 2],
        0.125,
    );
    let model = JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: None,
        minimum_applies_to_all_candidates: false,
    };
    let report = jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&request, &model, &report).unwrap();
    // Increment eigenvalues solve x^2-x-1=0; the third feature is exactly singular.
    let norm = (1. + 5f64.sqrt()) / 2.;
    assert!(report.family.cells[0].flagged);
    assert!((report.family.cells[0].amplitude.lower - (norm - 0.25)).abs() < 1e-12);
    assert!((report.family.cells[0].amplitude.upper - (norm + 0.25)).abs() < 1e-12);
    let mut gaussian = request;
    gaussian.calibration = Calibration::Gaussian {
        observations: 100_000_000,
        estimated_mean: false,
        divisor: 1.,
        alpha: 0.05,
        auxiliary_risk: 0.,
    };
    gaussian.correction = vec![0.001; 2];
    let report = jumps::analyze_with_cancel(&gaussian, &model, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&gaussian, &model, &report).unwrap();
    assert!(report.radii.iter().all(|e| *e > 0.001));
    let mut forged = report;
    forged.radii[0] = 0.001;
    assert!(eigentrail_check::jumps::check(&gaussian, &model, &forged).is_err());
}

#[test]
fn et42_jump_cancellation_resource_refusal_and_invalid_gaussian_risk() {
    let (request, model) = fixture();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        jumps::analyze_with_cancel(&request, &model, &cancel).unwrap_err(),
        Error::Cancelled
    );
    let report = jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    assert_eq!(
        eigentrail_check::jumps::check_with_cancel(&request, &model, &report, &cancel).unwrap_err(),
        Error::Cancelled
    );
    let mut small = request.clone();
    small.memory_bytes = 1;
    assert!(matches!(
        jumps::analyze_with_cancel(&small, &model, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
    let mut gaussian = request;
    gaussian.calibration = Calibration::Gaussian {
        observations: 100_000_000,
        estimated_mean: false,
        divisor: 100_000_000.,
        alpha: 0.0625,
        auxiliary_risk: 0.,
    };
    let mut report =
        jumps::analyze_with_cancel(&gaussian, &model, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&gaussian, &model, &report).unwrap();
    let mut continuous = track(&gaussian).unwrap();
    if let Calibration::Gaussian {
        alpha,
        auxiliary_risk,
        ..
    } = &mut gaussian.calibration
    {
        *alpha = 0.03125;
        *auxiliary_risk = -0.03125;
    }
    report.request_hash = serialization::digest(&gaussian).unwrap();
    // Same exact alpha-auxiliary budget and numerical outputs: only premise validation rejects it.
    report.calibration_scope = "Gaussian within-date iid homogeneous observations; known mean; arbitrary cross-date dependence; simultaneous failure <= 0.03125 including auxiliary risk -0.03125".into();
    continuous.request_hash = report.request_hash.clone();
    continuous.calibration_scope = report.calibration_scope.clone();
    assert!(eigentrail_check::check(&gaussian, &continuous).is_err());
    assert!(eigentrail_check::jumps::check(&gaussian, &model, &report).is_err());
    assert!(jumps::analyze_with_cancel(&gaussian, &model, &Cancellation::default()).is_err());
    assert!(jumps::cell(0., 1., 1., 0., f64::MAX, f64::MAX, 0.).is_err());
}

#[test]
fn et01_scalar_covariance_uses_exact_eigensystem_without_native_scratch() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![2.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    );
    let report = track(&request).unwrap();
    eigentrail_check::check(&request, &report).unwrap();
    assert_eq!(report.eigensystems[0].values, [2.]);
    assert_eq!(report.eigensystems[0].vectors.data, [1.]);
    assert_eq!(report.eigensystems[0].backward, 0.);
    let mut request = request;
    request.covariances[1].data[0] = 3.;
    let model = JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: None,
        minimum_applies_to_all_candidates: false,
    };
    let report = jumps::analyze_with_cancel(&request, &model, &Cancellation::default()).unwrap();
    eigentrail_check::jumps::check(&request, &model, &report).unwrap();
    assert_eq!(report.family.cells[0].amplitude, Range::closed(1., 1.));
    assert_eq!(report.family.mandatory, [true]);
}

#[test]
fn et42_extreme_finite_jump_enclosure_exhausts_without_panicking() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![f64::MAX / 4.],
            },
            Matrix {
                n: 1,
                data: vec![f64::MAX],
            },
        ],
        vec![0.; 2],
        0.,
    );
    let model = JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: None,
        minimum_applies_to_all_candidates: false,
    };
    let result = jumps::analyze_with_cancel(&request, &model, &Cancellation::default());
    assert!(matches!(result, Err(Error::Exhausted(_))), "{result:?}");
    let ordinary = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![2.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    );
    let mut report =
        jumps::analyze_with_cancel(&ordinary, &model, &Cancellation::default()).unwrap();
    report.request_hash = serialization::digest(&request).unwrap();
    report
        .eigensystems
        .iter_mut()
        .zip(&request.covariances)
        .for_each(|(e, a)| e.values[0] = a.data[0]);
    report.differences[0].values[0] = f64::MAX - f64::MAX / 4.;
    assert!(matches!(
        eigentrail_check::jumps::check(&request, &model, &report),
        Err(Error::Exhausted(_))
    ));
    assert!(matches!(
        jumps::inside_cell(0.5, 0., 1., f64::MAX, f64::MAX, f64::MAX, true),
        Err(Error::Exhausted(_))
    ));
}
