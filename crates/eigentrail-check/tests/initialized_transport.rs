//! Frozen independent initialized-transport geometry and covariance cases.
use eigentrail_core::{
    resources::{Cancellation, memory::Operation},
    scalar::*,
    transport_workflow::{self, TransportAssessment, TransportRequest},
    *,
};
use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/initialized_transport.json"
    ))
    .unwrap()
}
fn number(value: &Value) -> f64 {
    let text = value.as_str().unwrap();
    if let Some(raw) = text.strip_prefix("f64:") {
        return f64::from_bits(u64::from_str_radix(raw, 16).unwrap());
    }
    let mut parts = text.split('/');
    let a: f64 = parts.next().unwrap().parse().unwrap();
    a / parts.next().map_or(1., |v| v.parse::<f64>().unwrap())
}
fn encoded(value: f64) -> Value {
    Value::String(format!("f64:{:016x}", value.to_bits()))
}
fn input(case: &Value) -> TransportRequest {
    serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap()
}
fn produce(case: &Value) -> (TransportRequest, Value) {
    let request = input(case);
    let cancel = Cancellation::default();
    let assessment = transport_workflow::analyze(&request, &cancel).unwrap();
    eigentrail_check::transport::check(&request, &assessment, &cancel).unwrap();
    let bytes = serialization::encode(&assessment).unwrap();
    let replay: TransportAssessment = serialization::decode(&bytes).unwrap();
    eigentrail_check::transport::check(&request, &replay, &cancel).unwrap();
    (request, serde_json::from_slice(&bytes).unwrap())
}
fn rejected(request: &TransportRequest, bad: Value) {
    let assessment =
        serialization::decode::<TransportAssessment>(&serde_json::to_vec(&bad).unwrap());
    assert!(
        assessment.is_err()
            || eigentrail_check::transport::check(
                request,
                &assessment.unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
}
fn near_values(actual: &Value, truth: &Value, tolerance: f64) {
    let actual = actual.as_array().unwrap();
    let truth = truth.as_array().unwrap();
    assert_eq!(actual.len(), truth.len());
    for (value, expected) in actual.iter().zip(truth) {
        assert!((number(value) - number(expected)).abs() <= tolerance);
    }
}
fn rational_frame(p: usize, time: f64, gauge: bool) -> Vec<f64> {
    let z = time / 1024.;
    let c = (1. - z * z) / (1. + z * z);
    let s = 2. * z / (1. + z * z);
    if p == 2 {
        vec![c, s, 0., 0.]
    } else if gauge {
        vec![0., 1., 0., -c, 0., -s, 0., 0., 0.]
    } else {
        vec![c, 0., s, 0., 1., 0., 0., 0., 0.]
    }
}

#[test]
fn rational_paths_gauge_stationary_block_and_unequal_fibres_are_informative() {
    for case in oracle()["cases"].as_array().unwrap() {
        if case["expectation"]["outcome"] != "Resolved" {
            continue;
        }
        let (request, report) = produce(case);
        assert_eq!(report["schema"], "eigentrail.transport.v1");
        assert_eq!(report["completed"], true);
        assert_eq!(report["outcome"]["kind"], "Resolved");
        assert!(report["covariance"].as_array().unwrap().is_empty());
        assert_eq!(
            report["reference_bounds"].as_array().unwrap().len(),
            request.times.len()
        );
        assert_eq!(
            report["node_bounds"].as_array().unwrap().len(),
            request.times.len()
        );
        assert_eq!(
            report["endpoints"].as_array().unwrap().len(),
            request.times.len()
        );
        assert_eq!(
            report["steps"].as_array().unwrap().len(),
            request.times.len() - 1
        );
        for field in ["initial", "rotation", "frame", "budget"] {
            assert_eq!(report[field].as_array().unwrap().len(), 1);
        }
        let tolerance = number(&case["expectation"]["numerical_tolerance"]);
        near_values(
            &report["rotation"][0]["data"],
            &case["expectation"]["rotation"],
            tolerance,
        );
        near_values(
            &report["frame"][0]["data"],
            &case["expectation"]["frame"],
            tolerance,
        );
        let p = request.dimension;
        let [start, end] = request.block;
        let frame = &report["frame"][0]["data"];
        let rotation = &report["rotation"][0]["data"];
        for i in 0..p {
            for j in 0..p {
                let gram: f64 = (0..p)
                    .map(|k| number(&rotation[k + i * p]) * number(&rotation[k + j * p]))
                    .sum();
                assert!((gram - f64::from(i == j)).abs() <= 1e-8);
                if i < start || i >= end {
                    assert_eq!(number(&frame[j + i * p]), 0.);
                }
            }
        }
        for i in start..end {
            for j in start..end {
                let gram: f64 = (0..p)
                    .map(|k| number(&frame[k + i * p]) * number(&frame[k + j * p]))
                    .sum();
                assert!((gram - f64::from(i == j)).abs() <= 1e-8);
            }
        }
        if case["id"].as_str().unwrap().starts_with("rational-") {
            let ordinary = rational_frame(p, 1., false);
            for row in 0..p {
                for col in 0..p {
                    let left: f64 = (0..p)
                        .map(|k| {
                            let projector: f64 = (start..end)
                                .map(|mode| ordinary[row + mode * p] * ordinary[k + mode * p])
                                .sum();
                            projector * number(&rotation[k + col * p])
                        })
                        .sum();
                    let right = if (start..end).contains(&col) {
                        number(&rotation[row + col * p])
                    } else {
                        0.
                    };
                    assert!((left - right).abs() <= tolerance, "analytical intertwining");
                }
            }
            for (j, step) in report["steps"].as_array().unwrap().iter().enumerate() {
                assert_eq!(step["rotation"].as_array().unwrap().len(), 1);
                let actual = &step["rotation"][0]["frame"]["data"];
                let truth = rational_frame(
                    p,
                    request.times[j + 1],
                    case["expectation"].get("gauge").is_some(),
                );
                for (value, expected) in actual.as_array().unwrap().iter().zip(truth) {
                    assert!((number(value) - expected).abs() <= tolerance);
                }
                assert!(number(&step["rotation"][0]["frame_error"]) <= 0.001);
            }
        }
        let budget = &report["budget"][0];
        assert!(number(&budget["comparison_error"]) < request.target);
        assert!(number(&budget["reference_speed"]) <= request.reference_speed_limit);
        assert!(number(&budget["speed"]) <= request.max_speed);
        assert!(number(&budget["endpoint_error"]) <= request.endpoint_error_limit);
        assert!(number(&budget["initial_correction"]) <= request.initial_correction_limit);
        assert!(number(&budget["initial_mismatch"]) <= request.initial_mismatch_limit);
        assert!(number(&budget["numerical_error"]) <= 0.001);
        if let Some(upper) = case["expectation"].get("comparison_error_upper") {
            assert!(number(&budget["comparison_error"]) < number(upper));
        }
        if case["expectation"]
            .get("mismatch_strictly_positive")
            .is_some()
        {
            assert!(number(&budget["initial_mismatch"]) > 0.);
            assert!(
                number(&budget["initial_mismatch"])
                    >= number(&case["expectation"]["initial_mismatch_lower"])
            );
            assert!(
                number(&budget["endpoint_error"])
                    >= number(&case["expectation"]["endpoint_distance_lower"])
            );
        }
    }
}

#[test]
fn whole_space_projector_is_exact_identity_despite_raw_frame_motion_and_polar_error() {
    let mut request = input(&oracle()["cases"][0]);
    request.times = vec![0., 0.5, 1.];
    request.block = [0, 2];
    request.initial_gauge = Matrix {
        n: 2,
        data: vec![1., 0., 0., 1.],
    };
    let scale = 1. + 1e-9;
    request.reference_nodes = vec![
        Matrix {
            n: 2,
            data: vec![scale, 0., 0., scale],
        },
        Matrix {
            n: 2,
            data: vec![0., scale, -scale, 0.],
        },
        Matrix {
            n: 2,
            data: vec![-scale, 0., 0., -scale],
        },
    ];
    request.nodes = request.reference_nodes.clone();
    request.reference_speed_limit = 0.;
    request.max_speed = 0.;
    let cancel = Cancellation::default();
    let assessment = transport_workflow::analyze(&request, &cancel).unwrap();
    eigentrail_check::transport::check(&request, &assessment, &cancel).unwrap();
    assert!(matches!(
        assessment.outcome,
        transport_workflow::Outcome::Resolved
    ));
    for proof in assessment
        .reference_bounds
        .iter()
        .chain(&assessment.node_bounds)
    {
        assert!(proof.polar > 0.);
        assert_eq!(proof.projector_error, 0.);
    }
    assert_eq!(assessment.budget[0].reference_speed, 0.);
    assert_eq!(assessment.budget[0].speed, 0.);
    for row in 0..2 {
        for col in 0..2 {
            assert!((assessment.rotation[0].at(row, col) - f64::from(row == col)).abs() < 1e-8);
            assert!((assessment.frame[0].at(row, col) - f64::from(row == col)).abs() < 1e-8);
        }
    }
}

#[test]
fn initialization_speed_cut_locus_endpoint_and_accuracy_failures_retain_their_stage() {
    for case in oracle()["cases"].as_array().unwrap() {
        if case["expectation"]["outcome"] != "Unresolved" {
            continue;
        }
        let (request, report) = produce(case);
        assert_eq!(report["completed"], true);
        assert_eq!(report["outcome"]["kind"], "Unresolved");
        assert_eq!(report["outcome"]["gate"], case["expectation"]["gate"]);
        assert_eq!(report["initial"].as_array().unwrap().len(), 1);
        let gate = case["expectation"]["gate"].as_str().unwrap();
        if ["InitialFibre", "InitialMismatch"].contains(&gate) {
            assert!(report["endpoints"].as_array().unwrap().is_empty());
            assert!(report["steps"].as_array().unwrap().is_empty());
        } else {
            assert_eq!(
                report["endpoints"].as_array().unwrap().len(),
                request.times.len()
            );
        }
        if gate == "EndpointError" {
            assert!(report["steps"].as_array().unwrap().is_empty());
        }
        if ["ReferenceSpeed", "Speed", "CutLocus"].contains(&gate) {
            assert_eq!(report["steps"].as_array().unwrap().len(), 1);
            let rotations = report["steps"][0]["rotation"].as_array().unwrap();
            assert_eq!(rotations.len(), usize::from(gate != "CutLocus"));
        }
        for field in ["rotation", "frame", "budget"] {
            assert_eq!(
                report[field].as_array().unwrap().len(),
                usize::from(gate == "Accuracy")
            );
        }
        let mut forged = report;
        forged["outcome"] = serde_json::json!({"kind":"Resolved"});
        rejected(&request, forged);
    }
}

#[test]
fn independent_checker_rejects_frames_products_omitted_steps_and_tighter_claims() {
    let data = oracle();
    let (request, report) = produce(&data["cases"][0]);
    for mutation in 0..26 {
        let mut bad = report.clone();
        match mutation {
            0 => bad["request_hash"] = Value::String("0".repeat(64)),
            1 => bad["scope"] = Value::String("unconditional covariance transport".into()),
            2 => bad["completed"] = Value::Bool(false),
            3 => {
                bad["reference_bounds"].as_array_mut().unwrap().pop();
            }
            4 => bad["node_bounds"][0]["polar"] = encoded(-1.),
            5 => bad["node_bounds"][0]["projector_error"] = encoded(-1.),
            6 => bad["initial"] = serde_json::json!([]),
            7 => bad["initial"][0]["correction"] = encoded(-1.),
            8 => bad["initial"][0]["mismatch"] = encoded(-1.),
            9 => bad["initial"][0]["basis"]["data"][0] = encoded(0.5),
            10 => bad["endpoints"][0]["distance"] = encoded(-1.),
            11 => bad["endpoints"][0]["eigensystem"]["values"][0] = encoded(1.),
            12 => {
                bad["steps"].as_array_mut().unwrap().pop();
            }
            13 => bad["steps"][0]["rotation"] = serde_json::json!([]),
            14 => bad["steps"][0]["rotation"][0]["matrix"]["data"][0] = encoded(0.5),
            15 => bad["steps"][0]["rotation"][0]["reference_angle"] = encoded(0.),
            16 => bad["steps"][0]["rotation"][0]["angle"] = encoded(0.),
            17 => bad["steps"][0]["rotation"][0]["local_error"] = encoded(0.),
            18 => bad["steps"][0]["rotation"][0]["product_error"] = encoded(0.),
            19 => bad["steps"][0]["rotation"][0]["frame"]["data"][0] = encoded(0.5),
            20 => bad["steps"][0]["rotation"][0]["frame_error"] = encoded(0.),
            21 => bad["rotation"][0]["data"][0] = encoded(0.5),
            22 => bad["frame"][0]["data"][0] = encoded(0.5),
            23 => bad["budget"] = serde_json::json!([]),
            24 => bad["budget"][0]["comparison_error"] = encoded(0.),
            _ => {
                bad["covariance"] = serde_json::json!([{
                "observation_indices":[],"distances":[],"gaps":[],"node_gaps":[],"budget":[],
                "outcome":{"kind":"Resolved"},"scope":"geometry alone certifies population"}])
            }
        }
        rejected(&request, bad);
    }
}

#[test]
fn covariance_history_charges_observation_mesh_and_keeps_geometry_separate() {
    for case in oracle()["history_cases"].as_array().unwrap() {
        let request: families::HistoryRequest =
            serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap();
        let cancel = Cancellation::default();
        let report = families::analyze(&request, &cancel).unwrap();
        eigentrail_check::families::check(&request, &report, &cancel).unwrap();
        let value: Value =
            serde_json::from_slice(&serialization::encode(&report).unwrap()).unwrap();
        let cfg = &value["configurations"][0];
        assert_eq!(cfg["feasibility"]["kind"], "Unresolved");
        assert!(cfg.get("continuous_workflow").is_none());
        assert_eq!(cfg["transports"].as_array().unwrap().len(), 1);
        let transport = &cfg["transports"][0];
        assert_eq!(transport["outcome"]["kind"], "Resolved");
        assert_eq!(transport["covariance"].as_array().unwrap().len(), 1);
        let proof = &transport["covariance"][0];
        assert_eq!(
            proof["observation_indices"],
            case["expectation"]["observation_indices"]
        );
        assert_eq!(proof["outcome"]["kind"], case["expectation"]["covariance"]);
        assert_eq!(
            number(&proof["budget"][0]["mesh"]),
            number(&case["expectation"]["observation_mesh"])
        );
        if let Some(gate) = case["expectation"].get("gate") {
            assert_eq!(proof["outcome"]["gate"], *gate);
            assert!(number(&proof["budget"][0]["total_error"]) >= 0.01);
        } else {
            assert!(number(&proof["budget"][0]["total_error"]) < 0.01);
            assert!(number(&proof["budget"][0]["endpoint_error"]) <= 1e-8);
        }
        for mutation in 0..7 {
            let mut bad = value.clone();
            let cfg = &mut bad["configurations"][0];
            match mutation {
                0 => cfg["transports"] = serde_json::json!([]),
                1 => cfg["transports"][0]["covariance"] = serde_json::json!([]),
                2 => {
                    cfg["transports"][0]["covariance"][0]["distances"][0]["distance"] = encoded(-1.)
                }
                3 => cfg["transports"][0]["covariance"][0]["gaps"] = serde_json::json!([]),
                4 => cfg["transports"][0]["covariance"][0]["node_gaps"] = serde_json::json!([]),
                5 => {
                    cfg["transports"][0]["covariance"][0]["budget"][0]["initial_uncertainty"] =
                        encoded(-1.)
                }
                _ => cfg["feasibility"] = serde_json::json!({"kind":"OriginalAffineWitness"}),
            }
            let decoded = serialization::decode::<families::HistoryReport>(
                &serde_json::to_vec(&bad).unwrap(),
            );
            assert!(
                decoded.is_err()
                    || eigentrail_check::families::check(&request, &decoded.unwrap(), &cancel)
                        .is_err()
            );
        }
        let mut legacy = request;
        legacy.schema = families::SCHEMA.into();
        assert!(families::analyze(&legacy, &cancel).is_err());
    }
    let mut missing = oracle()["history_cases"][0]["request"].clone();
    for key in ["times", "reference_nodes", "nodes"] {
        missing["configurations"][0]["class"]["model"][key]
            .as_array_mut()
            .unwrap()
            .remove(64);
    }
    let request: families::HistoryRequest =
        serialization::decode(&serde_json::to_vec(&missing).unwrap()).unwrap();
    assert!(matches!(
        families::analyze(&request, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn cancellation_refinement_shapes_and_retained_outputs_obey_the_resource_contract() {
    let data = oracle();
    let mut request = input(&data["cases"][0]);
    request.memory_bytes = 16 * 1024 * 1024;
    let operation = Operation::new(request.memory_bytes).unwrap();
    let baseline = operation.stats().live;
    let cancel = Cancellation::default();
    let retained = operation
        .own(|| {
            let report = transport_workflow::analyze(&request, &cancel)?;
            eigentrail_check::transport::check(&request, &report, &cancel)?;
            let output = serialization::encode(&report)?;
            Ok((report, output))
        })
        .unwrap();
    assert!(operation.stats().live >= baseline + retained.1.capacity());
    cancel.cancel();
    assert!(matches!(
        transport_workflow::analyze(&request, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        eigentrail_check::transport::check(&request, &retained.0, &cancel),
        Err(Error::Cancelled)
    ));
    drop(retained);
    assert_eq!(operation.stats().live, baseline);
    let fresh = Cancellation::default();
    request.memory_bytes = 8192;
    assert!(matches!(
        transport_workflow::analyze(&request, &fresh),
        Err(Error::Resource(_))
    ));
    request.memory_bytes = resources::DEFAULT_BYTES;
    request.times = (0..65538).map(|j| j as f64).collect();
    assert!(matches!(
        transport_workflow::analyze(&request, &fresh),
        Err(Error::Exhausted(_))
    ));
    let mut request = input(&data["cases"][0]);
    request.nodes[0].data[0] = f64::NAN;
    assert!(matches!(
        transport_workflow::analyze(&request, &fresh),
        Err(Error::Invalid(_))
    ));
    let mut request = input(&data["cases"][0]);
    request.nodes[0].data.fill(0.);
    assert!(matches!(
        transport_workflow::analyze(&request, &fresh),
        Err(Error::Exhausted(_))
    ));
}

#[test]
fn represented_rank_one_loop_keeps_exact_closure_and_checked_sign_holonomy() {
    let recipe = &oracle()["recipes"]["rank_one_pi_loop"];
    let n = recipe["cells"].as_u64().unwrap() as usize;
    assert_eq!(n, 32768);
    let times: Vec<_> = (0..=n).map(|j| j as f64 / n as f64).collect();
    let mut nodes = Vec::with_capacity(n + 1);
    for (j, &time) in times.iter().enumerate() {
        let (s, c) = if j == 0 {
            (0., 1.)
        } else if j == n {
            (0., -1.)
        } else {
            (std::f64::consts::PI * time).sin_cos()
        };
        let matrix = Matrix {
            n: 2,
            data: vec![c, s, -s, c],
        };
        let angle = Interval::PI * point(time);
        let exact = [angle.cos(), angle.sin(), -angle.sin(), angle.cos()];
        let error = matrix
            .data
            .iter()
            .zip(exact)
            .fold(point(0.), |total, (&v, truth)| {
                total + square(point(v) - truth)
            })
            .sqrt();
        assert!(
            (point(4.) * error).sup() <= 1e-10,
            "independent analytic endpoint eligibility"
        );
        nodes.push(matrix);
    }
    assert_eq!(nodes[0].data, vec![1., 0., -0., 1.]);
    assert_eq!(nodes[n].data, vec![-1., 0., -0., -1.]);
    let request = TransportRequest {
        schema: "eigentrail.transport.v1".into(),
        dimension: 2,
        times,
        reference_nodes: nodes.clone(),
        nodes,
        block: [0, 1],
        initial_frame: Matrix {
            n: 2,
            data: vec![1., 0., 0., 1.],
        },
        initial_gauge: Matrix {
            n: 1,
            data: vec![1.],
        },
        reference_speed_limit: number(&recipe["reference_speed_limit"]),
        max_speed: number(&recipe["max_speed"]),
        endpoint_error_limit: 1e-10,
        initial_correction_limit: 1e-8,
        initial_mismatch_limit: 1e-8,
        target: 0.01,
        memory_bytes: resources::DEFAULT_BYTES,
    };
    let cancel = Cancellation::default();
    let report = transport_workflow::analyze(&request, &cancel).unwrap();
    eigentrail_check::transport::check(&request, &report, &cancel).unwrap();
    assert!(matches!(
        report.outcome,
        transport_workflow::Outcome::Resolved
    ));
    assert_eq!(report.steps.len(), n);
    assert_eq!(report.rotation.len(), 1);
    assert_eq!(report.frame.len(), 1);
    assert_eq!(report.budget.len(), 1);
    assert!(report.budget[0].comparison_error < 0.01);
    assert!(report.budget[0].numerical_error <= 0.001);
    for i in 0..2 {
        for j in 0..2 {
            assert!((report.rotation[0].at(i, j) + f64::from(i == j)).abs() < 1e-8);
        }
    }
    assert!((report.frame[0].at(0, 0) + 1.).abs() < 1e-8);
    assert!(report.frame[0].at(1, 0).abs() < 1e-8);
    assert_eq!(report.frame[0].at(0, 1), 0.);
    assert_eq!(report.frame[0].at(1, 1), 0.);
}
