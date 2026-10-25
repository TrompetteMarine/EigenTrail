use eigentrail_core::{resources::Cancellation, *};
fn frame(c: f64, s: f64) -> Eigensystem {
    Eigensystem {
        values: vec![1., 0.],
        vectors: Matrix {
            n: 2,
            data: vec![c, s, -s, c],
        },
        backward: 0.,
        polar: 0.,
    }
}
#[test]
fn rational_direct_rotation_identity_reverse_and_perturbed_frames() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/polar.json")).unwrap();
    assert_eq!(oracle["rotation"][0][0], "3/5");
    let a = frame(1., 0.);
    let b = frame(0.6, 0.8);
    let cancel = Cancellation::default();
    for (first, last, sign) in [(&a, &b, 1.), (&b, &a, -1.)] {
        let (r, error, angle) = transport::direct_rotation(first, last, 0, 1, &cancel).unwrap();
        let expected = [0.6, sign * 0.8, -sign * 0.8, 0.6];
        assert!(
            r.data
                .iter()
                .zip(expected)
                .all(|(&x, y)| (x - y).abs() < 1e-12)
        );
        assert!(error.is_finite() && angle > 0.9 && angle < 1.);
    }
    let (identity, error, _) = transport::direct_rotation(&a, &a, 0, 1, &cancel).unwrap();
    assert_eq!(identity.data, vec![1., 0., 0., 1.]);
    assert!(error < 1e-12);
    let mut noisy = b;
    for x in &mut noisy.vectors.data {
        *x *= 1. + 1e-9;
    }
    let (r, error, _) = transport::direct_rotation(&a, &noisy, 0, 1, &cancel).unwrap();
    assert!(error > 1e-9 && r.data.iter().all(|x| x.is_finite()));
    assert!(transport::direct_rotation(&a, &frame(0., 1.), 0, 1, &cancel).is_err());
}
