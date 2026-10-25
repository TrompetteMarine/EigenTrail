//! Frozen public-schema history cases; expected values come from rational oracles.
use eigentrail_core::{families::*, resources::Cancellation, *};
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../fixtures/oracles/continuous_workflow.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn number(value: &Value) -> f64 {
    let text = value.as_str().expect("expected encoded or rational scalar");
    if let Some(bits) = text.strip_prefix("f64:") {
        return f64::from_bits(u64::from_str_radix(bits, 16).unwrap());
    }
    let mut parts = text.split('/');
    let numerator: f64 = parts.next().unwrap().parse().unwrap();
    numerator / parts.next().map_or(1., |v| v.parse::<f64>().unwrap())
}
fn encoded(value: f64) -> Value {
    Value::String(format!("f64:{:016x}", value.to_bits()))
}
fn produce(case: &Value) -> (HistoryRequest, Value) {
    let input = serde_json::to_vec(&case["request"]).unwrap();
    let request: HistoryRequest = serialization::decode(&input).unwrap();
    let cancel = Cancellation::default();
    let report = families::analyze(&request, &cancel).unwrap();
    eigentrail_check::families::check(&request, &report, &cancel).unwrap();
    let bytes = serialization::encode(&report).unwrap();
    let replay: HistoryReport = serialization::decode(&bytes).unwrap();
    eigentrail_check::families::check(&request, &replay, &cancel).unwrap();
    (request, serde_json::from_slice(&bytes).unwrap())
}
fn flow(config: &Value) -> &Value {
    assert_eq!(config["continuous_workflow"].as_array().unwrap().len(), 1);
    &config["continuous_workflow"][0]
}
fn blocks(query: &Value) -> &[Value] {
    assert_eq!(query["state"]["kind"], "Bounds");
    query["state"]["blocks"].as_array().unwrap()
}
fn close(range: &Value, lo: f64, hi: f64) {
    let lower = number(&range["lower"]);
    let upper = number(&range["upper"]);
    assert!(
        lower <= lo + 1e-10 && upper >= hi - 1e-10,
        "{range} excludes [{lo},{hi}]"
    );
    assert!(
        (lower - lo).abs() < 1e-8 && (upper - hi).abs() < 1e-8,
        "{range} does not attain [{lo},{hi}]"
    );
    assert_eq!(
        (
            range["lower_closed"].as_bool(),
            range["upper_closed"].as_bool()
        ),
        (Some(true), Some(true))
    );
}
fn origins(block: &Value, expected: &[usize], persistent: bool) {
    assert_eq!(block["origins"], serde_json::json!(expected));
    assert_eq!(block["persistent"], persistent);
}
fn rejected(request: &HistoryRequest, bad: Value) {
    let bytes = serde_json::to_vec(&bad).unwrap();
    let decoded = serialization::decode::<HistoryReport>(&bytes);
    assert!(
        decoded.is_err()
            || eigentrail_check::families::check(
                request,
                &decoded.unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn forced_continuous_queries_and_all_configuration_scopes_are_public() {
    let case = &cases()[0];
    let (_, report) = produce(case);
    assert_eq!(report["schema"], "eigentrail.histories.v2");
    let configurations = report["configurations"].as_array().unwrap();
    assert_eq!(configurations.len(), 2);
    let workflow = flow(&configurations[0]);
    let queries = workflow["queries"].as_array().unwrap();
    assert_eq!(queries.len(), 3);
    for (row, expected) in queries
        .iter()
        .zip(case["expectation"]["strengths"].as_array().unwrap())
    {
        let b = blocks(row);
        assert_eq!(b.len(), 1);
        let lo = number(&expected[0]);
        close(&b[0]["strength"], lo, lo);
        close(&row["state"]["trace"], lo, lo);
        assert_eq!(b[0]["share"]["kind"], "Interval");
        close(&b[0]["share"]["value"], 1., 1.);
        assert_eq!(b[0]["rank"], serde_json::json!([1, 1]));
        origins(&b[0], &[0], true);
        for kind in ["reference_displacement", "local_displacement"] {
            assert_eq!(b[0][kind]["kind"], "Interval");
            close(&b[0][kind]["value"], 0., 0.);
        }
    }
    assert_eq!(
        configurations[0]["feasibility"]["kind"],
        "OriginalAffineWitness"
    );
    assert_eq!(workflow["witnesses"].as_array().unwrap().len(), 1);
    assert_eq!(configurations[1]["feasibility"]["kind"], "Unresolved");
    let slow = flow(&configurations[1]);
    assert!(slow["witnesses"].as_array().unwrap().is_empty());
    assert!(
        slow["queries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["state"]["kind"] == "InconsistentOuterBounds")
    );
}

#[test]
fn complete_competitors_preserve_ambiguity_and_no_matching() {
    for index in [1, 2] {
        let case = &cases()[index];
        let (request, report) = produce(case);
        let cfg = &report["configurations"][0];
        let segment = &cfg["continuous_conditional"]["segments"][0];
        assert_eq!(segment["edges"], case["expectation"]["edges"]);
        assert_eq!(
            segment["association"]["kind"],
            case["expectation"]["association"]
        );
        let workflow = flow(cfg);
        let final_query = workflow["queries"].as_array().unwrap().last().unwrap();
        for b in blocks(final_query) {
            assert_eq!(b["origins"], case["expectation"]["final_origins"]);
            assert_eq!(b["persistent"], false);
            assert_eq!(b["reference_displacement"]["kind"], "Undefined");
        }
        if index == 2 {
            assert_eq!(cfg["feasibility"]["kind"], "Unresolved");
            assert!(workflow["witnesses"].as_array().unwrap().is_empty());
        }
        for mutation in 0..4 {
            let mut bad = report.clone();
            match mutation {
                0 => {
                    bad["configurations"][0]["continuous_conditional"]["segments"][0]["edges"][0] =
                        serde_json::json!([0])
                }
                1 => {
                    bad["configurations"][0]["continuous_conditional"]["segments"][0]["association"] =
                        serde_json::json!({"kind":"Unique","matching":[0,1]})
                }
                2 => {
                    bad["configurations"][0]["continuous_conditional"]["segments"][0]["tubes"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                _ => {
                    let last = bad["configurations"][0]["continuous_workflow"][0]["queries"]
                        .as_array_mut()
                        .unwrap()
                        .last_mut()
                        .unwrap();
                    last["state"]["blocks"][0]["origins"] = serde_json::json!([0]);
                    last["state"]["blocks"][0]["persistent"] = Value::Bool(true);
                }
            }
            rejected(&request, bad);
        }
    }
}

#[test]
fn block_merges_and_splits_never_restore_internal_identity() {
    let case = &cases()[3];
    let (request, report) = produce(case);
    let cfg = &report["configurations"][0];
    let segments = cfg["continuous_conditional"]["segments"]
        .as_array()
        .unwrap();
    for (segment, expected) in segments
        .iter()
        .zip(case["expectation"]["partitions"].as_array().unwrap())
    {
        let actual: Vec<_> = segment["tubes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| serde_json::json!([t["start"], t["end"]]))
            .collect();
        assert_eq!(Value::Array(actual), *expected);
    }
    let overlaps = cfg["partition_overlaps"].as_array().unwrap();
    assert_eq!(overlaps.len(), 6);
    for date in [1, 2] {
        let at_date: Vec<_> = overlaps.iter().filter(|o| o["date"] == date).collect();
        assert_eq!(at_date.len(), 3);
        assert!(at_date.iter().all(|o| o["rank"] == 1));
    }
    let workflow = flow(cfg);
    let final_query = workflow["queries"].as_array().unwrap().last().unwrap();
    assert_eq!(blocks(final_query).len(), 3);
    for b in blocks(final_query) {
        origins(b, &[0, 1, 2], false);
        assert_eq!(b["reference_displacement"]["kind"], "Undefined");
    }
    let mut bad = report.clone();
    bad["configurations"][0]["partition_overlaps"]
        .as_array_mut()
        .unwrap()
        .pop();
    rejected(&request, bad);
    let mut bad = report.clone();
    let last = bad["configurations"][0]["continuous_workflow"][0]["queries"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["state"]["blocks"][0]["origins"] = serde_json::json!([0]);
    last["state"]["blocks"][0]["persistent"] = Value::Bool(true);
    rejected(&request, bad);
}

#[test]
fn pointwise_boxes_do_not_certify_incompatible_joint_selections() {
    let case = &cases()[4];
    let (request, report) = produce(case);
    let workflow = flow(&report["configurations"][0]);
    for (row, expected) in workflow["queries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(case["expectation"]["strengths"].as_array().unwrap())
    {
        close(
            &blocks(row)[0]["strength"],
            number(&expected[0]),
            number(&expected[1]),
        );
    }
    let witness = &workflow["witnesses"][0];
    assert_eq!(witness["feasibility"]["kind"], "OriginalAffineWitness");
    assert_eq!(witness["values"].as_array().unwrap().len(), 2);
    for value in witness["values"].as_array().unwrap() {
        assert_eq!(number(&value["data"][0]), 1.);
    }
    let mut bad = report.clone();
    let witness = &mut bad["configurations"][0]["continuous_workflow"][0]["witnesses"][0];
    witness["values"][0]["data"][0] = encoded(0.75);
    witness["values"][1]["data"][0] = encoded(1.5);
    rejected(&request, bad);
}

#[test]
fn widened_witness_interiors_remain_outside_original_family_bounds() {
    let case = &cases()[5];
    let (request, report) = produce(case);
    let cfg = &report["configurations"][0];
    assert_eq!(cfg["feasibility"]["kind"], "WidenedAffineWitness");
    assert_eq!(number(&cfg["feasibility"]["additional"]), 0.125);
    let workflow = flow(cfg);
    close(
        &blocks(&workflow["queries"][0])[0]["strength"],
        0.375,
        0.625,
    );
    assert_eq!(
        number(&workflow["witnesses"][0]["values"][0]["data"][0]),
        0.75
    );
    assert_eq!(
        workflow["witnesses"][0]["feasibility"]["kind"],
        "WidenedAffineWitness"
    );
    for radius in cfg["continuous_conditional"]["radii"].as_array().unwrap() {
        assert_eq!(number(radius), 0.125);
    }
    assert_eq!(
        report["configurations"][1]["feasibility"]["kind"],
        "Unresolved"
    );
    assert!(
        flow(&report["configurations"][1])["witnesses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut bad = report.clone();
    bad["configurations"][0]["feasibility"] = serde_json::json!({"kind":"OriginalAffineWitness"});
    bad["configurations"][0]["continuous_workflow"][0]["witnesses"][0]["feasibility"] =
        serde_json::json!({"kind":"OriginalAffineWitness"});
    rejected(&request, bad);
}

#[test]
fn local_and_reference_displacements_are_checked_separately() {
    let case = &cases()[6];
    let (_, report) = produce(case);
    let workflow = flow(&report["configurations"][0]);
    let queries = workflow["queries"].as_array().unwrap();
    assert_eq!(queries.len(), 17);
    let truth = number(&case["expectation"]["local_displacement_truth"]);
    for row in &queries[1..] {
        let b = &blocks(row)[0];
        origins(b, &[0], true);
        let local = &b["local_displacement"];
        assert_eq!(local["kind"], "Interval");
        assert!(
            number(&local["value"]["lower"]) <= truth && number(&local["value"]["upper"]) >= truth
        );
        assert!(number(&local["value"]["upper"]) < 0.2);
    }
    let reference = &blocks(queries.last().unwrap())[0]["reference_displacement"];
    assert_eq!(reference["kind"], "Interval");
    assert!(number(&reference["value"]["lower"]) > 0.99);
    let truth = number(&case["expectation"]["final_reference_truth"]);
    assert!(
        number(&reference["value"]["lower"]) <= truth
            && number(&reference["value"]["upper"]) >= truth
    );
}

#[test]
fn independent_checker_rejects_interior_and_zero_residual_wrong_branch_data() {
    let (request, report) = produce(&cases()[0]);
    for mutation in 0..4 {
        let mut bad = report.clone();
        let workflow = &mut bad["configurations"][0]["continuous_workflow"][0];
        match mutation {
            0 => workflow["queries"][1]["centre"]["eigensystem"]["values"][0] = encoded(3.5),
            1 => workflow["queries"][1]["centre"]["time"] = encoded(0.75),
            2 => workflow["witnesses"][0]["evaluation_errors"][1] = encoded(-1.),
            _ => {
                workflow["queries"].as_array_mut().unwrap().remove(1);
            }
        }
        rejected(&request, bad);
    }
    let (request, report) = produce(&cases()[1]);
    let mut bad = report.clone();
    let b =
        &mut bad["configurations"][0]["continuous_workflow"][0]["queries"][0]["state"]["blocks"][0];
    // The full centre eigensystem remains exact; only the claimed branch is changed.
    b["start"] = serde_json::json!(1);
    b["end"] = serde_json::json!(2);
    b["strength"]["lower"] = encoded(1.);
    b["strength"]["upper"] = encoded(1.);
    rejected(&request, bad);
}
