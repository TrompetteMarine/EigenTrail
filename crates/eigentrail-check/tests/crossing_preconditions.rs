//! Source Appendix B requires the entire collision bracket inside the secant.
use eigentrail_core::{resources::Cancellation, *};

fn request(times: Vec<f64>) -> TraceRequest {
    let matrices = times
        .iter()
        .map(|&t| Matrix {
            n: 2,
            data: vec![2. - t, 0., 0., 2. + t],
        })
        .collect();
    let count = times.len();
    TraceRequest::conditional(times, matrices, vec![0.; count], 1.)
}

#[test]
fn a_one_sided_secant_cannot_use_the_collision_derivative_bound() {
    let input = request(vec![-1. / 16., 0., 1. / 32., 1. / 16.]);
    let window = crossings::CrossingWindow {
        block: [0, 2],
        centre: 1,
        secant: [2, 3],
        attachments: [0, 3],
        bracket: [-1. / 4096., 1. / 4096.],
        curvature: 0.,
        exact_collision_assumed: true,
    };
    assert!(
        matches!(
            crossings::reconnect(&input, &window, &Cancellation::default()),
            Err(Error::Invalid(_))
        ),
        "one-sided secant accepted despite an unproved Mh/2 comparison"
    );
}

#[test]
fn closed_secant_boundaries_and_strict_attachments_are_distinct() {
    let input = request(vec![-1. / 16., -1. / 32., 0., 1. / 32., 1. / 16.]);
    let window = crossings::CrossingWindow {
        block: [0, 2],
        centre: 2,
        secant: [1, 3],
        attachments: [0, 4],
        bracket: [-1. / 32., 1. / 32.],
        curvature: 0.,
        exact_collision_assumed: true,
    };
    let report = crossings::reconnect(&input, &window, &Cancellation::default()).unwrap();
    assert!(report.timing.lower <= 0. && report.timing.upper >= 0.);
    let touching = crossings::CrossingWindow {
        attachments: [1, 4],
        ..window
    };
    assert!(matches!(
        crossings::reconnect(&input, &touching, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
}
