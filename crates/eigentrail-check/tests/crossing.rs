use eigentrail_core::{resources::Cancellation, *};
fn fixture(p: usize) -> TraceRequest {
    let times = vec![-0.01, 0., 0.01];
    let covariances = times
        .iter()
        .map(|&t| {
            let mut values = vec![5. - t, 5., 5. + t];
            if p == 4 {
                values.push(1.);
            }
            let mut data = vec![0.; p * p];
            for i in 0..p {
                data[i + i * p] = values[i];
            }
            if p == 4 {
                let (s, c) = (0.001_f64 * t).sin_cos();
                data[0] = c * c * values[0] + s * s;
                data[3 + 3 * p] = s * s * values[0] + c * c;
                data[3] = (values[0] - 1.) * s * c;
                data[3 * p] = data[3];
            }
            Matrix { n: p, data }
        })
        .collect();
    TraceRequest::conditional(times, covariances, vec![1e-12; 3], 1.01)
}
#[test]
fn et06_general_multiplicity_moving_plane_and_full_space() {
    for p in [3, 4] {
        let request = fixture(p);
        let window = crossings::CrossingWindow {
            block: [0, 3],
            centre: 1,
            secant: [0, 2],
            attachments: [0, 2],
            bracket: [-1e-5, 1e-5],
            curvature: 0.005,
            exact_collision_assumed: true,
        };
        let report = crossings::reconnect(&request, &window, &Cancellation::default()).unwrap();
        assert!(report.budget.branch_error < 0.01);
        assert_eq!(report.left, vec![0, 1, 2]);
        assert_eq!(report.right, vec![2, 1, 0]);
        assert!(report.timing.lower <= 0. && report.timing.upper >= 0.);
        assert!(report.slopes[0] < report.slopes[1] && report.slopes[1] < report.slopes[2]);
    }
}
#[test]
fn et05_pair_collision_and_attachment_must_straddle() {
    let request = TraceRequest::conditional(
        vec![-0.01, 0., 0.01],
        [-0.01, 0., 0.01]
            .iter()
            .map(|&t| Matrix {
                n: 2,
                data: vec![2. - t, 0., 0., 2. + t],
            })
            .collect(),
        vec![1e-12; 3],
        1.,
    );
    let window = crossings::CrossingWindow {
        block: [0, 2],
        centre: 1,
        secant: [0, 2],
        attachments: [0, 2],
        bracket: [-1e-5, 1e-5],
        curvature: 0.,
        exact_collision_assumed: true,
    };
    let r = crossings::reconnect(&request, &window, &Cancellation::default()).unwrap();
    assert_eq!(r.left, vec![0, 1]);
    assert_eq!(r.right, vec![1, 0]);
    assert!(r.budget.branch_error < 1e-7);
    let bad = crossings::CrossingWindow {
        attachments: [1, 2],
        ..window
    };
    assert!(crossings::reconnect(&request, &bad, &Cancellation::default()).is_err());
}
