//! Frozen independent rational examples through public crossing/check/history APIs.
use eigentrail_core::{
    resources::{Cancellation, memory::Operation},
    scalar::point,
    *,
};
use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/transverse_crossings.json"
    ))
    .unwrap()
}
fn number(v: &Value) -> f64 {
    let text = v.as_str().unwrap();
    if let Some(raw) = text.strip_prefix("f64:") {
        return f64::from_bits(u64::from_str_radix(raw, 16).unwrap());
    }
    let mut parts = text.split('/');
    let numerator: f64 = parts.next().unwrap().parse().unwrap();
    numerator / parts.next().map_or(1., |s| s.parse::<f64>().unwrap())
}
fn encoded(value: f64) -> Value {
    Value::String(format!("f64:{:016x}", value.to_bits()))
}
fn inputs(case: &Value) -> (TraceRequest, crossings::CrossingWindow) {
    (
        serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap(),
        serialization::decode(&serde_json::to_vec(&case["window"]).unwrap()).unwrap(),
    )
}
fn produce(case: &Value) -> (TraceRequest, crossings::CrossingWindow, Value) {
    let (request, window) = inputs(case);
    let cancel = Cancellation::default();
    let assessment = crossings::analyze(&request, &window, &cancel).unwrap();
    eigentrail_check::crossings::check(&request, &window, &assessment, &cancel).unwrap();
    let bytes = serialization::encode(&assessment).unwrap();
    let replay: crossings::CrossingAssessment = serialization::decode(&bytes).unwrap();
    eigentrail_check::crossings::check(&request, &window, &replay, &cancel).unwrap();
    (request, window, serde_json::from_slice(&bytes).unwrap())
}
fn rejected(request: &TraceRequest, window: &crossings::CrossingWindow, bad: Value) {
    let value =
        serialization::decode::<crossings::CrossingAssessment>(&serde_json::to_vec(&bad).unwrap());
    assert!(
        value.is_err()
            || eigentrail_check::crossings::check(
                request,
                window,
                &value.unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn pair_multiplicity_and_moving_plane_have_checked_attachments_and_timing() {
    for case in oracle()["cases"].as_array().unwrap() {
        if case["expectation"]["outcome"] != "Resolved" {
            continue;
        }
        let (_, _, report) = produce(case);
        assert_eq!(report["schema"], "eigentrail.crossings.v1");
        assert_eq!(report["completed"], true);
        assert_eq!(report["outcome"]["kind"], "Resolved");
        assert_eq!(report["proof"].as_array().unwrap().len(), 1);
        let proof = &report["proof"][0];
        assert_eq!(proof["budget"].as_array().unwrap().len(), 1);
        assert_eq!(proof["left"], case["expectation"]["left"]);
        assert_eq!(proof["right"], case["expectation"]["right"]);
        for side in ["left", "right"] {
            let singleton: Vec<_> = case["expectation"][side]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| serde_json::json!([i]))
                .collect();
            assert_eq!(proof[format!("{side}_candidates")], Value::Array(singleton));
        }
        let budget = &proof["budget"][0];
        assert!(
            number(&budget["branch_error"]) < number(&case["expectation"]["branch_error_upper"])
        );
        let values = proof["compressed"]["values"].as_array().unwrap();
        for (value, truth) in values
            .iter()
            .rev()
            .zip(case["expectation"]["slopes"].as_array().unwrap())
        {
            assert!(
                (number(value) - number(truth)).abs()
                    <= number(&budget["derivative_error"])
                        + number(&proof["compressed"]["backward"])
            );
        }
        assert_eq!(proof["contrasts"].as_array().unwrap().len(), 2);
        assert_eq!(proof["timing"].as_array().unwrap().len(), 1);
        let interval = &proof["timing"][0];
        assert!(number(&interval["lower"]) <= 0. && number(&interval["upper"]) >= 0.);
        assert_eq!(interval["lower_closed"], true);
        assert_eq!(interval["upper_closed"], true);
        if let Some(width) = case["expectation"].get("timing_width_upper") {
            assert!(number(&interval["upper"]) - number(&interval["lower"]) < number(width));
        }
    }
}

#[test]
fn scientific_gates_preserve_structured_unresolved_results_and_competitors() {
    for case in oracle()["cases"].as_array().unwrap() {
        if case["expectation"]["outcome"] != "Unresolved" {
            continue;
        }
        let (request, window, report) = produce(case);
        assert_eq!(report["completed"], true);
        assert_eq!(report["outcome"]["kind"], "Unresolved");
        assert_eq!(report["outcome"]["gate"], case["expectation"]["gate"]);
        if case["expectation"]["gate"] == "StructuralPremise" {
            assert!(report["proof"].as_array().unwrap().is_empty());
            continue;
        }
        let proof = &report["proof"][0];
        assert!(proof["timing"].as_array().unwrap().is_empty());
        if case["expectation"].get("left_candidates").is_some() {
            assert_eq!(
                proof["left_candidates"],
                case["expectation"]["left_candidates"]
            );
            assert_eq!(
                proof["right_candidates"],
                case["expectation"]["right_candidates"]
            );
            assert!(proof["left"].as_array().unwrap().is_empty());
            assert!(proof["right"].as_array().unwrap().is_empty());
            let mut bad = report.clone();
            bad["proof"][0]["left_candidates"][0] = serde_json::json!([0]);
            bad["proof"][0]["right_candidates"][0] = serde_json::json!([1]);
            bad["proof"][0]["left"] = serde_json::json!([0, 1]);
            bad["proof"][0]["right"] = serde_json::json!([1, 0]);
            bad["outcome"] = serde_json::json!({"kind":"Resolved"});
            rejected(&request, &window, bad);
        }
        if case["expectation"]["gate"] == "TimingIntersection" {
            assert_eq!(proof["contrasts"].as_array().unwrap().len(), 2);
            assert_eq!(proof["left"].as_array().unwrap().len(), 2);
            let mut bad = report.clone();
            bad["proof"][0]["timing"] = serde_json::json!([{
                "lower":encoded(window.bracket[0]),"upper":encoded(window.bracket[1]),
                "lower_closed":true,"upper_closed":true}]);
            bad["outcome"] = serde_json::json!({"kind":"Resolved"});
            rejected(&request, &window, bad);
        }
    }
}

#[test]
fn compression_anchors_complete_graphs_and_timing_cannot_be_forged() {
    let data = oracle();
    let (request, window, report) = produce(&data["cases"][0]);
    for mutation in 0..14 {
        let mut bad = report.clone();
        match mutation {
            0 => bad["request_hash"] = Value::String("0".repeat(64)),
            1 => bad["window_hash"] = Value::String("0".repeat(64)),
            2 => bad["completed"] = Value::Bool(false),
            3 => bad["proof"][0]["compressed"]["values"][0] = encoded(2.),
            4 => bad["proof"][0]["compressed"]["vectors"]["data"][0] = encoded(0.25),
            5 => bad["proof"][0]["compression_error"] = encoded(-1.),
            6 => bad["proof"][0]["display_error"] = encoded(-1.),
            7 => bad["proof"][0]["left_candidates"][0] = serde_json::json!([]),
            8 => {
                bad["proof"][0]["right_candidates"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            9 => bad["proof"][0]["left"] = serde_json::json!([1, 0]),
            10 => bad["proof"][0]["contrasts"][0]["radius"] = encoded(-1.),
            11 => bad["proof"][0]["timing"][0]["lower"] = encoded(1e-6),
            12 => bad["baseline"]["eigensystems"][0]["values"][0] = encoded(3.),
            _ => {
                let values = bad["proof"][0]["slope_anchors"]["data"]
                    .as_array_mut()
                    .unwrap();
                values.swap(0, 2);
                values.swap(1, 3);
                for side in ["left", "right"] {
                    bad["proof"][0][side].as_array_mut().unwrap().swap(0, 1);
                    bad["proof"][0][format!("{side}_candidates")]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 1);
                }
            }
        }
        rejected(&request, &window, bad);
    }
    let mut changed = window;
    changed.exact_collision_assumed = false;
    rejected(&request, &changed, report);
}

#[test]
fn derivative_error_uses_the_entire_outward_secant_duration() {
    let case = oracle()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "outward-secant-duration")
        .unwrap()
        .clone();
    let (request, window, report) = produce(&case);
    let [a, b] = window.secant;
    let h = point(request.times[b]) - point(request.times[a]);
    assert!(
        h.inf() < h.sup(),
        "fixture does not exercise outward duration"
    );
    let proof = &report["proof"][0];
    let radius = &report["baseline"]["radii"];
    let expected = ((point(number(&radius[a])) + point(number(&radius[b]))) / h
        + point(window.curvature) * h / point(2.)
        + point(number(&proof["compression_error"])))
    .sup();
    assert_eq!(number(&proof["budget"][0]["derivative_error"]), expected);
}

#[test]
fn history_retains_each_crossing_window_without_promoting_family_feasibility() {
    let data = oracle();
    let request: families::HistoryRequest =
        serialization::decode(&serde_json::to_vec(&data["history_request"]).unwrap()).unwrap();
    let cancel = Cancellation::default();
    let report = families::analyze(&request, &cancel).unwrap();
    eigentrail_check::families::check(&request, &report, &cancel).unwrap();
    let value: Value = serde_json::from_slice(&serialization::encode(&report).unwrap()).unwrap();
    let cfg = &value["configurations"][0];
    assert_eq!(cfg["crossings"].as_array().unwrap().len(), 2);
    assert_eq!(cfg["crossings"][0]["outcome"]["kind"], "Resolved");
    assert_eq!(cfg["crossings"][1]["outcome"]["gate"], "StructuralPremise");
    assert_eq!(cfg["feasibility"]["kind"], "Unresolved");
    assert!(cfg.get("continuous_workflow").is_none());
    let mut bad = value.clone();
    bad["configurations"][0]["crossings"]
        .as_array_mut()
        .unwrap()
        .pop();
    let bad: families::HistoryReport =
        serialization::decode(&serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(eigentrail_check::families::check(&request, &bad, &cancel).is_err());
    let mut legacy = request;
    legacy.schema = families::SCHEMA.into();
    assert!(families::analyze(&legacy, &cancel).is_err());
}

#[test]
fn crossing_outputs_share_the_operation_budget_and_cancel_without_completion() {
    let (mut request, window) = inputs(&oracle()["cases"][0]);
    request.memory_bytes = 16 * 1024 * 1024;
    let operation = Operation::new(request.memory_bytes).unwrap();
    let baseline = operation.stats().live;
    let cancel = Cancellation::default();
    let result = operation
        .own(|| {
            let report = crossings::analyze(&request, &window, &cancel)?;
            eigentrail_check::crossings::check(&request, &window, &report, &cancel)?;
            let output = serialization::encode(&report)?;
            Ok((report, output))
        })
        .unwrap();
    assert!(operation.stats().live >= baseline + result.1.capacity());
    cancel.cancel();
    assert!(matches!(
        crossings::analyze(&request, &window, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        eigentrail_check::crossings::check(&request, &window, &result.0, &cancel),
        Err(Error::Cancelled)
    ));
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    request.memory_bytes = 8192;
    assert!(matches!(
        crossings::analyze(&request, &window, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
}
