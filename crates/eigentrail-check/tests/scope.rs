use eigentrail_core::{resources::Cancellation, *};
fn matrix(a: f64, b: f64) -> Matrix {
    Matrix {
        n: 2,
        data: vec![a, 0., 0., b],
    }
}
#[test]
fn common_frame_crossing_exchanges_ranks_and_requires_structural_premise() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![matrix(0.8, 0.2), matrix(0.2, 0.8)],
        vec![1e-10; 2],
        0.6,
    );
    let r =
        common_frame::common_frame(&request, &[1., 0.], true, &Cancellation::default()).unwrap();
    assert_eq!(r.possible_ranks[0][0], [1, 1]);
    assert_eq!(r.possible_ranks[1][0], [2, 2]);
    assert_eq!(r.possible_ranks[1][1], [1, 1]);
    assert!(
        common_frame::common_frame(&request, &[1., 0.], false, &Cancellation::default()).is_err()
    );
    assert!(
        common_frame::common_frame(&request, &[0.5, 0.5], true, &Cancellation::default()).is_err(),
        "tied anchor cannot invent labels"
    );
}
#[test]
fn calibration_or_release_status_cannot_be_forged() {
    let request =
        TraceRequest::conditional(vec![0., 1.], vec![matrix(0.8, 0.2); 2], vec![1e-6; 2], 0.);
    let report = track(&request).unwrap();
    let mut bad = report.clone();
    bad.calibration_scope = "bootstrap finite-sample certified".into();
    assert!(eigentrail_check::check(&request, &bad).is_err());
    let mut bad = report;
    bad.resource_qualification = "qualified".into();
    assert!(eigentrail_check::check(&request, &bad).is_err());
}
#[test]
fn fixed_frame_permutation_sign_and_nonorthogonal_scaling() {
    let request =
        TraceRequest::conditional(vec![0., 1.], vec![matrix(0.8, 0.2); 2], vec![1e-6; 2], 0.);
    let report = track(&request).unwrap();
    let mut permuted = request.clone();
    permuted.covariances = vec![matrix(0.2, 0.8); 2];
    let other = track(&permuted).unwrap();
    assert_eq!(report.summaries, other.summaries);
    assert_eq!(report.segments, other.segments);
    let mut signed = report.clone();
    for e in &mut signed.eigensystems {
        for i in 0..2 {
            e.vectors.data[i] *= -1.;
        }
    }
    eigentrail_check::check(&request, &signed).unwrap();
    let mut scaled = request;
    scaled.covariances = vec![matrix(0.8, 20.); 2];
    let other = track(&scaled).unwrap();
    assert!(
        other.summaries[0][0].strength.lower > report.summaries[0][0].strength.upper,
        "nonorthogonal feature scaling changes PCA"
    );
}

#[test]
fn et43_uniform_accuracy_requires_every_configuration() {
    let a = TraceRequest::conditional(vec![0., 1.], vec![matrix(0.8, 0.2); 2], vec![1e-6; 2], 0.);
    let mut b = a.clone();
    b.lipschitz = 1.;
    let ra = track(&a).unwrap();
    let rb = track(&b).unwrap();
    eigentrail_check::check_uniform_accuracy(
        std::slice::from_ref(&a),
        std::slice::from_ref(&ra),
        [0, 1],
        0.002,
    )
    .unwrap();
    assert!(
        eigentrail_check::check_uniform_accuracy(
            &[a.clone(), b.clone()],
            std::slice::from_ref(&ra),
            [0, 1],
            0.002
        )
        .is_err()
    );
    assert!(eigentrail_check::check_uniform_accuracy(&[a, b], &[ra, rb], [0, 1], 0.002).is_err());
}
