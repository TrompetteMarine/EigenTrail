use eigentrail_core::{resources::Cancellation, scalar::Interval, *};
#[test]
fn quantitative_general_direct_rotation_and_block_intertwining() {
    let n = 32768;
    let times = (0..=n).map(|i| i as f64 / n as f64).collect::<Vec<_>>();
    let nodes = times
        .iter()
        .map(|t| {
            let (s, c) = (std::f64::consts::PI * t).sin_cos();
            Eigensystem {
                values: vec![1., 1., 0.],
                vectors: Matrix {
                    n: 3,
                    data: vec![c, 0., s, 0., 1., 0., -s, 0., c],
                },
                backward: 0.,
                polar: 0.,
            }
        })
        .collect::<Vec<_>>();
    for (&t, node) in times.iter().zip(&nodes) {
        use eigentrail_core::scalar::*;
        let angle = Interval::PI * point(t);
        let c = angle.cos();
        let s = angle.sin();
        let exact = [
            c,
            point(0.),
            s,
            point(0.),
            point(1.),
            point(0.),
            -s,
            point(0.),
            c,
        ];
        let norm = node
            .vectors
            .data
            .iter()
            .zip(exact)
            .fold(point(0.), |total, (&v, u)| total + square(point(v) - u))
            .sqrt();
        // Weyl gives ||polar(V)-V||<=||V-R||, hence projector error <=4||V-R||.
        assert!((point(4.) * norm).sup() <= 1e-10);
    }
    let result = transport::transport_blocks(
        &times,
        &nodes,
        [0, 2],
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
    assert!(result.product_error < 0.001 && result.comparison_error < 0.01);
    assert!((result.rotation.at(0, 0) + 1.).abs() < 1e-8);
    assert!((result.rotation.at(1, 1) - 1.).abs() < 1e-8);
    assert!((result.rotation.at(2, 2) + 1.).abs() < 1e-8);
    for i in 0..3 {
        for j in 0..3 {
            let gram = (0..3)
                .map(|k| result.rotation.at(k, i) * result.rotation.at(k, j))
                .sum::<f64>();
            assert!((gram - f64::from(i == j)).abs() < 1e-8);
        }
    }
}

#[test]
fn fixed_block_transport_does_not_follow_its_rotating_internal_eigenlines() {
    let n = 128;
    let times = (0..=n).map(|i| i as f64 / n as f64).collect::<Vec<_>>();
    let nodes = times
        .iter()
        .map(|t| {
            let (s, c) = (std::f64::consts::PI * t).sin_cos();
            Eigensystem {
                values: vec![0.8, 0.2],
                vectors: Matrix {
                    n: 2,
                    data: vec![c, s, -s, c],
                },
                backward: 0.,
                polar: 0.,
            }
        })
        .collect::<Vec<_>>();
    let contract = transport::TransportContract {
        reference_speed: 0.,
        max_speed: 1e-6,
        endpoint_error: 1e-14,
        initial_mismatch: 0.,
        target: 1e-6,
    };
    let result =
        transport::transport_blocks(&times, &nodes, [0, 2], &contract, &Cancellation::default())
            .unwrap();
    assert!(result.comparison_error < 1e-6);
    assert!(
        (result.rotation.at(0, 0) - 1.).abs() < 1e-10 && result.rotation.at(1, 0).abs() < 1e-10
    );
    // The covariance is not diagonal in the stationary transported coordinates at t=1/4.
    let angle = std::f64::consts::PI / 4.;
    assert!(((0.8 - 0.2) * angle.sin() * angle.cos()).abs() > 0.1);
}
