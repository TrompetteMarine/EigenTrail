use eigentrail_core::{resources::Cancellation, *};
#[test]
fn et09_identical_endpoints_do_not_identify_the_interior() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![1., 0., 0., 0.]
            };
            2
        ],
        vec![0.; 2],
        7.,
    );
    let report = track(&request).unwrap();
    eigentrail_check::check(&request, &report).unwrap();
    assert_eq!(report.segments[0].tubes.len(), 1);
    assert_eq!(report.segments[0].tubes[0].end, 2);
    // Constant and pi-turn projector paths have the same nodes; both satisfy K=7.
    assert!(std::f64::consts::PI < request.lipschitz);
}
#[test]
fn et15_initialized_line_cannot_silently_project_an_incompatible_vector() {
    let a = transport::initialize_line([1., 0.], [1., 0.], 1e-8).unwrap();
    assert!(a.mismatch_upper < 1e-8);
    let b = transport::initialize_line([1., 0.], [-1., 0.], 1e-8).unwrap();
    assert_eq!(b.compatible_vector, [-1., 0.]);
    assert!(transport::initialize_line([1., 0.], [0., 1.], 1e-8).is_err());
    assert!(transport::initialize_line([1., 0.], [0.999, 0.001], 1e-8).is_err());
}
#[test]
fn et25_local_and_reference_displacements_are_distinct() {
    fn eigen(angle: f64) -> Eigensystem {
        let (s, c) = angle.sin_cos();
        Eigensystem {
            values: vec![1., 0.],
            vectors: Matrix {
                n: 2,
                data: vec![c, s, -s, c],
            },
            backward: 0.,
            polar: 1e-15,
        }
    }
    let first = eigen(0.);
    let previous = eigen(11. * std::f64::consts::PI / 24.);
    let last = eigen(std::f64::consts::PI / 2.);
    let local = association::displacement(&previous, &last, 0, 1, 1e-6, 1e-6).unwrap();
    let reference = association::displacement(&first, &last, 0, 1, 1e-6, 1e-6).unwrap();
    assert!(local.upper < 0.2 && reference.lower > 0.99);
}
#[test]
fn et19_unsupported_sampling_law_needs_an_explicit_conditional_envelope() {
    let input = br#"{"kind":"dependent_rows","radii":[]}"#;
    assert!(serialization::decode::<Calibration>(input).is_err());
}

#[test]
fn canonical_float_bits_do_not_consume_literal_feature_names() {
    let mut r = TraceRequest::conditional(
        vec![-0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    );
    r.features = vec!["f64:0000000000000000".into()];
    if let Calibration::Conditional { assumptions, .. } = &mut r.calibration {
        *assumptions = "str:declared exact envelope".into();
    }
    let bytes = serialization::encode(&r).unwrap();
    let decoded: TraceRequest = serialization::decode(&bytes).unwrap();
    assert_eq!(decoded, r);
    assert_eq!(decoded.times[0].to_bits(), (-0f64).to_bits());
}
#[test]
fn malformed_overflowing_and_nonfinite_inputs_return_errors() {
    let mut request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![1., 0., 0., 1.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    );
    request.covariances[0].data[1] = 0.01;
    assert!(track(&request).is_err());
    request.covariances[0].data[1] = 0.;
    request.times = vec![-f64::MAX, f64::MAX];
    assert!(track(&request).is_err());
    request.times = vec![0., 1.];
    request.calibration = Calibration::Gaussian {
        observations: 100000000,
        estimated_mean: false,
        divisor: f64::MAX,
        alpha: 0.05,
        auxiliary_risk: 0.,
    };
    request.covariances[0].data[0] = f64::MAX;
    assert!(track(&request).is_err());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(track_with_cancel(&request, &cancel), Err(Error::Cancelled));
}

#[test]
fn et13_endpoint_residuals_cannot_certify_an_unchecked_interior() {
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![0.8, 0., 0., 0.2]
            };
            2
        ],
        vec![1e-6; 2],
        0.,
    );
    let mut evaluation = curves::evaluate(&request, 0, 0.5, &Cancellation::default()).unwrap();
    eigentrail_check::curves::check_evaluation(&request, &evaluation, &Cancellation::default())
        .unwrap();
    assert!(evaluation.evaluation_error < 1e-12);
    evaluation.eigensystem.vectors.data = vec![0., 1., 1., 0.];
    assert!(
        eigentrail_check::curves::check_evaluation(&request, &evaluation, &Cancellation::default())
            .is_err()
    );
    assert!(curves::evaluate(&request, usize::MAX, 0.5, &Cancellation::default()).is_err());
}

#[test]
fn transport_refuses_oversized_owned_node_stacks_before_evaluation() {
    let times = (0..=65536).map(|i| i as f64 / 65536.).collect::<Vec<_>>();
    let nodes = vec![
        Eigensystem {
            values: vec![],
            vectors: Matrix {
                n: 256,
                data: vec![]
            },
            backward: 0.,
            polar: 0.
        };
        times.len()
    ];
    let contract = transport::TransportContract {
        reference_speed: 1.,
        max_speed: 1.,
        endpoint_error: 0.,
        initial_mismatch: 0.,
        target: 0.01,
    };
    assert!(matches!(
        transport::transport_blocks(&times, &nodes, [0, 1], &contract, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
}

#[test]
fn finite_but_nonorthogonal_overflowing_transport_proposals_are_refused() {
    let bad = Eigensystem {
        values: vec![1.],
        vectors: Matrix {
            n: 1,
            data: vec![f64::MAX],
        },
        backward: 0.,
        polar: 0.,
    };
    assert!(matches!(
        transport::direct_rotation(&bad, &bad, 0, 1, &Cancellation::default()),
        Err(Error::Exhausted(_))
    ));
}
