//! Independent System allocator checks of shared operation ownership.
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, Operation},
    },
    serialization::input::{RequestKind, preflight},
    *,
};
use sha2::{Digest, Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Default)]
struct Counts {
    live: usize,
    peak: usize,
    underflows: usize,
}
thread_local! {static COUNTS:Cell<Option<Counts>>=const {Cell::new(None)};}
struct Probe;
fn allocation(old: usize, new: usize) {
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut count) = slot.get() {
            if old > count.live {
                count.underflows += 1;
                count.live = 0;
            } else {
                count.live -= old;
            }
            count.live += new;
            count.peak = count.peak.max(count.live);
            slot.set(Some(count));
        }
    });
}
unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocation(0, layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocation(0, layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        allocation(layout.size(), 0);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new) };
        if !replacement.is_null() {
            allocation(layout.size(), new);
        }
        replacement
    }
}
#[global_allocator]
static ALLOCATOR: Probe = Probe;
fn start() {
    COUNTS.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some(Counts::default()));
    });
}
fn stop() -> Counts {
    COUNTS.with(|slot| slot.replace(None).unwrap())
}

fn current() -> Counts {
    COUNTS.with(|slot| slot.get().unwrap())
}
fn scalar(dates: usize, limit: usize) -> TraceRequest {
    let mut request = TraceRequest::conditional(
        (0..dates).map(|i| i as f64).collect(),
        vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            dates
        ],
        vec![0.; dates],
        0.,
    );
    request.memory_bytes = limit;
    request
}
fn encode<T: serde::Serialize>(value: &T) -> String {
    String::from_utf8(serialization::encode(value).unwrap()).unwrap()
}
fn history() -> families::HistoryRequest {
    families::HistoryRequest {
        schema: families::SCHEMA.into(),
        observations: scalar(2, 16 * 1024 * 1024),
        configurations: vec![families::Configuration {
            id: "common-frame".into(),
            lipschitz: 0.,
            witnesses: vec![],
            class: families::PathClass::CommonFrame {
                model: common_frame::CommonFrameModel {
                    weights: vec![1., 0.],
                    common_frame_assumed: true,
                },
            },
        }],
        query_times: vec![0.5],
    }
}
fn payload() -> String {
    let mut value = String::from("[");
    for i in 0..500000 {
        if i != 0 {
            value.push(',');
        }
        value.push_str("[]");
    }
    value.push(']');
    value
}
fn fixture(name: &str) -> String {
    let assumptions =
        "\"supplied nodal covariance envelope; declared global Lipschitz regularity\"";
    let malformed = || payload();
    match name {
        "trace" => encode(&scalar(2, 3 * 1024 * 1024)),
        "history" => encode(&history()),
        "common_frame_model" => encode(&common_frame::CommonFrameModel {
            weights: vec![1., 0.],
            common_frame_assumed: true,
        }),
        "jump_model" => encode(&jumps::JumpModel {
            at_most_one_per_cell: true,
            minimum_jump: None,
            minimum_applies_to_all_candidates: false,
        }),
        "known_trace" => fixture("trace").replace(assumptions, &malformed()),
        "known_history" => fixture("history")
            .replace(assumptions, &malformed())
            .replace("\"memory_bytes\":16777216", "\"memory_bytes\":3145728"),
        "known_common_frame_model" => format!(
            "{{\"common_frame_assumed\":true,\"weights\":{}}}",
            malformed()
        ),
        "known_jump_model" => format!(
            "{{\"at_most_one_per_cell\":true,\"minimum_applies_to_all_candidates\":false,\"minimum_jump\":{}}}",
            malformed()
        ),
        "known_report" => format!(
            "{{\"eigensystems\":{},\"schema\":\"eigentrail.trace.v1\"}}",
            malformed()
        ),
        "retained_tight" => encode(&scalar(2000, 3 * 1024 * 1024)),
        "retained_default" => encode(&scalar(2000, resources::DEFAULT_BYTES)),
        _ => panic!("unknown independent fixture"),
    }
}
fn verified_fixture(name: &str) -> String {
    let input = fixture(name);
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/shared_memory.json")).unwrap();
    assert_eq!(
        input.len(),
        oracle["wire"][name]["bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(input.as_bytes())),
        oracle["wire"][name]["sha256"]
    );
    input
}
fn assert_peak(ledger_peak: usize, measured: Counts, limit: usize) {
    assert_eq!(measured.underflows, 0);
    assert!(
        measured.peak <= limit,
        "System peak {} > {limit}",
        measured.peak
    );
    assert!(
        ledger_peak >= measured.peak,
        "shared ledger {ledger_peak} < System peak {}",
        measured.peak
    );
    assert!(
        ledger_peak <= limit,
        "ledger peak {ledger_peak} > limit {limit}"
    );
}

#[test]
fn system_probe_self_check_and_zero_budget_context_refusal() {
    start();
    let mut known = Vec::<u8>::with_capacity(4096);
    known.resize(4096, 1);
    let initial = current();
    known.reserve_exact(4096);
    let grown = current();
    drop(known);
    let empty = stop();
    assert_eq!(initial.live, 4096);
    assert_eq!(grown.live, 8192);
    assert_eq!((empty.live, empty.peak, empty.underflows), (0, 8192, 0));
    start();
    let result = Operation::new(0);
    let refused = matches!(result, Err(Error::Invalid(_) | Error::Resource(_)));
    drop(result);
    let measured = stop();
    assert!(refused);
    assert_eq!(
        (measured.live, measured.peak, measured.underflows),
        (0, 0, 0)
    );
}

#[test]
fn known_field_nested_payloads_refuse_with_shared_ledger_covering_actual_heap() {
    let cancel = Cancellation::default();
    let valid_request = verified_fixture("trace");
    for name in [
        "known_trace",
        "known_history",
        "known_common_frame_model",
        "known_jump_model",
        "known_report",
    ] {
        let input = verified_fixture(name);
        let limit = 3 * 1024 * 1024;
        start();
        let operation = Operation::new(limit).unwrap();
        let baseline = operation.stats().live;
        let result = operation.own(|| {
            if name == "known_trace" || name == "known_history" {
                let kind = if name == "known_history" {
                    RequestKind::History
                } else {
                    RequestKind::Trace
                };
                preflight(&input, &[], kind)?;
            } else {
                preflight(&valid_request, &[&input], RequestKind::Trace)?;
            }
            let owned = memory::text(&input)?;
            match name {
                "known_trace" => {
                    serialization::decode_in::<TraceRequest>(owned.as_bytes(), &cancel)?;
                }
                "known_history" => {
                    serialization::decode_in::<families::HistoryRequest>(
                        owned.as_bytes(),
                        &cancel,
                    )?;
                }
                "known_common_frame_model" => {
                    serialization::decode_in::<common_frame::CommonFrameModel>(
                        owned.as_bytes(),
                        &cancel,
                    )?;
                }
                "known_jump_model" => {
                    serialization::decode_in::<jumps::JumpModel>(owned.as_bytes(), &cancel)?;
                }
                _ => {
                    serialization::decode_in::<TraceReport>(owned.as_bytes(), &cancel)?;
                }
            }
            Ok(())
        });
        let refused = matches!(result, Err(Error::Invalid(_) | Error::Resource(_)));
        drop(result);
        let refunded = operation.stats();
        drop(operation);
        let measured = stop();
        assert!(refused, "{name} accepted");
        assert_eq!(
            refunded.live, baseline,
            "{name} retained failed temporary charges"
        );
        assert_eq!(
            measured.live, 0,
            "{name} retained an allocation after context drop"
        );
        assert_peak(refunded.peak, measured, limit);
        println!(
            "{name}: System peak={} ledger peak={} live_after_drop={}",
            measured.peak, refunded.peak, measured.live
        );
    }
}

#[test]
fn full_owned_retained_pipeline_and_checker_codec_coexistence_are_charged() {
    let cancel = Cancellation::default();
    for (name, limit) in [
        ("retained_tight", 3 * 1024 * 1024),
        ("retained_default", resources::DEFAULT_BYTES),
    ] {
        let input = verified_fixture(name);
        start();
        let operation = Operation::new(limit).unwrap();
        let baseline = operation.stats().live;
        let result = operation.own(|| {
            preflight(&input, &[], RequestKind::Trace)?;
            let owned = memory::text(&input)?;
            let request: TraceRequest = serialization::decode_in(owned.as_bytes(), &cancel)?;
            let report = track_with_cancel(&request, &cancel)?;
            eigentrail_check::check_with_cancel(&request, &report, &cancel)?;
            let output = serialization::encode_with_cancel(&report, &cancel)?;
            Ok((owned, request, report, output))
        });
        let retained = operation.stats();
        let actual_retained = current();
        let (successful, exact_strengths, output_bytes) = match &result {
            Ok(managed) => {
                let (_, request, report, bytes) = &**managed;
                (
                    true,
                    request.times.len() == 2000
                        && report.summaries.len() == 2000
                        && report
                            .summaries
                            .iter()
                            .all(|row| row[0].strength.lower == 1. && row[0].strength.upper == 1.),
                    bytes.len(),
                )
            }
            Err(Error::Resource(_)) => (false, false, 0),
            Err(_) => (false, false, usize::MAX),
        };
        drop(result);
        let refunded = operation.stats();
        drop(operation);
        let measured = stop();
        assert_eq!(successful, name == "retained_default");
        if successful {
            assert!(exact_strengths);
            assert!(retained.live >= actual_retained.live);
            assert!(retained.live >= output_bytes);
        } else {
            assert_ne!(
                output_bytes,
                usize::MAX,
                "tight analysis failed without Resource"
            );
        }
        assert_eq!(refunded.live, baseline);
        assert_eq!(measured.live, 0);
        assert_peak(refunded.peak, measured, limit);
        println!(
            "{name}: System peak={} retained={} ledger peak={} ledger retained={} output={output_bytes}",
            measured.peak, actual_retained.live, retained.peak, retained.live
        );
    }
}

#[test]
fn cancelled_and_refused_temporary_allocations_release_their_charges() {
    let input = verified_fixture("trace");
    let cancel = Cancellation::default();
    cancel.cancel();
    for cancellation in [false, true] {
        let limit = 3 * 1024 * 1024;
        start();
        let operation = Operation::new(limit).unwrap();
        let baseline = operation.stats().live;
        let result = operation.own(|| {
            let owned = memory::text(&input)?;
            if cancellation {
                let _: TraceRequest = serialization::decode_in(owned.as_bytes(), &cancel)?;
            }
            memory::capacity::<u8>(limit)
        });
        let expected_error = if cancellation {
            matches!(result, Err(Error::Cancelled))
        } else {
            matches!(result, Err(Error::Resource(_)))
        };
        drop(result);
        let refunded = operation.stats();
        drop(operation);
        let measured = stop();
        assert!(expected_error);
        assert_eq!(refunded.live, baseline);
        assert_eq!(measured.live, 0);
        assert_peak(refunded.peak, measured, limit);
    }
}

#[test]
fn all_supported_capabilities_share_retained_output_with_checker_replay() {
    let cancel = Cancellation::default();
    let trace = verified_fixture("trace");
    let history = verified_fixture("history");
    let common_model = verified_fixture("common_frame_model");
    let jump_model = verified_fixture("jump_model");
    for capability in ["trace", "history", "common-frame", "jumps"] {
        let limit = if capability == "history" {
            16 * 1024 * 1024
        } else {
            3 * 1024 * 1024
        };
        let input = if capability == "history" {
            &history
        } else {
            &trace
        };
        start();
        let operation = Operation::new(limit).unwrap();
        let baseline = operation.stats().live;
        let result = operation.own(|| {
            let owned = memory::text(input)?;
            if capability == "history" {
                let request: families::HistoryRequest =
                    serialization::decode_in(owned.as_bytes(), &cancel)?;
                let report = families::analyze(&request, &cancel)?;
                eigentrail_check::families::check(&request, &report, &cancel)?;
                return serialization::encode_with_cancel(&report, &cancel);
            }
            let request: TraceRequest = serialization::decode_in(owned.as_bytes(), &cancel)?;
            match capability {
                "trace" => {
                    let report = track_with_cancel(&request, &cancel)?;
                    eigentrail_check::check_with_cancel(&request, &report, &cancel)?;
                    serialization::encode_with_cancel(&report, &cancel)
                }
                "common-frame" => {
                    let model_text = memory::text(&common_model)?;
                    let model: common_frame::CommonFrameModel =
                        serialization::decode_in(model_text.as_bytes(), &cancel)?;
                    let report = common_frame::analyze(&request, &model, &cancel)?;
                    eigentrail_check::common_frame::check(&request, &model, &report, &cancel)?;
                    serialization::encode_with_cancel(&report, &cancel)
                }
                _ => {
                    let model_text = memory::text(&jump_model)?;
                    let model: jumps::JumpModel =
                        serialization::decode_in(model_text.as_bytes(), &cancel)?;
                    let report = jumps::analyze_with_cancel(&request, &model, &cancel)?;
                    eigentrail_check::jumps::check_with_cancel(&request, &model, &report, &cancel)?;
                    serialization::encode_with_cancel(&report, &cancel)
                }
            }
        });
        let mut replay_ok = false;
        if let Ok(encoded) = &result {
            replay_ok = operation
                .run(|| {
                    if capability == "history" {
                        let request: families::HistoryRequest =
                            serialization::decode_in(input.as_bytes(), &cancel)?;
                        let report: families::HistoryReport =
                            serialization::decode_in(encoded, &cancel)?;
                        return eigentrail_check::families::check(&request, &report, &cancel);
                    }
                    let request: TraceRequest =
                        serialization::decode_in(input.as_bytes(), &cancel)?;
                    match capability {
                        "trace" => {
                            let report: TraceReport = serialization::decode_in(encoded, &cancel)?;
                            eigentrail_check::check_with_cancel(&request, &report, &cancel)
                        }
                        "common-frame" => {
                            let model: common_frame::CommonFrameModel =
                                serialization::decode_in(common_model.as_bytes(), &cancel)?;
                            let report: common_frame::CommonFrameReport =
                                serialization::decode_in(encoded, &cancel)?;
                            eigentrail_check::common_frame::check(
                                &request, &model, &report, &cancel,
                            )
                        }
                        _ => {
                            let model: jumps::JumpModel =
                                serialization::decode_in(jump_model.as_bytes(), &cancel)?;
                            let report: jumps::JumpReport =
                                serialization::decode_in(encoded, &cancel)?;
                            eigentrail_check::jumps::check_with_cancel(
                                &request, &model, &report, &cancel,
                            )
                        }
                    }
                })
                .is_ok();
        }
        let retained = operation.stats();
        let actual_retained = current();
        let success = result.is_ok();
        drop(result);
        let refunded = operation.stats();
        drop(operation);
        let measured = stop();
        assert!(
            success && replay_ok,
            "{capability} failed production/checker replay"
        );
        assert!(retained.live >= actual_retained.live);
        assert_eq!(refunded.live, baseline);
        assert_eq!(measured.live, 0);
        assert_peak(retained.peak, measured, limit);
        println!(
            "{capability}: shared checker/output System peak={} ledger peak={}",
            measured.peak, retained.peak
        );
    }
}
