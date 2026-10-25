use eigentrail_core::{families::*, resources::Cancellation, *};
fn scalar(x: f64) -> Matrix {
    Matrix {
        n: 1,
        data: vec![x],
    }
}
fn configuration(id: &str, k: f64) -> Configuration {
    Configuration {
        id: id.into(),
        lipschitz: k,
        class: PathClass::Continuous,
        witnesses: Vec::new(),
    }
}
fn request() -> HistoryRequest {
    HistoryRequest {
        schema: eigentrail_core::families::SCHEMA.into(),
        observations: TraceRequest::conditional(
            vec![0., 1.],
            vec![scalar(0.), scalar(2.)],
            vec![0.; 2],
            0.,
        ),
        configurations: vec![configuration("slow", 1.), configuration("fast", 2.)],
        query_times: vec![0.5],
    }
}
fn produce(request: &HistoryRequest) -> HistoryReport {
    let cancel = Cancellation::default();
    let report = eigentrail_core::families::analyze(request, &cancel).unwrap();
    eigentrail_check::families::check(request, &report, &cancel).unwrap();
    report
}
#[test]
fn exact_joint_chain_configurations_and_query_grid() {
    let request = request();
    let report = produce(&request);
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/histories.json")).unwrap();
    assert_eq!(oracle["incompatible_K1"], "Unresolved");
    assert_eq!(
        report.configurations[0].feasibility,
        Feasibility::Unresolved
    );
    assert_eq!(
        report.configurations[1].feasibility,
        Feasibility::OriginalAffineWitness
    );
    for cfg in &report.configurations {
        assert_eq!(
            cfg.grid.iter().map(|n| n.time).collect::<Vec<_>>(),
            vec![0., 0.5, 1.]
        );
        assert_eq!(cfg.grid[1].queries, vec![0]);
        assert!(cfg.grid[1].observations.is_empty());
    }
    let bytes = serialization::encode(&report).unwrap();
    assert_eq!(
        serialization::decode::<HistoryReport>(&bytes).unwrap(),
        report
    );
    let cancel = Cancellation::default();
    let mut bad = report.clone();
    bad.configurations.remove(0);
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut bad = report.clone();
    bad.configurations[0].feasibility = Feasibility::OriginalAffineWitness;
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut bad = report.clone();
    bad.configurations[0].grid.remove(1);
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut bad = report.clone();
    bad.configurations[1].continuous_conditional.segments[0].edges[0].clear();
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
}
#[test]
fn widened_witness_is_scoped_and_does_not_widen_the_family() {
    let mut request = request();
    request.observations.covariances = vec![scalar(0.), scalar(1.)];
    request.observations.calibration = Calibration::Conditional {
        radii: vec![0.125; 2],
        assumptions: "rational deterministic envelope".into(),
    };
    request.configurations.truncate(1);
    request.configurations[0].witnesses = vec![AffineWitness {
        nodes: vec![scalar(0.25), scalar(1.25)],
        widening: 0.125,
    }];
    let report = produce(&request);
    assert_eq!(
        report.configurations[0].feasibility,
        Feasibility::WidenedAffineWitness { additional: 0.125 }
    );
    assert_eq!(
        report.configurations[0].continuous_conditional.radii,
        vec![0.125; 2]
    );
    let mut bad = report.clone();
    bad.configurations[0].feasibility = Feasibility::OriginalAffineWitness;
    assert!(eigentrail_check::families::check(&request, &bad, &Cancellation::default()).is_err());
    request.configurations[0].witnesses[0].widening = 0.;
    assert_eq!(
        produce(&request).configurations[0].feasibility,
        Feasibility::Unresolved
    );
}
fn jumps() -> HistoryRequest {
    let mut request = request();
    request.observations = TraceRequest::conditional(
        vec![0., 1., 2.],
        vec![2., 3.125, 3.25]
            .into_iter()
            .map(|x| Matrix {
                n: 2,
                data: vec![x, 0., 0., 1.],
            })
            .collect(),
        vec![0.0625; 3],
        0.125,
    );
    request.query_times = vec![0.5, 1.5];
    request.configurations = vec![false, true]
        .into_iter()
        .map(|minimum| Configuration {
            id: format!("candidate-minimum-{minimum}"),
            lipschitz: 0.125,
            class: PathClass::Jumps {
                model: eigentrail_core::jumps::JumpModel {
                    at_most_one_per_cell: true,
                    minimum_jump: Some(1.),
                    minimum_applies_to_all_candidates: minimum,
                },
            },
            witnesses: Vec::new(),
        })
        .collect();
    request
}
#[test]
fn checked_jump_masks_block_interpolation_and_identity_links() {
    let request = jumps();
    let report = produce(&request);
    assert_eq!(
        report.configurations[0].cells,
        vec![
            CellScope::JumpPossible {
                cell: 0,
                mandatory: true
            },
            CellScope::JumpPossible {
                cell: 1,
                mandatory: false
            }
        ]
    );
    assert_eq!(
        report.configurations[1].cells,
        vec![
            CellScope::JumpPossible {
                cell: 0,
                mandatory: true
            },
            CellScope::Smooth { segment: 1 }
        ]
    );
    assert!(
        report
            .configurations
            .iter()
            .all(|cfg| cfg.partition_overlaps.is_empty()
                && cfg.feasibility == Feasibility::Unresolved)
    );
    let cancel = Cancellation::default();
    let mut bad = report.clone();
    bad.configurations[0].cells[1] = CellScope::Smooth { segment: 1 };
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut bad = report.clone();
    bad.configurations[0].jumps[0].family.optional[1] = false;
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut bad = report.clone();
    bad.configurations[0].feasibility = Feasibility::OriginalAffineWitness;
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut unsupported = request.clone();
    unsupported.configurations[0].witnesses.push(AffineWitness {
        nodes: request.observations.covariances.clone(),
        widening: 0.,
    });
    assert_eq!(
        produce(&unsupported).configurations[0].feasibility,
        Feasibility::UnsupportedModel
    );
}
#[test]
fn all_partition_overlap_constraints_survive_block_merges() {
    fn diagonal(x: &[f64]) -> Matrix {
        let mut data = vec![0.; x.len() * x.len()];
        for (i, &v) in x.iter().enumerate() {
            data[i + i * x.len()] = v;
        }
        Matrix { n: x.len(), data }
    }
    let mut request = request();
    request.observations = TraceRequest::conditional(
        vec![0., 0.01, 1.01],
        vec![
            diagonal(&[5., 3., 1.]),
            diagonal(&[5., 3., 1.]),
            diagonal(&[5., 3., 2.95]),
        ],
        vec![0.; 3],
        2.,
    );
    request.query_times = vec![0.005, 0.5];
    request.configurations = vec![configuration("merging", 2.)];
    let report = produce(&request);
    let cfg = &report.configurations[0];
    assert_eq!(cfg.continuous_conditional.segments[0].tubes.len(), 3);
    assert_eq!(cfg.continuous_conditional.segments[1].tubes.len(), 1);
    assert_eq!(cfg.partition_overlaps.len(), 3);
    assert!(
        cfg.partition_overlaps
            .iter()
            .all(|o| o.date == 1 && o.right_block == 0 && o.rank == 1)
    );
    let mut bad = report.clone();
    bad.configurations[0].partition_overlaps.pop();
    assert!(eigentrail_check::families::check(&request, &bad, &Cancellation::default()).is_err());
}
#[test]
fn history_boundaries_and_cancellation_are_explicit() {
    let request = request();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        eigentrail_core::families::analyze(&request, &cancel),
        Err(Error::Cancelled)
    );
    let mut bad = request.clone();
    bad.configurations[1].id = bad.configurations[0].id.clone();
    assert!(validate_request(&bad).is_err());
    let mut bad = request.clone();
    bad.query_times = vec![0.; 10001];
    assert!(validate_request(&bad).is_err());
    let mut bad = request.clone();
    bad.query_times = vec![f64::NAN];
    assert!(validate_request(&bad).is_err());
    let mut bad = request.clone();
    bad.observations.memory_bytes = 1;
    assert!(matches!(validate_request(&bad), Err(Error::Resource(_))));
    let mut bad = request.clone();
    bad.configurations[0].witnesses.push(AffineWitness {
        nodes: vec![scalar(0.)],
        widening: 0.,
    });
    assert!(validate_request(&bad).is_err());
    let mut signed = request;
    signed.query_times = vec![-0., 0., 0.5, 0.5];
    produce(&signed);
}
