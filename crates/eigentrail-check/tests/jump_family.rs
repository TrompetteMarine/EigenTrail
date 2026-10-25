use eigentrail_core::{resources::Cancellation, *};
fn request() -> TraceRequest {
    TraceRequest::conditional(
        vec![0., 1., 2.],
        vec![
            Matrix {
                n: 2,
                data: vec![1., 0., 0., 0.5],
            },
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 0.5],
            },
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 0.5],
            },
        ],
        vec![0.001; 3],
        0.01,
    )
}
#[test]
fn et29_minimum_signal_must_constrain_all_retained_candidates() {
    let r = request();
    let model = jumps::JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: Some(1.),
        minimum_applies_to_all_candidates: false,
    };
    let a = jumps::analyze(&r, &model, &Cancellation::default()).unwrap();
    assert_eq!(a.mandatory, vec![true, false]);
    assert_eq!(a.optional, vec![false, true]);
    let model = jumps::JumpModel {
        minimum_applies_to_all_candidates: true,
        ..model
    };
    let b = jumps::analyze(&r, &model, &Cancellation::default()).unwrap();
    assert_eq!(b.optional, vec![false, false]);
    assert!(b.cells[0].amplitude.lower <= 1. && b.cells[0].amplitude.upper >= 1.);
    assert_eq!(
        jumps::inside_cell(0.5, 0., 1., 0.001, 0.001, 0.01, true)
            .unwrap()
            .combination,
        "union of PSD balls"
    );
    assert_eq!(
        jumps::inside_cell(0.5, 0., 1., 0.001, 0.001, 0.01, false)
            .unwrap()
            .combination,
        "intersection of PSD balls"
    );
}
#[test]
fn et28_outside_spacing_assumption_is_not_claimed_detectable() {
    let model = jumps::JumpModel {
        at_most_one_per_cell: false,
        minimum_jump: None,
        minimum_applies_to_all_candidates: false,
    };
    assert!(jumps::analyze(&request(), &model, &Cancellation::default()).is_err());
}

#[test]
fn archived_positive_jump_and_unknown_timing_constructions() {
    let request = TraceRequest::conditional(
        vec![0., 1., 2.],
        vec![2., 3.1, 3.2]
            .into_iter()
            .map(|a| Matrix {
                n: 2,
                data: vec![a, 0., 0., 1.],
            })
            .collect(),
        vec![0.05; 3],
        0.1,
    );
    let model = jumps::JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: Some(1.),
        minimum_applies_to_all_candidates: true,
    };
    let family = jumps::analyze(&request, &model, &Cancellation::default()).unwrap();
    assert_eq!(family.mandatory, vec![true, false]);
    assert_eq!(family.optional, vec![false, false]);
    assert!(
        family.cells[0].amplitude.lower <= 0.9 + 1e-14
            && family.cells[0].amplitude.upper >= 1.3 - 1e-14
    );
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 1.],
            },
            Matrix {
                n: 2,
                data: vec![3., 0., 0., 1.],
            },
        ],
        vec![0.; 2],
        0.,
    );
    let family = jumps::analyze(&request, &model, &Cancellation::default()).unwrap();
    assert!(family.cells[0].flagged);
    assert_eq!(
        family.cells[0].timing,
        Range {
            lower: 0.,
            upper: 1.,
            lower_closed: false,
            upper_closed: true
        }
    );
    let state = jumps::inside_cell(0.5, 0., 1., 0., 0., 0., true).unwrap();
    assert_eq!(state.combination, "union of PSD balls");
    // Jumps at 1/3 and 2/3 have the same observations and different interior states.
    assert_eq!(state.left_radius, 0.);
    assert_eq!(state.right_radius, 0.);
}
#[test]
fn archived_strict_power_boundary_and_smooth_rotation() {
    let lo = 0.2_f64.next_down();
    let hi = 0.2_f64.next_up();
    assert!(
        !jumps::cell(0., 1., lo, hi - lo, 0.05, 0.05, 0.1)
            .unwrap()
            .flagged
    );
    assert!(!jumps::minimum_signal_power(
        0.4,
        0.1,
        1.,
        0.05,
        0.05,
        hi - lo
    ));
    let request = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![2., 0., 0., 1.],
            },
            Matrix {
                n: 2,
                data: vec![2., 0.25, 0.25, 1.],
            },
        ],
        vec![0.; 2],
        0.25,
    );
    let model = jumps::JumpModel {
        at_most_one_per_cell: true,
        minimum_jump: None,
        minimum_applies_to_all_candidates: false,
    };
    let family = jumps::analyze(&request, &model, &Cancellation::default()).unwrap();
    assert!(!family.cells[0].flagged && family.optional[0]);
}
