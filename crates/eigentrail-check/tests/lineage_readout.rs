//! Prospective readout acceptance: exact analytic fixtures, no producer-generated gold.
use eigentrail_core::{
    Error, Quantity, Range,
    families::{self, HistoryRequest, PathClass},
    lineage_summary::{self as readout, Motion, Report, Request},
    resources::Cancellation,
    serialization,
};
use serde_json::{Value, json};

fn oracles() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/lineage_readout_oracles.json"
    ))
    .unwrap()
}

fn input(name: &str) -> Request {
    let data = oracles();
    let case = data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    Request {
        schema: "eigentrail.lineage-summary.v1".into(),
        history: serialization::decode(&serde_json::to_vec(&case["history"]).unwrap()).unwrap(),
        delta: 0.05,
    }
}

fn number(value: &Value) -> f64 {
    let text = value.as_str().expect("canonical bit string required");
    f64::from_bits(u64::from_str_radix(text.strip_prefix("f64:").unwrap(), 16).unwrap())
}

fn rational(text: &str) -> f64 {
    let mut parts = text.split('/');
    parts.next().unwrap().parse::<f64>().unwrap()
        / parts.next().map_or(1., |v| v.parse::<f64>().unwrap())
}

fn encoded(value: f64) -> Value {
    Value::String(format!("f64:{:016x}", value.to_bits()))
}

fn produce(request: &Request) -> (Report, Value) {
    let cancel = Cancellation::default();
    let report = readout::analyze(request, &cancel).unwrap();
    eigentrail_check::lineage_summary::check(request, &report, &cancel).unwrap();
    let bytes = serialization::encode(&report).unwrap();
    let replay: Report = serialization::decode(&bytes).unwrap();
    eigentrail_check::lineage_summary::check(request, &replay, &cancel).unwrap();
    (report, serde_json::from_slice(&bytes).unwrap())
}

fn rows(report: &Value) -> &[Value] {
    report["rows"].as_array().unwrap()
}

fn state(row: &Value) -> &Value {
    assert_eq!(row["state"]["kind"], "Bounds");
    &row["state"]
}

fn leading(report: &Value, query: usize) -> &Value {
    rows(report)
        .iter()
        .find(|row| {
            row["configuration"] == 0
                && row["query"] == query
                && row["state"]["kind"] == "Bounds"
                && row["state"]["target"]["kind"] == "OrderedBlock"
                && row["state"]["target"]["start"] == 0
        })
        .unwrap()
}

fn contains(range: &Value, truth: f64, maximum_width: f64) {
    let lower = number(&range["lower"]);
    let upper = number(&range["upper"]);
    assert!(lower <= truth && truth <= upper, "{range} excludes {truth}");
    assert!(upper - lower < maximum_width, "{range} is uninformative");
    assert_eq!(range["lower_closed"], true);
    assert_eq!(range["upper_closed"], true);
}

fn rejected(request: &Request, bad: Value) {
    let decoded = serialization::decode::<Report>(&serde_json::to_vec(&bad).unwrap());
    assert!(
        decoded.is_err()
            || eigentrail_check::lineage_summary::check(
                request,
                &decoded.unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn constant_line_and_proper_plane_are_informative_without_internal_axis_labels() {
    for (name, p, q, count, strength, share, ranks) in [
        ("constant_line3", 3, 1, 9, 4., 16. / 21., [1, 1]),
        ("constant_plane5", 5, 2, 12, 8., 32. / 39., [1, 2]),
    ] {
        let request = input(name);
        let (_, report) = produce(&request);
        assert_eq!(rows(&report).len(), count);
        for query in 0..3 {
            let item = state(leading(&report, query));
            assert_eq!(item["ambient_dimension"], p);
            assert_eq!(item["block_dimension"], q);
            assert_eq!(
                item["information_level"],
                if q == 1 { "Line" } else { "ProperSubspace" }
            );
            assert_eq!(item["rank"], json!(ranks));
            assert_eq!(item["target"]["start"], 0);
            assert_eq!(item["target"]["end"], q);
            assert_eq!(item["target"]["origins"], json!([0]));
            assert_eq!(item["target"]["persistent"], true);
            assert_eq!(item["accuracy_target_attained"], true);
            assert!(number(&item["projector_error"]) < 0.002);
            contains(&item["strength"], strength, 1e-8);
            assert_eq!(item["share"]["kind"], "Interval");
            contains(&item["share"]["value"], share, 1e-8);
            assert_eq!(item["reference_displacement"]["kind"], "Interval");
            contains(&item["reference_displacement"]["value"], 0., 1e-8);
            assert_eq!(item["motion"], "WithinTolerance");
        }
        // A rank-two target remains one block, never two arbitrary anchor axes.
        assert!(
            rows(&report)
                .iter()
                .all(|row| state(row)["target"]["kind"] == "OrderedBlock")
        );
    }
}

#[test]
fn rotation_uses_129_actual_observations_and_certifies_nontrivial_reference_motion() {
    let request = input("rotating_line3");
    assert_eq!(request.history.observations.times.len(), 129);
    assert_eq!(request.history.observations.lipschitz, 3. / 8.);
    assert_eq!(request.history.query_times, vec![0., 0.5, 1.]);
    let (_, report) = produce(&request);
    assert_eq!(rows(&report).len(), 9);
    let endpoint = state(leading(&report, 2));
    assert_eq!(endpoint["information_level"], "Line");
    assert_eq!(endpoint["motion"], "BeyondTolerance");
    assert_eq!(endpoint["accuracy_target_attained"], true);
    assert_eq!(endpoint["reference_displacement"]["kind"], "Interval");
    let range = &endpoint["reference_displacement"]["value"];
    let data = oracles();
    let truth = &data["rotation_endpoint_displacement"];
    assert!(number(&range["lower"]) <= rational(truth["lower"].as_str().unwrap()));
    assert!(number(&range["upper"]) >= rational(truth["upper"].as_str().unwrap()));
    assert!(number(&range["lower"]) > 0.05);
    assert!(number(&range["upper"]) <= 1.);
}

#[test]
fn hidden_ancestry_same_observations_have_same_uninformative_readout() {
    let request = input("hidden_shared_observations3");
    let (_, first) = produce(&request);
    let (_, second) = produce(&request);
    assert_eq!(first, second);
    assert_eq!(rows(&first).len(), 37);
    assert_eq!(
        first["history"]["configurations"][0]["feasibility"]["kind"],
        "OriginalAffineWitness"
    );
    for row in rows(&first) {
        let item = state(row);
        assert_eq!(item["information_level"], "WholeSpace");
        assert_eq!(item["block_dimension"], 3);
        assert_eq!(item["motion"], "Unresolved");
        assert_eq!(item["accuracy_target_attained"], false);
        contains(&item["reference_displacement"]["value"], 0., 1e-8);
    }
}

#[test]
fn whole_space_override_includes_one_dimensional_ambient_and_undefined_share() {
    for name in ["scalar_ambient1", "zero_trace3"] {
        let (_, report) = produce(&input(name));
        assert_eq!(rows(&report).len(), 3);
        for row in rows(&report) {
            let item = state(row);
            assert_eq!(item["information_level"], "WholeSpace");
            assert_eq!(item["motion"], "Unresolved");
            assert_eq!(item["accuracy_target_attained"], false);
            if name == "zero_trace3" {
                assert_eq!(item["share"]["kind"], "Undefined");
            }
        }
    }
}

#[test]
fn exact_and_avoided_crossings_do_not_create_internal_branch_or_collision_claims() {
    for name in ["exact_crossing3", "avoided_crossing3"] {
        let request = input(name);
        let (_, report) = produce(&request);
        assert_eq!(rows(&report).len(), 6);
        let item = state(leading(&report, 1));
        assert_eq!(item["target"]["start"], 0);
        assert_eq!(item["target"]["end"], 2);
        assert_eq!(item["information_level"], "ProperSubspace");
        assert_eq!(item["rank"], json!([1, 2]));
        assert_eq!(item["accuracy_target_attained"], false);
        assert_eq!(
            report["history"]["configurations"][0]["class"]["kind"],
            "Continuous"
        );
        assert!(
            report["history"]["configurations"][0]
                .get("crossings")
                .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
        );
    }
    // Exact analytic overlaps identify different targets, not an estimator winner.
    assert_eq!(
        oracles()["avoided_overlap"]["same_spectral_branch"],
        "16/41"
    );
    assert_eq!(oracles()["avoided_overlap"]["other_active_branch"], "25/41");
    assert!(16. / 41. < 25. / 41.);
}

#[test]
fn common_frame_anchor_axis_can_change_rank_without_becoming_a_generating_label() {
    let (_, report) = produce(&input("common_frame_exchange3"));
    assert_eq!(rows(&report).len(), 9);
    let axis = |query: usize| {
        rows(&report)
            .iter()
            .find(|row| row["query"] == query && row["state"]["target"]["axis"] == 0)
            .unwrap()
    };
    assert_eq!(state(axis(0))["rank"], json!([1, 1]));
    assert_eq!(state(axis(2))["rank"], json!([2, 2]));
    assert_eq!(state(axis(1))["rank"], json!([1, 2]));
    for row in rows(&report) {
        let item = state(row);
        assert_eq!(item["target"]["kind"], "AnchorAxis");
        assert!(item["target"].get("origins").is_none());
        assert!(item["target"].get("persistent").is_none());
        assert_eq!(item["motion"], "WithinTolerance");
        contains(
            &item["reference_displacement"]["value"],
            0.,
            f64::MIN_POSITIVE,
        );
    }
}

#[test]
fn common_frame_zero_motion_does_not_imply_an_accurate_axis_estimate() {
    let request = input("common_frame_imprecise_axis2");
    let (_, report) = produce(&request);
    assert_eq!(rows(&report).len(), 6);
    for row in rows(&report) {
        let item = state(row);
        assert_eq!(item["information_level"], "Line");
        assert_eq!(item["motion"], "WithinTolerance");
        assert_eq!(item["accuracy_target_attained"], false);
        assert!(number(&item["projector_error"]) > 0.002);
    }
    let mut bad = report;
    bad["rows"][0]["state"]["accuracy_target_attained"] = json!(true);
    rejected(&request, bad);
}

#[test]
fn unresolved_anchors_and_inconsistent_bounds_emit_one_explicit_row_per_query() {
    for (name, expected) in [
        ("common_frame_unresolved_anchor5", "UnresolvedAnchor"),
        ("inconsistent_outer3", "InconsistentOuterBounds"),
    ] {
        let request = input(name);
        let (_, report) = produce(&request);
        assert_eq!(rows(&report).len(), 3);
        for (index, row) in rows(&report).iter().enumerate() {
            assert_eq!(row["configuration"], 0);
            assert_eq!(row["query"], index);
            assert_eq!(number(&row["time"]), request.history.query_times[index]);
            assert_eq!(row["state"]["kind"], expected);
            assert!(row["state"].get("target").is_none());
        }
    }
}

#[test]
fn widened_unlabelled_witness_existence_does_not_repair_original_outer_bounds() {
    let request = input("widened_witness3");
    let (_, report) = produce(&request);
    assert_eq!(
        report["history"]["configurations"][0]["feasibility"]["kind"],
        "WidenedAffineWitness"
    );
    assert!(
        rows(&report)
            .iter()
            .all(|row| row["state"]["kind"] == "InconsistentOuterBounds")
    );
    let mut bad = report;
    bad["history"]["configurations"][0]["feasibility"] = json!({"kind":"OriginalAffineWitness"});
    rejected(&request, bad);
}

#[test]
fn all_configurations_queries_duplicates_and_target_order_are_retained() {
    let request = input("two_configs_duplicate_queries3");
    let (_, report) = produce(&request);
    assert_eq!(rows(&report).len(), 18);
    for configuration in 0..2 {
        for query in 0..3 {
            for target in 0..3 {
                let row = &rows(&report)[configuration * 9 + query * 3 + target];
                assert_eq!(row["configuration"], configuration);
                assert_eq!(row["query"], query);
                assert_eq!(number(&row["time"]), request.history.query_times[query]);
                let item = state(row);
                if configuration == 0 {
                    assert_eq!(item["target"]["kind"], "OrderedBlock");
                    assert_eq!(item["target"]["start"], target);
                } else {
                    assert_eq!(item["target"]["kind"], "AnchorAxis");
                    assert_eq!(item["target"]["axis"], target);
                }
            }
        }
    }
    let mut bad = report.clone();
    bad["rows"].as_array_mut().unwrap().pop();
    rejected(&request, bad);
    let mut bad = report.clone();
    bad["rows"].as_array_mut().unwrap().swap(0, 3);
    rejected(&request, bad);
    let mut bad = report;
    bad["rows"][0]["query"] = json!(1);
    rejected(&request, bad);
}

#[test]
fn reference_time_is_first_observation_not_first_requested_query() {
    let mut request = input("rotating_line3");
    request.history.query_times = vec![1., 0., 1.];
    let (_, report) = produce(&request);
    assert_eq!(state(leading(&report, 0))["motion"], "BeyondTolerance");
    assert_eq!(state(leading(&report, 1))["motion"], "WithinTolerance");
    assert_eq!(state(leading(&report, 2))["motion"], "BeyondTolerance");
}

#[test]
fn scalar_motion_rule_uses_strict_margins_and_whole_space_override() {
    for (lo, hi, expected) in [
        (0., 0.04, Motion::WithinTolerance),
        (0.06, 0.1, Motion::BeyondTolerance),
        (0., 0.05, Motion::Unresolved),
        (0.05, 0.1, Motion::Unresolved),
        (0.04, 0.06, Motion::Unresolved),
    ] {
        assert_eq!(
            readout::classify_motion(&Quantity::Interval(Range::closed(lo, hi)), 1, 3, 0.05)
                .unwrap(),
            expected
        );
    }
    let zero = Quantity::Interval(Range::closed(0., 0.));
    assert_eq!(
        readout::classify_motion(&zero, 3, 3, 0.05).unwrap(),
        Motion::Unresolved
    );
    assert_eq!(
        readout::classify_motion(&zero, 1, 1, 0.05).unwrap(),
        Motion::Unresolved
    );
    assert_eq!(
        readout::classify_motion(&Quantity::Undefined("unresolved origin".into()), 1, 3, 0.05)
            .unwrap(),
        Motion::Unresolved
    );
    assert_eq!(
        readout::classify_motion(&Quantity::Interval(Range::closed(0., 1.)), 1, 3, 2.).unwrap(),
        Motion::WithinTolerance
    );
}

#[test]
fn invalid_delta_dimensions_and_nonfinite_ranges_are_refused() {
    let zero = Quantity::Interval(Range::closed(0., 0.));
    for delta in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            readout::classify_motion(&zero, 1, 3, delta),
            Err(Error::Invalid(_))
        ));
        let mut request = input("constant_line3");
        request.delta = delta;
        assert!(matches!(
            readout::analyze(&request, &Cancellation::default()),
            Err(Error::Invalid(_))
        ));
    }
    for (q, p) in [(0, 3), (1, 0), (4, 3)] {
        assert!(matches!(
            readout::classify_motion(&zero, q, p, 0.05),
            Err(Error::Invalid(_))
        ));
    }
    for range in [
        Range::closed(0.1, 0.),
        Range::closed(f64::NAN, 0.1),
        Range::closed(0., f64::INFINITY),
    ] {
        assert!(matches!(
            readout::classify_motion(&Quantity::Interval(range), 1, 3, 0.05),
            Err(Error::Invalid(_))
        ));
    }
    let mut request = input("constant_line3");
    request.delta = 2.;
    produce(&request);
}

#[test]
fn history_v1_and_unsupported_models_are_rejected_instead_of_omitted() {
    let mut request = input("constant_line3");
    request.schema = "eigentrail.lineage-summary.v0".into();
    assert!(matches!(
        readout::analyze(&request, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
    let mut request = input("constant_line3");
    request.history.schema = "eigentrail.histories.v1".into();
    assert!(matches!(
        readout::analyze(&request, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
    let mut request = input("constant_line3");
    let mut extra = request.history.configurations[0].clone();
    extra.id = "unsupported-jumps".into();
    extra.class = PathClass::Jumps {
        model: eigentrail_core::jumps::JumpModel {
            at_most_one_per_cell: true,
            minimum_jump: None,
            minimum_applies_to_all_candidates: false,
        },
    };
    request.history.configurations.push(extra);
    assert!(matches!(
        readout::analyze(&request, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
    let transport: Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/initialized_transport.json"
    ))
    .unwrap();
    let crossings: Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/transverse_crossings.json"
    ))
    .unwrap();
    for value in [
        &transport["history_cases"][0]["request"],
        &crossings["history_request"],
    ] {
        let history: HistoryRequest =
            serialization::decode(&serde_json::to_vec(value).unwrap()).unwrap();
        let request = Request {
            schema: "eigentrail.lineage-summary.v1".into(),
            history,
            delta: 0.05,
        };
        assert!(matches!(
            readout::analyze(&request, &Cancellation::default()),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn empty_queries_and_an_unassumed_common_frame_are_explicitly_refused() {
    let mut request = input("constant_line3");
    request.history.query_times.clear();
    assert!(matches!(
        readout::analyze(&request, &Cancellation::default()),
        Err(Error::Invalid(_))
    ));
    let mut request = input("common_frame_exchange3");
    if let PathClass::CommonFrame { model } = &mut request.history.configurations[0].class {
        model.common_frame_assumed = false;
    } else {
        panic!("expected model fixture");
    }
    assert!(readout::analyze(&request, &Cancellation::default()).is_err());
}

#[test]
fn cancellation_and_memory_refusal_cannot_return_completed_reports() {
    let request = input("constant_line3");
    let (report, _) = produce(&request);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        readout::analyze(&request, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        eigentrail_check::lineage_summary::check(&request, &report, &cancel),
        Err(Error::Cancelled)
    ));
    let mut small = input("constant_line3");
    small.history.observations.memory_bytes = 1;
    assert!(matches!(
        readout::analyze(&small, &Cancellation::default()),
        Err(Error::Resource(_))
    ));
}

#[test]
fn checker_reconstructs_row_dimensions_labels_intervals_and_motion() {
    let request = input("constant_line3");
    let (_, report) = produce(&request);
    for mutation in 0..12 {
        let mut bad = report.clone();
        let item = &mut bad["rows"][0]["state"];
        match mutation {
            0 => item["block_dimension"] = json!(2),
            1 => item["ambient_dimension"] = json!(4),
            2 => item["information_level"] = json!("WholeSpace"),
            3 => item["target"]["origins"] = json!([]),
            4 => item["target"]["persistent"] = json!(false),
            5 => item["rank"] = json!([2, 2]),
            6 => item["projector_error"] = encoded(0.),
            7 => item["strength"]["lower"] = encoded(5.),
            8 => item["share"] = json!({"kind":"Undefined","value":"forced"}),
            9 => item["motion"] = json!("BeyondTolerance"),
            10 => item["accuracy_target_attained"] = json!(false),
            _ => item["target"]["kind"] = json!("AnchorAxis"),
        }
        rejected(&request, bad);
    }
    let mut bad = report.clone();
    bad["completed"] = json!(false);
    rejected(&request, bad);
    let mut bad = report.clone();
    bad["request_hash"] = json!("0".repeat(64));
    rejected(&request, bad);
    let mut bad = report.clone();
    bad["managed_reservation"] = json!(1);
    rejected(&request, bad);
    let mut bad = report.clone();
    bad["resource_qualification"] = json!("total process RSS certified");
    rejected(&request, bad);
    let mut bad = report;
    bad["rows"][0]["state"]["unique_history"] = json!(true);
    rejected(&request, bad);
}

#[test]
fn checker_does_not_trust_whole_space_relabelled_as_an_accurate_line() {
    let request = input("hidden_shared_observations3");
    let (_, mut bad) = produce(&request);
    let item = &mut bad["rows"][0]["state"];
    item["block_dimension"] = json!(1);
    item["information_level"] = json!("Line");
    item["target"]["end"] = json!(1);
    item["motion"] = json!("WithinTolerance");
    item["accuracy_target_attained"] = json!(true);
    rejected(&request, bad);
}

#[test]
fn wrapper_hash_and_delta_are_both_authoritative() {
    let mut request = input("rotating_line3");
    let (_, mut bad) = produce(&request);
    request.delta = 1.;
    bad["request_hash"] = json!(serialization::digest(&request).unwrap());
    rejected(&request, bad); // Old BeyondTolerance rows are wrong under delta=1.
    let mut request = input("constant_line3");
    let (_, mut bad) = produce(&request);
    bad["history"]["request_hash"] = json!("0".repeat(64));
    rejected(&request, bad);
    request.history.configurations[0].id = "changed-information-set".into();
    let (_, report) = produce(&input("constant_line3"));
    rejected(&request, report);
}

#[test]
fn existing_outer_competitors_and_undefined_origin_motion_are_preserved() {
    let source: Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/continuous_workflow.json"
    ))
    .unwrap();
    let history: HistoryRequest =
        serialization::decode(&serde_json::to_vec(&source["cases"][3]["request"]).unwrap())
            .unwrap();
    let request = Request {
        schema: "eigentrail.lineage-summary.v1".into(),
        history,
        delta: 0.05,
    };
    let (_, report) = produce(&request);
    let final_index = request.history.query_times.len() - 1;
    let final_rows: Vec<_> = rows(&report)
        .iter()
        .filter(|row| row["query"] == final_index)
        .collect();
    assert_eq!(final_rows.len(), 3);
    for row in final_rows {
        let item = state(row);
        assert_eq!(item["target"]["origins"], json!([0, 1, 2]));
        assert_eq!(item["target"]["persistent"], false);
        assert_eq!(item["reference_displacement"]["kind"], "Undefined");
        assert_eq!(item["motion"], "Unresolved");
    }
    let mut bad = report.clone();
    bad["history"]["scope"] = json!("uniquely realizable complete labelled histories");
    rejected(&request, bad);
    let mut bad = report;
    let last = bad["rows"].as_array_mut().unwrap().last_mut().unwrap();
    last["state"]["target"]["origins"] = json!([0]);
    last["state"]["target"]["persistent"] = json!(true);
    last["state"]["reference_displacement"] = json!({"kind":"Interval","value":{
        "lower":encoded(0.),"upper":encoded(0.),"lower_closed":true,"upper_closed":true}});
    last["state"]["motion"] = json!("WithinTolerance");
    rejected(&request, bad);
}

#[test]
fn nested_history_is_checked_independently_of_readout_rows() {
    let source: Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/continuous_workflow.json"
    ))
    .unwrap();
    let history: HistoryRequest =
        serialization::decode(&serde_json::to_vec(&source["cases"][1]["request"]).unwrap())
            .unwrap();
    let request = Request {
        schema: "eigentrail.lineage-summary.v1".into(),
        history,
        delta: 0.05,
    };
    let (_, report) = produce(&request);
    let mut bad = report.clone();
    bad["history"]["configurations"][0]["continuous_conditional"]["segments"][0]["edges"][0]
        .as_array_mut()
        .unwrap()
        .pop();
    rejected(&request, bad);
    let mut bad = report;
    bad["history"]["configurations"][0]["continuous_workflow"][0]["queries"][0]["state"]["blocks"]
        [0]["origins"] = json!([]);
    rejected(&request, bad);
}

#[test]
fn existing_history_wire_and_feasibility_are_preserved_in_the_embedded_report() {
    let request = input("constant_plane5");
    let (report, value) = produce(&request);
    let original = families::analyze(&request.history, &Cancellation::default()).unwrap();
    assert_eq!(
        serialization::encode(&report.history).unwrap(),
        serialization::encode(&original).unwrap()
    );
    assert_eq!(value["schema"], "eigentrail.lineage-summary.v1");
    assert_eq!(value["history"]["schema"], "eigentrail.histories.v2");
    assert_eq!(
        value["request_hash"],
        serialization::digest(&request).unwrap()
    );
    assert_eq!(
        value["history"]["request_hash"],
        serialization::digest(&request.history).unwrap()
    );
    assert!(
        value["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("feasibility").is_none())
    );
    let p = request.history.observations.features.len();
    let rows_max = request.history.configurations.len() * request.history.query_times.len() * p;
    let expected_reservation =
        families::validate_request(&request.history).unwrap() + 4096 + rows_max * (8192 + 16 * p);
    assert_eq!(report.managed_reservation, expected_reservation);
    assert!(report.managed_reservation <= request.history.observations.memory_bytes);
}
