use eigentrail_core::{common_frame::*, families::*, resources::Cancellation, *};

fn request() -> HistoryRequest {
    let observations: TraceRequest = serialization::decode(include_bytes!(
        "../../../fixtures/common-frame-request.json"
    ))
    .unwrap();
    let k = observations.lipschitz;
    HistoryRequest {
        schema: eigentrail_core::families::SCHEMA.into(),
        observations,
        configurations: vec![
            Configuration {
                id: "continuous".into(),
                lipschitz: k,
                class: PathClass::Continuous,
                witnesses: vec![],
            },
            Configuration {
                id: "anchored".into(),
                lipschitz: k,
                class: PathClass::CommonFrame {
                    model: CommonFrameModel {
                        weights: vec![1., 0.],
                        common_frame_assumed: true,
                    },
                },
                witnesses: vec![],
            },
            Configuration {
                id: "tied-anchor".into(),
                lipschitz: k,
                class: PathClass::CommonFrame {
                    model: CommonFrameModel {
                        weights: vec![0.5; 2],
                        common_frame_assumed: true,
                    },
                },
                witnesses: vec![],
            },
        ],
        query_times: vec![0.5, 1., 0., 0.5],
    }
}
fn produce(request: &HistoryRequest) -> HistoryReport {
    let cancel = Cancellation::default();
    let report = eigentrail_core::families::analyze(request, &cancel).unwrap();
    eigentrail_check::families::check(request, &report, &cancel).unwrap();
    report
}
#[test]
fn whole_common_frame_members_and_tied_anchor_remain_in_the_union() {
    let request = request();
    let report = produce(&request);
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/common_frame_histories.json"
    ))
    .unwrap();
    assert_eq!(
        report.configurations.len(),
        oracle["configuration_count"].as_u64().unwrap() as usize
    );
    assert!(report.configurations[0].common_frame.is_empty());
    let CommonFrameAssessment::Resolved { report: anchored } =
        &report.configurations[1].common_frame[0]
    else {
        panic!("missing persistent labels")
    };
    let ranks: Vec<Vec<usize>> = anchored
        .frame
        .possible_ranks
        .iter()
        .map(|row| {
            row.iter()
                .map(|range| {
                    assert_eq!(range[0], range[1]);
                    range[0]
                })
                .collect()
        })
        .collect();
    assert_eq!(serde_json::to_value(ranks).unwrap(), oracle["nodal_ranks"]);
    assert!(matches!(
        report.configurations[2].common_frame[0],
        CommonFrameAssessment::UnresolvedAnchor { .. }
    ));
    for cfg in &report.configurations {
        assert_eq!(
            cfg.grid.iter().map(|n| n.time).collect::<Vec<_>>(),
            vec![0., 0.5, 1.]
        );
        assert_eq!(
            cfg.grid
                .iter()
                .map(|n| n.queries.clone())
                .collect::<Vec<_>>(),
            vec![vec![2], vec![0, 3], vec![1]]
        );
    }
    assert_eq!(
        report.configurations[1].feasibility,
        Feasibility::OriginalAffineWitness
    );
    assert!(matches!(
        report.configurations[2].feasibility,
        Feasibility::OriginalAffineWitness | Feasibility::Unresolved
    ));
    let encoded = serialization::encode(&report).unwrap();
    assert_eq!(
        serialization::decode::<HistoryReport>(&encoded).unwrap(),
        report
    );
    let wire: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert!(wire["configurations"][0].get("common_frame").is_none());
    assert!(
        wire["configurations"][2]["common_frame"][0]
            .get("frame")
            .is_none()
    );
    let PathClass::CommonFrame { model } = &request.configurations[2].class else {
        unreachable!()
    };
    assert!(matches!(
        eigentrail_core::common_frame::analyze(
            &request.observations,
            model,
            &Cancellation::default()
        ),
        Err(Error::Exhausted(_))
    ));
    let mut witness = request.clone();
    witness.configurations[1].witnesses.push(AffineWitness {
        nodes: request.observations.covariances.clone(),
        widening: 0.,
    });
    assert_eq!(
        produce(&witness).configurations[1].feasibility,
        Feasibility::OriginalAffineWitness
    );
}

#[test]
fn checker_reconstructs_assessment_status_event_labels_and_complete_scope() {
    let request = request();
    let report = produce(&request);
    let mut cases = Vec::new();
    let mut bad = report.clone();
    bad.configurations.pop();
    cases.push(bad);
    let mut bad = report.clone();
    bad.configurations[1].common_frame.clear();
    cases.push(bad);
    let mut bad = report.clone();
    bad.configurations[0].common_frame = report.configurations[1].common_frame.clone();
    cases.push(bad);
    let mut bad = report.clone();
    bad.configurations[2]
        .common_frame
        .push(report.configurations[2].common_frame[0].clone());
    cases.push(bad);
    let mut bad = report.clone();
    bad.configurations[1].feasibility = Feasibility::WidenedAffineWitness { additional: 0.125 };
    cases.push(bad);
    let mut bad = report.clone();
    bad.configurations[2].grid[1].queries.pop();
    cases.push(bad);
    let mut bad = report.clone();
    let CommonFrameAssessment::Resolved { report: r } = &report.configurations[1].common_frame[0]
    else {
        unreachable!()
    };
    bad.configurations[1].common_frame[0] = CommonFrameAssessment::UnresolvedAnchor {
        request_hash: r.request_hash.clone(),
        model_hash: r.model_hash.clone(),
        eigensystems: r.eigensystems.clone(),
        radii: r.radii.clone(),
        calibration_scope: r.calibration_scope.clone(),
        axes: r.frame.axes.clone(),
    };
    cases.push(bad);
    for alteration in 0..6 {
        let mut bad = report.clone();
        let CommonFrameAssessment::UnresolvedAnchor {
            request_hash,
            radii,
            axes,
            calibration_scope,
            ..
        } = &mut bad.configurations[2].common_frame[0]
        else {
            unreachable!()
        };
        match alteration {
            0 => *request_hash = "forged".into(),
            1 => radii[0] = 0.,
            2 => axes.values[0] += 1.,
            3 => axes.vectors.data[0] += 0.5,
            4 => *calibration_scope = "unconditional".into(),
            _ => axes.backward = -1.,
        }
        cases.push(bad);
    }
    let mut bad = report.clone();
    let CommonFrameAssessment::Resolved { report: r } = &mut bad.configurations[1].common_frame[0]
    else {
        unreachable!()
    };
    r.frame.possible_ranks[1][0] = [1, 1];
    cases.push(bad);
    for bad in cases {
        assert!(
            eigentrail_check::families::check(&request, &bad, &Cancellation::default()).is_err()
        );
    }
    let mut wire: serde_json::Value =
        serde_json::from_slice(&serialization::encode(&report).unwrap()).unwrap();
    wire["configurations"][2]["common_frame"][0]["frame"] =
        serde_json::json!({"fabricated_labels":true});
    assert!(serialization::decode::<HistoryReport>(wire.to_string().as_bytes()).is_err());
}

#[test]
fn scalar_validation_budget_and_incomplete_errors_keep_their_meaning() {
    let mut request = request();
    request.configurations.remove(0);
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(matches!(
        eigentrail_core::families::analyze(&request, &cancelled),
        Err(Error::Cancelled)
    ));
    let reservation = validate_request(&request).unwrap();
    request.observations.memory_bytes = reservation - 1;
    assert!(matches!(
        eigentrail_core::families::analyze(&request, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
    request.observations.memory_bytes = resources::DEFAULT_BYTES;
    for weights in [
        vec![],
        vec![0.; 2],
        vec![-1., 2.],
        vec![f64::NAN, 1.],
        vec![f64::MAX; 2],
    ] {
        let mut invalid = request.clone();
        let PathClass::CommonFrame { model } = &mut invalid.configurations[0].class else {
            unreachable!()
        };
        model.weights = weights;
        assert!(eigentrail_core::families::analyze(&invalid, &Cancellation::default()).is_err());
    }
    let mut absent = request.clone();
    let PathClass::CommonFrame { model } = &mut absent.configurations[0].class else {
        unreachable!()
    };
    model.common_frame_assumed = false;
    assert!(matches!(
        eigentrail_core::families::analyze(&absent, &Cancellation::default()),
        Err(Error::Exhausted(_))
    ));
    let mut overflow = request.clone();
    overflow.observations.calibration = Calibration::Conditional {
        radii: vec![f64::MAX; 2],
        assumptions: "overflow must stay incomplete".into(),
    };
    assert!(eigentrail_core::families::analyze(&overflow, &Cancellation::default()).is_err());
    request.observations = TraceRequest::conditional(
        vec![0., 1.],
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
    for cfg in &mut request.configurations {
        cfg.lipschitz = 0.;
    }
    let scalar = produce(&request);
    assert!(
        scalar
            .configurations
            .iter()
            .all(|cfg| matches!(cfg.common_frame[0], CommonFrameAssessment::Resolved { .. }))
    );
}
