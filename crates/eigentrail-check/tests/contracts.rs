use eigentrail_check::check;
use eigentrail_core::{resources::Cancellation, scalar::*, *};

fn diagonal(values: &[f64]) -> Matrix {
    let n = values.len();
    let mut data = vec![0.; n * n];
    for i in 0..n {
        data[i + i * n] = values[i];
    }
    Matrix { n, data }
}
fn rotating(t: f64) -> Matrix {
    let (s, c) = (0.1 * t).sin_cos();
    let a = 0.8 + 0.01 * t;
    let b = 0.2 - 0.01 * t;
    Matrix {
        n: 2,
        data: vec![
            a * c * c + b * s * s,
            (a - b) * s * c,
            (a - b) * s * c,
            a * s * s + b * c * c,
        ],
    }
}
fn rotation_request() -> TraceRequest {
    let times = (0..101).map(|i| i as f64 / 100.).collect::<Vec<_>>();
    TraceRequest::conditional(
        times.clone(),
        times.iter().map(|&t| rotating(t)).collect(),
        vec![1e-6; 101],
        0.072,
    )
}

#[test]
fn et03_separate_informative_deterministic_and_gaussian_contracts() {
    let mut req = rotation_request();
    let report = track(&req).unwrap();
    check(&req, &report).unwrap();
    assert!(
        report
            .segments
            .iter()
            .all(|s| s.association == Association::Unique(vec![0, 1]))
    );
    let displacement = association::displacement(
        &report.eigensystems[0],
        &report.eigensystems[100],
        0,
        1,
        report.segments[0].tubes[0].bound,
        report.segments[99].tubes[0].bound,
    )
    .unwrap();
    assert!(displacement.lower > 0.05);
    for (j, share) in [(0, 0.8), (100, 0.81)] {
        let Quantity::Interval(range) = &report.summaries[j][0].share else {
            panic!()
        };
        assert!(range.lower <= share && range.upper >= share);
    }
    for (j, segment) in report.segments.iter().enumerate() {
        for tube in &segment.tubes {
            assert!(
                budget::numerical_component(&req, &report.eigensystems, j, tube).unwrap()
                    <= req.numerical_allowance
            );
        }
    }
    assert!(report.segments.iter().all(|s| {
        s.tubes.len() == 2
            && s.tubes
                .iter()
                .all(|t| t.end - t.start == 1 && t.informative && t.bound < 0.002)
    }));
    let deterministic = report.segments[0].tubes[0].bound;
    req.calibration = Calibration::Gaussian {
        observations: 100000000,
        estimated_mean: false,
        divisor: 100000000.,
        alpha: 0.05,
        auxiliary_risk: 0.,
    };
    req.target = 0.005;
    let report = track(&req).unwrap();
    check(&req, &report).unwrap();
    assert!(report.segments.iter().all(|s|s.tubes.len()==2 && s.tubes.iter().all(|t|t.informative && t.bound<0.005)));
    assert!(report.segments[0].tubes[0].bound > deterministic);
    for (j, segment) in report.segments.iter().enumerate() {
        for tube in &segment.tubes {
            assert!(
                budget::numerical_component(&req, &report.eigensystems, j, tube).unwrap()
                    <= req.numerical_allowance
            );
        }
    }
    assert!(
        calibration::scale_envelope(2, 101, 100000000, 0.05, 0.81).unwrap() < 0.00089149930213542
    );
    let mut forged = report.clone();
    forged.segments[0].tubes[0].bound = 0.002;
    assert!(
        check(&req, &forged).is_err(),
        "stochastic case cannot assert deterministic accuracy"
    );
}

#[test]
fn et01_et02_et23_et24_strength_share_and_tie_ranks() {
    let req = TraceRequest::conditional(
        vec![0., 1.],
        vec![diagonal(&[0.8, 0.2]), diagonal(&[0.81, 0.19])],
        vec![1e-12; 2],
        0.02,
    );
    let rep = track(&req).unwrap();
    check(&req, &rep).unwrap();
    assert_eq!(rep.segments[0].association, Association::Unique(vec![0, 1]));
    let req = TraceRequest::conditional(
        vec![0., 1.],
        vec![diagonal(&[8., 2.]), diagonal(&[7., 1.])],
        vec![1e-12; 2],
        2.,
    );
    let rep = track(&req).unwrap();
    check(&req, &rep).unwrap();
    assert!(rep.summaries[1][0].strength.upper < rep.summaries[0][0].strength.lower);
    let (Quantity::Interval(a), Quantity::Interval(b)) =
        (&rep.summaries[0][0].share, &rep.summaries[1][0].share)
    else {
        panic!()
    };
    assert!(a.upper < b.lower);
    let req =
        TraceRequest::conditional(vec![0., 1.], vec![diagonal(&[0., 0.]); 2], vec![0.; 2], 0.);
    let rep = track(&req).unwrap();
    check(&req, &rep).unwrap();
    assert!(matches!(rep.summaries[0][0].share, Quantity::Undefined(_)));
    assert_eq!(rep.summaries[0][0].rank, [1, 2]);
    assert_eq!(
        rep.segments[0].tubes.len(),
        1,
        "no invented repeated-eigenvalue basis"
    );
}

#[test]
fn et10_et11_et12_all_matching_graphs_agree_with_independent_enumeration() {
    fn permutations(v: &mut [usize], i: usize, g: &[Vec<usize>], count: &mut usize) {
        if i == v.len() {
            if v.iter().enumerate().all(|(i, j)| g[i].contains(j)) {
                *count += 1;
            }
            return;
        }
        for j in i..v.len() {
            v.swap(i, j);
            permutations(v, i + 1, g, count);
            v.swap(i, j);
        }
    }
    for mask in 0..512usize {
        let g = (0..3)
            .map(|i| (0..3).filter(|j| mask & (1 << (3 * i + j)) != 0).collect())
            .collect::<Vec<Vec<_>>>();
        let mut count = 0;
        permutations(&mut [0, 1, 2], 0, &g, &mut count);
        let a = association::classify(&g);
        let b = eigentrail_check::association::classify(&g);
        assert_eq!(a, b);
        match count {
            0 => assert_eq!(a, Association::NoPerfectMatching),
            1 => assert!(matches!(a, Association::Unique(_))),
            _ => assert_eq!(a, Association::Alternatives),
        }
    }
}

#[test]
fn et12_et14_et36_checker_rejects_omitted_vertices_edges_and_wrong_branches() {
    let req = rotation_request();
    let report = track(&req).unwrap();
    let mut bad = report.clone();
    bad.segments[0].tubes.pop();
    assert!(check(&req, &bad).is_err());
    let mut bad = report.clone();
    bad.segments[0].edges[0].clear();
    assert!(check(&req, &bad).is_err());
    let mut bad = report.clone();
    bad.eigensystems[0].values.swap(0, 1);
    assert!(check(&req, &bad).is_err());
    let mut bad = report.clone();
    bad.eigensystems[0].backward = 0.;
    assert!(check(&req, &bad).is_err());
    let mut bad = report.clone();
    bad.radii[0] = 0.;
    assert!(check(&req, &bad).is_err());
    let mut bad = report.clone();
    bad.summaries[0][0].rank = [2, 2];
    assert!(check(&req, &bad).is_err());
}

#[test]
fn et18_et21_et22_et44_calibration_divisor_gates_corrections_and_event_budget() {
    assert_eq!(calibration::degrees(1001, true).unwrap(), 1000);
    assert!(calibration::degrees(1, true).is_err());
    assert!(calibration::multiplier(80, 1, point(0.05)).is_err());
    let mut req = rotation_request();
    req.calibration = Calibration::Gaussian {
        observations: 100000001,
        estimated_mean: true,
        divisor: 50000000.,
        alpha: 0.05,
        auxiliary_risk: 0.01,
    };
    req.correction = vec![1e-6; 101];
    let report = track(&req).unwrap();
    check(&req, &report).unwrap();
    assert!(
        report.summaries[0][0].strength.upper < 0.5,
        "stored divisor must normalize covariance"
    );
    req.calibration = Calibration::Gaussian {
        observations: 100000001,
        estimated_mean: true,
        divisor: 50000000.,
        alpha: 0.05,
        auxiliary_risk: 0.05,
    };
    assert!(track(&req).is_err());
}

#[test]
fn et16_et17_whole_chain_and_widening_are_separate() {
    let mut req = TraceRequest::conditional(
        vec![0., 1.],
        vec![diagonal(&[1., 0.]), diagonal(&[2., 0.])],
        vec![0.1; 2],
        0.,
    );
    let cancel = Cancellation::default();
    assert_eq!(
        histories::witness(
            &req,
            &req.covariances,
            &[0.1, 0.1],
            &req.covariances,
            0.,
            &cancel
        )
        .unwrap(),
        histories::Witness::Unverified
    );
    req.lipschitz = 2.;
    let outside = vec![diagonal(&[1.2, 0.]), diagonal(&[2.2, 0.])];
    assert_eq!(
        histories::witness(&req, &req.covariances, &[0.1, 0.1], &outside, 0., &cancel).unwrap(),
        histories::Witness::Unverified
    );
    assert!(matches!(
        histories::witness(&req, &req.covariances, &[0.1, 0.1], &outside, 0.2, &cancel).unwrap(),
        histories::Witness::Widened { .. }
    ));
}

#[test]
fn et26_et27_et28_jump_flags_do_not_erase_unflagged_alternatives() {
    let j = jumps::cell(0., 1., 1., 1e-12, 0.01, 0.01, 0.1).unwrap();
    assert!(j.flagged);
    assert!(!j.timing.lower_closed && j.timing.upper_closed);
    assert!(j.amplitude.lower <= 0.88 && j.amplitude.upper >= 1.12);
    let j = jumps::cell(0., 1., 0.1, 0., 0.01, 0.01, 0.1).unwrap();
    assert!(!j.flagged);
    assert!(j.alternative.contains("hidden-jump"));
    assert!(jumps::minimum_signal_power(1., 0.1, 1., 0.01, 0.01, 1e-12));
}

#[test]
fn et30_et31_quantitative_line_transport_and_sign_holonomy() {
    let n = 32768;
    let times = (0..=n).map(|i| i as f64 / n as f64).collect::<Vec<_>>();
    let nodes = times
        .iter()
        .map(|t| {
            let (s, c) = (std::f64::consts::PI * t).sin_cos();
            [c, s]
        })
        .collect::<Vec<_>>();
    for (&t, node) in times.iter().zip(&nodes) {
        let angle = Interval::PI * point(t);
        let norm = (square(point(node[0])) + square(point(node[1]))).sqrt();
        let error = (square(point(node[0]) / norm - angle.cos())
            + square(point(node[1]) / norm - angle.sin()))
        .sqrt();
        assert!(
            (point(2.) * error).sup() <= 1e-10,
            "independent exact-projector endpoint oracle"
        );
    }
    let result = transport::transport_lines(
        &times,
        &nodes,
        &transport::TransportContract {
            reference_speed: Interval::PI.sup(),
            max_speed: Interval::PI.sup() + 1e-4,
            endpoint_error: 1e-10,
            initial_mismatch: 1e-8,
            target: 0.01,
        },
        &Cancellation::default(),
    )
    .unwrap();
    assert!(result.comparison_error < 0.005 && result.product_error < 0.001);
    let r = result.rotation;
    assert!((r[0][0] + 1.).abs() < 1e-8 && (r[1][1] + 1.).abs() < 1e-8);
    assert!(r[0][1].abs() < 1e-8 && r[1][0].abs() < 1e-8);
    let norm = r[0][0] * r[0][0] + r[1][0] * r[1][0];
    assert!((norm - 1.).abs() < 1e-8);
    // Initial line gauge -1 changes transported vector sign while leaving the operator fixed.
    assert!((-r[0][0] - 1.).abs() < 1e-8);
}

#[test]
fn et32_et33_et42_transport_speed_cut_locus_refinement_and_cancellation() {
    let cancel = Cancellation::default();
    let c = transport::TransportContract {
        reference_speed: 2.,
        max_speed: 2.,
        endpoint_error: 0.,
        initial_mismatch: 0.,
        target: 1.,
    };
    assert!(transport::transport_lines(&[0., 1.], &[[1., 0.], [0., 1.]], &c, &cancel).is_err());
    assert!(
        transport::transport_lines(
            &[0., 1e-5],
            &[[1., 0.], [0.99, 0.1]],
            &transport::TransportContract {
                reference_speed: 1.,
                max_speed: 1.,
                ..c
            },
            &cancel
        )
        .is_err()
    );
    let times = (0..65538).map(|i| i as f64 / 65537.).collect::<Vec<_>>();
    let nodes = vec![[1., 0.]; 65538];
    assert!(matches!(
        transport::transport_lines(&times, &nodes, &c, &cancel),
        Err(Error::Exhausted(_))
    ));
    cancel.cancel();
    assert_eq!(
        track_with_cancel(&rotation_request(), &cancel),
        Err(Error::Cancelled)
    );
    assert!(resources::preflight(256, 10000, resources::DEFAULT_BYTES).is_err());
    assert!(resources::preflight(usize::MAX, usize::MAX, resources::DEFAULT_BYTES).is_err());
}

#[test]
fn et39_et40_serialization_tampering_and_dependency_invalidation() {
    let req = rotation_request();
    let bytes = serialization::encode(&req).unwrap();
    let back: TraceRequest = serialization::decode(&bytes).unwrap();
    assert_eq!(req, back);
    assert!(serialization::decode::<TraceRequest>(b"{\"schema\":\"a\",\"schema\":\"b\"}").is_err());
    let mut bad = req.clone();
    bad.times[0] = f64::NAN;
    assert!(serialization::encode(&bad).is_err());
    use std::collections::{BTreeMap, BTreeSet};
    let edges = BTreeMap::from([
        ("ET:execution".into(), vec!["lock".into()]),
        ("ET:release".into(), vec!["ET:execution".into()]),
        ("theorem".into(), vec![]),
    ]);
    let stale = eigentrail_check::evidence::invalidate(&BTreeSet::from(["lock".into()]), &edges);
    assert!(stale.contains("ET:release") && !stale.contains("theorem"));
}

#[test]
fn et36_strict_interval_gates_and_exact_collision_premise() {
    assert!(!jumps::cell(0., 1., 0.1, 0., 0., 0., 0.1).unwrap().flagged);
    let x = crossings::CollisionInput {
        full_space: true,
        collision_assumed: true,
        k: 1.,
        curvature: 0.,
        centre_radius: 1e-12,
        centre_width: 1e-5,
        external_gap: 0.,
        secant_width: 0.02,
        endpoint_radii: [1e-12; 2],
        derivative_gaps: &[1., 1.],
        window_radius: 0.01,
    };
    let b = crossings::transverse(&x).unwrap();
    assert!(b.branch_error < 1e-8);
    assert!(crossings::attachment(b.branch_error, 1e-8, true));
    assert!(!crossings::attachment(0., 0., false));
    assert!(!crossings::attachment(0.25, 0.25, true));
    let x = crossings::CollisionInput {
        collision_assumed: false,
        ..x
    };
    assert!(crossings::transverse(&x).is_err());
}
