//! Frozen rational expectations are independent of the production algorithms.
use eigentrail_core::{common_frame_queries::QueryState, families::*, resources::Cancellation, *};
use serde_json::Value;

fn number(text: &str) -> f64 {
    let mut parts = text.split('/');
    let numerator: f64 = parts.next().unwrap().parse().unwrap();
    numerator / parts.next().map_or(1., |v| v.parse::<f64>().unwrap())
}
fn diagonal(row: &Value) -> Matrix {
    let values = row.as_array().unwrap();
    let mut data = vec![0.; values.len() * values.len()];
    for (i, value) in values.iter().enumerate() {
        data[i * (values.len() + 1)] = number(value.as_str().unwrap());
    }
    Matrix {
        n: values.len(),
        data,
    }
}
fn cases() -> Vec<(Value, HistoryRequest)> {
    let oracle: Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/workflow.json")).unwrap();
    oracle["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            let bits = case["encoded_lipschitz"]
                .as_str()
                .unwrap()
                .strip_prefix("f64:")
                .unwrap();
            let k = f64::from_bits(u64::from_str_radix(bits, 16).unwrap());
            let times = case["observation_times"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| number(t.as_str().unwrap()))
                .collect::<Vec<_>>();
            let covariances = case["observation_diagonals"]
                .as_array()
                .unwrap()
                .iter()
                .map(diagonal)
                .collect::<Vec<_>>();
            let radii = vec![number(case["nodal_radius"].as_str().unwrap()); times.len()];
            let observations = TraceRequest::conditional(times, covariances, radii, k);
            let witnesses = case
                .get("explicit_witness_diagonals")
                .map_or_else(Vec::new, |nodes| {
                    vec![AffineWitness {
                        nodes: nodes.as_array().unwrap().iter().map(diagonal).collect(),
                        widening: number(case["widening"].as_str().unwrap()),
                    }]
                });
            let request = HistoryRequest {
                schema: eigentrail_core::families::SCHEMA.into(),
                observations,
                configurations: vec![Configuration {
                    id: case["name"].as_str().unwrap().into(),
                    lipschitz: k,
                    class: PathClass::CommonFrame {
                        model: common_frame::CommonFrameModel {
                            weights: vec![1., 0., 0.],
                            common_frame_assumed: true,
                        },
                    },
                    witnesses,
                }],
                query_times: case["query_expectations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| number(row["time"].as_str().unwrap()))
                    .collect(),
            };
            (case.clone(), request)
        })
        .collect()
}
fn produce(request: &HistoryRequest) -> HistoryReport {
    let cancel = Cancellation::default();
    let report = eigentrail_core::families::analyze(request, &cancel).unwrap();
    eigentrail_check::families::check(request, &report, &cancel).unwrap();
    report
}
fn contains(range: &Range, exact: f64) {
    assert!(
        range.lower <= exact && exact <= range.upper,
        "{range:?} excludes {exact}"
    );
    assert!(range.lower_closed && range.upper_closed);
}

#[test]
fn rational_six_case_workflow_is_informative_and_independently_checked() {
    for (case, request) in cases() {
        let report = produce(&request);
        let cfg = &report.configurations[0];
        let workflow = &cfg.common_frame_queries[0];
        assert_eq!(workflow.queries.len(), request.query_times.len());
        let expected = case["query_expectations"].as_array().unwrap();
        for (row, oracle) in workflow.queries.iter().zip(expected) {
            assert_eq!(row.time, number(oracle["time"].as_str().unwrap()));
            let QueryState::Bounds { summaries, trace } = &row.state else {
                panic!("a frozen positive workflow did not produce bounds")
            };
            contains(trace, number(oracle["trace"].as_str().unwrap()));
            for (label, summary) in summaries.iter().enumerate() {
                contains(
                    &summary.strength,
                    number(oracle["strengths"][label].as_str().unwrap()),
                );
                let truth = &oracle["population_ranks"][label];
                assert!(summary.rank[0] <= truth[0].as_u64().unwrap() as usize);
                assert!(summary.rank[1] >= truth[1].as_u64().unwrap() as usize);
                match &summary.share {
                    Quantity::Interval(range) => {
                        assert_eq!(oracle["reported_share_kind"], "Interval");
                        contains(
                            range,
                            number(oracle["population_shares"][label].as_str().unwrap()),
                        );
                    }
                    Quantity::Undefined(_) => {
                        assert_eq!(oracle["reported_share_kind"], "Undefined")
                    }
                }
            }
        }
        if case["expected_witness_scope"] == "Original" {
            assert_eq!(cfg.feasibility, Feasibility::OriginalAffineWitness);
            assert_eq!(workflow.witnesses.len(), 1);
            assert_eq!(
                workflow.witnesses[0].feasibility,
                Feasibility::OriginalAffineWitness
            );
        } else {
            assert_eq!(
                cfg.feasibility,
                Feasibility::WidenedAffineWitness { additional: 0.125 }
            );
            assert!(workflow.witnesses.is_empty());
        }
        let bounds = |index: usize| {
            let QueryState::Bounds { summaries, .. } = &workflow.queries[index].state else {
                unreachable!()
            };
            summaries
        };
        match case["name"].as_str().unwrap() {
            "share-80-to-81" => {
                for row in &workflow.queries {
                    let QueryState::Bounds { summaries, .. } = &row.state else {
                        unreachable!()
                    };
                    assert!(summaries[0].strength.upper - summaries[0].strength.lower < 1e-8);
                    assert_eq!(summaries[0].rank, [1, 1]);
                }
                let (Quantity::Interval(start), Quantity::Interval(end)) =
                    (&bounds(0)[0].share, &bounds(4)[0].share)
                else {
                    unreachable!()
                };
                assert!(start.upper < end.lower);
            }
            "strength-down-share-up" => {
                assert!(bounds(4)[0].strength.upper < bounds(0)[0].strength.lower);
                let (Quantity::Interval(start), Quantity::Interval(end)) =
                    (&bounds(0)[0].share, &bounds(4)[0].share)
                else {
                    unreachable!()
                };
                assert!(start.upper < end.lower);
            }
            "rank-exchange" => {
                assert_eq!(bounds(0)[0].rank, [1, 1]);
                assert_eq!(bounds(2)[0].rank, [1, 2]);
                assert_eq!(bounds(2)[1].rank, [1, 2]);
                assert_eq!(bounds(4)[0].rank, [2, 2]);
            }
            "widened-witness" => assert!(bounds(0)[0].strength.upper < 80.25),
            _ => {}
        }
        let bytes = serialization::encode(&report).unwrap();
        let replay: HistoryReport = serialization::decode(&bytes).unwrap();
        eigentrail_check::families::check(&request, &replay, &Cancellation::default()).unwrap();
    }
}

#[test]
fn independent_checker_rejects_query_share_rank_trace_and_witness_corruption() {
    let request = cases().remove(0).1;
    let report = produce(&request);
    for mutation in 0..17 {
        let mut bad = report.clone();
        let cfg = &mut bad.configurations[0];
        let workflow = &mut cfg.common_frame_queries[0];
        match mutation {
            0 => {
                cfg.common_frame_queries.clear();
            }
            1 => {
                workflow.scope.push_str(" forged");
            }
            2 => {
                workflow.queries.reverse();
            }
            3 => {
                workflow.queries[0].time = 0.125;
            }
            4 => {
                workflow.queries.pop();
            }
            5..=9 => {
                let QueryState::Bounds { summaries, trace } = &mut workflow.queries[0].state else {
                    unreachable!()
                };
                match mutation {
                    5 => summaries[0].strength.lower = 99.,
                    6 => summaries[0].share = Quantity::Undefined("forged".into()),
                    7 => summaries[0].rank = [2, 2],
                    8 => trace.upper += 1.,
                    _ => {
                        let Quantity::Interval(share) = &mut summaries[0].share else {
                            unreachable!()
                        };
                        share.upper += 0.001;
                    }
                }
            }
            10 => workflow.witnesses[0].diagonal_nodes[0][0] += 1.,
            11 => workflow.witnesses[0].nodal_error_bounds[0] = -1.,
            12 => workflow.witnesses[0].step_norm_bounds[0] = 0.,
            13 => workflow.witnesses[0].feasibility = Feasibility::Unresolved,
            14 => {
                let extra = workflow.witnesses[0].clone();
                workflow.witnesses.push(extra);
            }
            15 => {
                workflow.witnesses.clear();
            }
            _ => cfg.feasibility = Feasibility::UnsupportedModel,
        }
        assert!(
            eigentrail_check::families::check(&request, &bad, &Cancellation::default()).is_err(),
            "checker accepted corruption {mutation}"
        );
    }
}

#[test]
fn unresolved_anchor_empty_outer_intersection_and_explicit_model_limits_are_honest() {
    let all = cases();
    let mut tied = all[2].1.clone();
    let PathClass::CommonFrame { model } = &mut tied.configurations[0].class else {
        unreachable!()
    };
    model.weights = vec![0.5, 0., 0.5];
    let tied = produce(&tied);
    assert!(
        tied.configurations[0].common_frame_queries[0]
            .queries
            .iter()
            .all(|row| matches!(row.state, QueryState::UnresolvedAnchor))
    );
    assert_eq!(
        tied.configurations[0].feasibility,
        Feasibility::OriginalAffineWitness
    );
    let mut impossible = all[0].1.clone();
    impossible.configurations[0].lipschitz = 0.;
    let report = produce(&impossible);
    assert_eq!(
        report.configurations[0].feasibility,
        Feasibility::Unresolved
    );
    assert!(
        report.configurations[0].common_frame_queries[0]
            .queries
            .iter()
            .any(|row| matches!(row.state, QueryState::InconsistentOuterBounds))
    );
    let mut unsupported = all[0].1.clone();
    let mut nodes = unsupported.observations.covariances.clone();
    for node in &mut nodes {
        node.data[1] = 0.125;
        node.data[2] = 0.125;
    }
    unsupported.configurations[0].witnesses = vec![AffineWitness {
        nodes,
        widening: 0.,
    }];
    let report = produce(&unsupported);
    assert_eq!(
        report.configurations[0].feasibility,
        Feasibility::UnsupportedModel
    );
    assert!(
        report.configurations[0].common_frame_queries[0]
            .witnesses
            .is_empty()
    );
}

#[test]
fn query_order_duplicates_cancellation_and_resource_refusal_are_preserved() {
    let mut request = cases().remove(0).1;
    request.query_times = vec![0.75, 0., 0.5, 0.75, 1.];
    let report = produce(&request);
    assert_eq!(
        report.configurations[0].common_frame_queries[0]
            .queries
            .iter()
            .map(|row| row.time)
            .collect::<Vec<_>>(),
        request.query_times
    );
    assert_eq!(
        report.configurations[0].common_frame_queries[0].queries[0],
        report.configurations[0].common_frame_queries[0].queries[3]
    );
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(matches!(
        eigentrail_core::families::analyze(&request, &cancelled),
        Err(Error::Cancelled)
    ));
    request.observations.memory_bytes = validate_request(&request).unwrap() - 1;
    assert!(matches!(
        eigentrail_core::families::analyze(&request, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
}

#[test]
fn rotated_polar_witness_and_anchor_column_sign_are_checked_without_changing_labels() {
    // R=([3/5,4/5],[-4/5,3/5]); the exact population is R diag(80,20) R^T.
    let mut request = cases().remove(0).1;
    request.observations.covariances = vec![
        Matrix {
            n: 2,
            data: vec![208. / 5., 144. / 5., 144. / 5., 292. / 5.],
        };
        3
    ];
    request.observations.lipschitz = 0.;
    request.observations.calibration = Calibration::Conditional {
        radii: vec![1e-10; 3],
        assumptions: "constant rational rotated covariance; rounding covered by nodal envelope"
            .into(),
    };
    request.configurations[0].lipschitz = 0.;
    let mut report = produce(&request);
    let cfg = &report.configurations[0];
    assert_eq!(cfg.feasibility, Feasibility::OriginalAffineWitness);
    let workflow = &cfg.common_frame_queries[0];
    let witness = &workflow.witnesses[0];
    assert_eq!(witness.feasibility, Feasibility::OriginalAffineWitness);
    assert!(
        witness
            .nodal_error_bounds
            .iter()
            .all(|e| e.is_finite() && *e >= 0. && *e <= 1e-10)
    );
    assert_eq!(witness.step_norm_bounds, vec![0., 0.]);
    for row in &workflow.queries {
        let QueryState::Bounds { summaries, .. } = &row.state else {
            unreachable!()
        };
        for (summary, (strength, share)) in summaries.iter().zip([(80., 0.8), (20., 0.2)]) {
            contains(&summary.strength, strength);
            assert!(summary.strength.upper - summary.strength.lower < 1e-8);
            let Quantity::Interval(interval) = &summary.share else {
                panic!("positive trace")
            };
            contains(interval, share);
        }
    }
    let before = report.configurations[0].common_frame_queries.clone();
    let common_frame::CommonFrameAssessment::Resolved { report: anchor } =
        &mut report.configurations[0].common_frame[0]
    else {
        unreachable!()
    };
    assert!(anchor.frame.axes.polar.is_finite() && anchor.frame.axes.polar >= 0.);
    for entry in &mut anchor.frame.axes.vectors.data[..2] {
        *entry = -*entry;
    }
    eigentrail_check::families::check(&request, &report, &Cancellation::default()).unwrap();
    assert_eq!(report.configurations[0].common_frame_queries, before);
}
