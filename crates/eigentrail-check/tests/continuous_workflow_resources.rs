//! Additional interface/resource regressions; primary frozen cases are separate.
use eigentrail_core::{
    families::*,
    resources::{Cancellation, memory::Operation},
    *,
};
use serde_json::Value;

fn request(value: f64) -> HistoryRequest {
    HistoryRequest {
        schema: "eigentrail.histories.v2".into(),
        observations: TraceRequest::conditional(
            vec![0., 1.],
            vec![
                Matrix {
                    n: 1,
                    data: vec![value]
                };
                2
            ],
            vec![0.; 2],
            0.,
        ),
        configurations: vec![Configuration {
            id: "continuous".into(),
            lipschitz: 0.,
            class: PathClass::Continuous,
            witnesses: vec![],
        }],
        query_times: vec![0.5],
    }
}
fn report(request: &HistoryRequest) -> Value {
    let cancel = Cancellation::default();
    let result = families::analyze(request, &cancel).unwrap();
    eigentrail_check::families::check(request, &result, &cancel).unwrap();
    serde_json::from_slice(&serialization::encode(&result).unwrap()).unwrap()
}
fn rejected(request: &HistoryRequest, bad: &Value) {
    let decoded = serialization::decode::<HistoryReport>(&serde_json::to_vec(bad).unwrap());
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
fn zero_trace_has_an_undefined_share_and_whole_space_projector() {
    let value = report(&request(0.));
    let query = &value["configurations"][0]["continuous_workflow"][0]["queries"][0];
    assert_eq!(query["state"]["kind"], "Bounds");
    assert_eq!(query["state"]["trace"]["lower"], "f64:0000000000000000");
    assert_eq!(query["state"]["trace"]["upper"], "f64:0000000000000000");
    let block = &query["state"]["blocks"][0];
    assert_eq!(block["share"]["kind"], "Undefined");
    assert_eq!(block["projector_error"], "f64:0000000000000000");
    assert_eq!(block["rank"], serde_json::json!([1, 1]));
}

#[test]
fn mixed_models_preserve_query_order_duplicates_and_distinct_scopes() {
    let mut input = request(1.);
    input.query_times = vec![0.75, 0.25, 0.75];
    input.configurations.push(Configuration {
        id: "common-frame".into(),
        lipschitz: 0.,
        class: PathClass::CommonFrame {
            model: common_frame::CommonFrameModel {
                weights: vec![1., 0.],
                common_frame_assumed: true,
            },
        },
        witnesses: vec![],
    });
    let value = report(&input);
    assert_eq!(value["configurations"].as_array().unwrap().len(), 2);
    let continuous = &value["configurations"][0];
    let common = &value["configurations"][1];
    assert!(continuous.get("common_frame_queries").is_none());
    assert!(common.get("continuous_workflow").is_none());
    let expected = [
        "f64:3fe8000000000000",
        "f64:3fd0000000000000",
        "f64:3fe8000000000000",
    ];
    for rows in [
        &continuous["continuous_workflow"][0]["queries"],
        &common["common_frame_queries"][0]["queries"],
    ] {
        assert_eq!(rows.as_array().unwrap().len(), 3);
        for (row, time) in rows.as_array().unwrap().iter().zip(expected) {
            assert_eq!(row["time"], time);
        }
    }
    let grid = continuous["grid"].as_array().unwrap();
    assert_eq!(grid.len(), 4);
    assert_eq!(grid[2]["queries"], serde_json::json!([0, 2]));
}

#[test]
fn v2_evidence_is_required_and_cannot_be_smuggled_into_v1() {
    let input = request(1.);
    let valid = report(&input);
    let mut missing = valid.clone();
    missing["configurations"][0]
        .as_object_mut()
        .unwrap()
        .remove("continuous_workflow");
    rejected(&input, &missing);
    let mut legacy = input.clone();
    legacy.schema = families::SCHEMA.into();
    let mut old = report(&legacy);
    assert!(
        old["configurations"][0]
            .get("continuous_workflow")
            .is_none()
    );
    old["configurations"][0]["continuous_workflow"] =
        valid["configurations"][0]["continuous_workflow"].clone();
    rejected(&legacy, &old);
}

#[test]
fn v2_outputs_retain_their_budget_and_incomplete_work_refunds() {
    let input = request(1.);
    let cancel = Cancellation::default();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let result = operation
        .own(|| {
            let output = families::analyze(&input, &cancel)?;
            eigentrail_check::families::check(&input, &output, &cancel)?;
            let bytes = serialization::encode(&output)?;
            Ok((output, bytes))
        })
        .unwrap();
    assert!(operation.stats().live >= baseline + result.1.capacity());
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    cancel.cancel();
    assert!(matches!(
        operation.own(|| families::analyze(&input, &cancel)),
        Err(Error::Cancelled)
    ));
    assert_eq!(operation.stats().live, baseline);
    let mut tight = input;
    tight.observations.memory_bytes = 8192;
    let tiny = Operation::new(8192).unwrap();
    assert!(matches!(
        tiny.own(|| families::analyze(&tight, &Cancellation::default())),
        Err(Error::Resource(_))
    ));
    assert_eq!(tiny.stats().live, 8192);
}
