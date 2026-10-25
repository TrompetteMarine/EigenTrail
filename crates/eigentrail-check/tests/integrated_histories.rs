//! Frozen rational jump-query cases; independent complete-history and common-event expectations.
use eigentrail_core::{families::*, resources::Cancellation, *};
use serde_json::{Value, json};

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/integrated_histories.json"
    ))
    .unwrap()
}
fn number(value: &Value) -> f64 {
    let text = value.as_str().unwrap();
    if let Some(bits) = text.strip_prefix("f64:") {
        return f64::from_bits(u64::from_str_radix(bits, 16).unwrap());
    }
    text.split_once('/').map_or_else(
        || text.parse().unwrap(),
        |(a, b)| a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap(),
    )
}
fn encoded(value: f64) -> Value {
    Value::String(format!("f64:{:016x}", value.to_bits()))
}
fn produce(case: &Value) -> (HistoryRequest, Value) {
    let request: HistoryRequest =
        serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap();
    let cancel = Cancellation::default();
    let report = families::analyze(&request, &cancel).unwrap();
    eigentrail_check::families::check(&request, &report, &cancel).unwrap();
    let bytes = serialization::encode(&report).unwrap();
    let replay: HistoryReport = serialization::decode(&bytes).unwrap();
    eigentrail_check::families::check(&request, &replay, &cancel).unwrap();
    (request, serde_json::from_slice(&bytes).unwrap())
}
fn rejected(request: &HistoryRequest, value: Value) {
    let result = serialization::decode::<HistoryReport>(&serde_json::to_vec(&value).unwrap());
    assert!(
        result.is_err()
            || eigentrail_check::families::check(
                request,
                &result.unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
}
fn bounds(actual: &Value, expected: &Value, tolerance: f64) {
    let lo = number(&actual["lower"]);
    let hi = number(&actual["upper"]);
    let elo = number(&expected[0]);
    let ehi = number(&expected[1]);
    assert!(
        lo <= elo + tolerance && hi >= ehi - tolerance,
        "{actual} excludes {expected}"
    );
    assert!(
        (lo - elo).abs() < tolerance && (hi - ehi).abs() < tolerance,
        "{actual} does not attain {expected}"
    );
    assert_eq!(actual["lower_closed"], true);
    assert_eq!(actual["upper_closed"], true);
}
fn mode_bounds(modes: &[Value], expected: &Value, tolerance: f64) {
    let truth = expected.as_array().unwrap();
    assert_eq!(modes.len(), truth.len());
    for (index, (mode, range)) in modes.iter().zip(truth).enumerate() {
        bounds(&mode["strength"], range, tolerance);
        assert_eq!(mode["rank"], json!([index + 1, index + 1]));
    }
}
fn assert_case(case: &Value, request: &HistoryRequest, report: &Value) {
    let tolerance = number(&case["expectation"]["numeric_tolerance"]);
    let configurations = report["configurations"].as_array().unwrap();
    let expected = case["expectation"]["configurations"].as_array().unwrap();
    assert_eq!(configurations.len(), expected.len());
    let common = &configurations[0]["continuous_conditional"];
    for (cfg, truth) in configurations.iter().zip(expected) {
        assert_eq!(cfg["id"], truth["id"]);
        assert_eq!(cfg["class"]["kind"], truth["class"]);
        assert_eq!(cfg["feasibility"]["kind"], truth["feasibility"]);
        assert_eq!(cfg["continuous_conditional"]["radii"], common["radii"]);
        assert_eq!(
            cfg["continuous_conditional"]["calibration_scope"],
            common["calibration_scope"]
        );
        if truth["class"] != "Jumps" {
            assert!(
                cfg.get("jump_workflow")
                    .is_none_or(|v| v.as_array().unwrap().is_empty())
            );
            if let Some(strengths) = truth.get("continuous_strengths") {
                let rows = cfg["continuous_workflow"][0]["queries"].as_array().unwrap();
                for (row, strength) in rows.iter().zip(strengths.as_array().unwrap()) {
                    bounds(&row["state"]["blocks"][0]["strength"], strength, tolerance);
                }
            }
            if let Some(expected) = truth.get("common_frame") {
                assert_eq!(cfg["common_frame"][0]["kind"], *expected);
            }
            if let Some(gate) = truth.get("crossing_gate") {
                assert_eq!(cfg["crossings"][0]["outcome"]["kind"], "Unresolved");
                assert_eq!(cfg["crossings"][0]["outcome"]["gate"], *gate);
            }
            if let Some(geometry) = truth.get("geometry") {
                assert_eq!(cfg["transports"][0]["outcome"]["kind"], *geometry);
                assert_eq!(
                    cfg["transports"][0]["covariance"][0]["outcome"]["kind"],
                    truth["covariance"]
                );
            }
            continue;
        }
        assert_eq!(cfg["jumps"][0]["family"]["mandatory"], truth["mandatory"]);
        assert_eq!(cfg["jumps"][0]["family"]["optional"], truth["optional"]);
        let flows = cfg["jump_workflow"]
            .as_array()
            .expect("v2 jump workflow required");
        assert_eq!(flows.len(), 1);
        let flow = &flows[0];
        let components = flow["components"].as_array().unwrap();
        assert_eq!(
            components.len(),
            truth["components"].as_array().unwrap().len()
        );
        for (component, expected) in components
            .iter()
            .zip(truth["components"].as_array().unwrap())
        {
            assert_eq!(component["cells"], *expected);
            assert_eq!(
                component["lineage"].as_array().unwrap().len(),
                (expected[1].as_u64().unwrap() - expected[0].as_u64().unwrap()) as usize
            );
        }
        let rows = flow["queries"].as_array().unwrap();
        assert_eq!(rows.len(), request.query_times.len());
        assert_eq!(rows.len(), truth["queries"].as_array().unwrap().len());
        for ((row, query), time) in rows
            .iter()
            .zip(truth["queries"].as_array().unwrap())
            .zip(case["request"]["query_times"].as_array().unwrap())
        {
            assert_eq!(row["time"], *time);
            let state = &row["state"];
            assert_eq!(state["kind"], query["kind"]);
            for key in ["date", "component", "cell", "mandatory"] {
                if let Some(expected) = query.get(key) {
                    assert_eq!(state[key], *expected);
                }
            }
            if state["kind"] == "Smooth" {
                let nested = &state["query"];
                assert_eq!(nested["time"], row["time"]);
                assert_eq!(nested["centre"]["segment"], query["segment"]);
                assert_eq!(
                    nested["centre"]["request_hash"],
                    cfg["continuous_conditional"]["request_hash"]
                );
                assert_eq!(nested["state"]["kind"], "Bounds");
                bounds(&nested["state"]["trace"], &query["trace"], tolerance);
                let modes = nested["state"]["blocks"].as_array().unwrap();
                mode_bounds(modes, &query["strengths"], tolerance);
                for (i, mode) in modes.iter().enumerate() {
                    assert_eq!(mode["origins"], json!([i]));
                    assert_eq!(mode["persistent"], true);
                    if query["reference_displacement_zero"] == true {
                        assert_eq!(mode["reference_displacement"]["kind"], "Interval");
                        bounds(
                            &mode["reference_displacement"]["value"],
                            &json!(["0", "0"]),
                            tolerance,
                        );
                    }
                }
            } else {
                let actual_sides: Vec<&Value> = if state["kind"] == "Observed" {
                    vec![state]
                } else {
                    state["sides"].as_array().unwrap().iter().collect()
                };
                let expected_sides: Vec<&Value> = if state["kind"] == "Observed" {
                    vec![query]
                } else {
                    query["sides"].as_array().unwrap().iter().collect()
                };
                assert_eq!(actual_sides.len(), expected_sides.len());
                for (side, expected) in actual_sides.iter().zip(expected_sides) {
                    assert_eq!(side["date"], expected["date"]);
                    if let Some(radius) = expected.get("radius") {
                        assert!((number(&side["radius"]) - number(radius)).abs() < tolerance);
                    }
                    bounds(&side["trace"], &expected["trace"], tolerance);
                    mode_bounds(
                        side["modes"].as_array().unwrap(),
                        &expected["strengths"],
                        tolerance,
                    );
                    if number(&side["trace"]["upper"]) == 0. {
                        assert!(
                            side["modes"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|m| m["share"]["kind"] == "Undefined")
                        );
                    }
                }
            }
        }
        let witnesses = flow["witnesses"].as_array().unwrap();
        if let Some(witness) = truth.get("witness") {
            assert_eq!(witnesses.len(), 1);
            assert_eq!(witnesses[0], *witness);
        } else {
            assert!(witnesses.is_empty());
        }
    }
    if let Some(radii) = case["expectation"].get("global_radii") {
        assert_eq!(case["expectation"]["global_dates"], 4);
        for (value, interval) in common["radii"]
            .as_array()
            .unwrap()
            .iter()
            .zip(radii.as_array().unwrap())
        {
            assert!(number(value) >= number(&interval[0]) - tolerance);
            assert!(number(value) <= number(&interval[1]) + tolerance);
        }
    }
}

#[test]
fn all_eight_independent_jump_queries_and_configuration_unions_are_checked() {
    for case in oracle()["cases"].as_array().unwrap() {
        let (request, report) = produce(case);
        assert_case(case, &request, &report);
    }
}

#[test]
fn full_union_sides_components_and_complete_step_witness_cannot_be_pruned_or_forged() {
    let data = oracle();
    let (request, report) = produce(&data["cases"][0]);
    for mutation in 0..12 {
        let mut bad = report.clone();
        let cfg = &mut bad["configurations"][0];
        match mutation {
            0 => {
                cfg["jump_workflow"] = json!([]);
            }
            1 => {
                cfg["jump_workflow"][0]["queries"][0]["state"]["sides"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            2 => {
                cfg["jump_workflow"][0]["queries"][0]["state"]["sides"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            }
            3 => {
                cfg["jump_workflow"][0]["queries"][0]["state"]["sides"][0]["modes"][0]["strength"]
                    ["upper"] = encoded(1.);
            }
            4 => {
                cfg["jump_workflow"][0]["queries"][0]["state"]["mandatory"] = json!(false);
            }
            5 => {
                cfg["jump_workflow"][0]["components"][0]["cells"] = json!([0, 2]);
            }
            6 => {
                cfg["jump_workflow"][0]["queries"][2]["state"]["query"]["centre"]["segment"] =
                    json!(0);
            }
            7 => {
                cfg["jump_workflow"][0]["witnesses"][0]["cells"] = json!([]);
            }
            8 => {
                cfg["jump_workflow"][0]["witnesses"][0]["values"][1]["data"][0] = encoded(2.);
            }
            9 => {
                cfg["jump_workflow"][0]["scope"] =
                    json!("every pointwise selection is realizable and jump time known");
            }
            10 => {
                cfg["feasibility"] = json!({"kind":"OriginalAffineWitness"});
            }
            _ => {
                cfg["jump_workflow"][0]["queries"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            }
        }
        rejected(&request, bad);
    }
    let (request, report) = produce(&data["cases"][4]);
    let mut bad = report.clone();
    bad["configurations"][0]["jump_workflow"][0]["queries"][2]["state"]["query"]["state"]["blocks"]
        [0]["reference_displacement"]["value"]["upper"] = encoded(1.);
    rejected(&request, bad);
}

#[test]
fn marginal_membership_does_not_make_one_jump_history_or_identify_jump_time() {
    let data = oracle();
    let (request, report) = produce(&data["cases"][1]);
    // Endpoint .875 -> selected 1.25 -> selected .875 -> endpoint 1.25 requires three jumps.
    let values = [7. / 8., 5. / 4., 7. / 8., 5. / 4.];
    assert_eq!(values.windows(2).filter(|w| w[0] != w[1]).count(), 3);
    assert!(values.iter().all(|v| 7. / 8. <= *v && *v <= 5. / 4.));
    let mut bad = report.clone();
    bad["configurations"][0]["jump_workflow"][0]["witnesses"][0]["values"][0]["data"][0] =
        encoded(5. / 4.);
    bad["configurations"][0]["jump_workflow"][0]["witnesses"][0]["values"][1]["data"][0] =
        encoded(7. / 8.);
    rejected(&request, bad);
    let query = 0.5;
    assert!(1. / 3. < query && query < 2. / 3.);
    assert_eq!((3. - 2.) / 2., 0.5); // Two admissible switch times have different query values.
    let mut outside = data["cases"][3]["request"].clone();
    outside["configurations"][0]["class"]["model"]["at_most_one_per_cell"] = json!(false);
    let request: HistoryRequest =
        serialization::decode(&serde_json::to_vec(&outside).unwrap()).unwrap();
    assert!(families::analyze(&request, &Cancellation::default()).is_err());
}

#[test]
fn candidate_minimum_eligibility_and_legacy_supplied_affine_scope_stay_distinct() {
    let data = oracle();
    let (request, report) = produce(&data["cases"][2]);
    for index in [1, 2] {
        let mut bad = report.clone();
        bad["configurations"][index]["feasibility"] = json!({"kind":"OriginalStepWitness"});
        bad["configurations"][index]["jump_workflow"][0]["witnesses"] =
            report["configurations"][0]["jump_workflow"][0]["witnesses"].clone();
        rejected(&request, bad);
    }
    let mut supplied = data["cases"][0].clone();
    supplied["request"]["configurations"][0]["witnesses"] = json!([{"nodes":supplied["request"]["observations"]["covariances"],"widening":encoded(0.)}]);
    let (_, report) = produce(&supplied);
    assert_eq!(
        report["configurations"][0]["feasibility"]["kind"],
        "UnsupportedModel"
    );
    assert!(
        report["configurations"][0]["jump_workflow"][0]["witnesses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut legacy = data["cases"][0].clone();
    legacy["request"]["schema"] = json!("eigentrail.histories.v1");
    let (request, report) = produce(&legacy);
    assert!(report["configurations"][0].get("jump_workflow").is_none());
    assert_eq!(
        report["configurations"][0]["feasibility"]["kind"],
        "Unresolved"
    );
    let mut bad = report.clone();
    bad["configurations"][0]["jump_workflow"] = json!([{}]);
    rejected(&request, bad);
}

#[test]
fn no_queries_still_retains_smooth_components_and_zero_step_witness() {
    let mut case = oracle()["cases"][6].clone();
    case["request"]["query_times"] = json!([]);
    let (_, report) = produce(&case);
    let flow = &report["configurations"][1]["jump_workflow"][0];
    assert_eq!(flow["components"][0]["cells"], json!([0, 2]));
    assert!(flow["queries"].as_array().unwrap().is_empty());
    assert_eq!(flow["witnesses"], json!([{"cells":[],"values":[]}]));
}

#[test]
fn signed_zero_observation_precedence_cancellation_and_refusal_are_explicit() {
    let mut case = oracle()["cases"][0].clone();
    case["request"]["query_times"] = json!([encoded(-0.), encoded(1.)]);
    let (request, report) = produce(&case);
    let rows = report["configurations"][0]["jump_workflow"][0]["queries"]
        .as_array()
        .unwrap();
    assert_eq!(rows[0]["time"], encoded(-0.));
    assert_eq!(rows[0]["state"]["kind"], "Observed");
    assert_eq!(rows[0]["state"]["date"], 0);
    assert_eq!(rows[1]["state"]["date"], 1);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        families::analyze(&request, &cancel),
        Err(Error::Cancelled)
    ));
    let typed: HistoryReport =
        serialization::decode(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert!(matches!(
        eigentrail_check::families::check(&request, &typed, &cancel),
        Err(Error::Cancelled)
    ));
    let mut tiny = request;
    tiny.observations.memory_bytes = 4096;
    assert!(matches!(
        families::analyze(&tiny, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
}
