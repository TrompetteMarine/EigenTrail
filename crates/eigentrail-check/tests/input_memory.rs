//! Independent live Rust-heap measurements for the frozen pre-copy boundaries.
use eigentrail_core::{
    resources::Cancellation,
    serialization::input::{RequestKind, inspect_request, preflight, preflight_with_cancel},
    *,
};
use sha2::{Digest, Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    io::{self, Cursor, Read},
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

fn scalar(dates: usize, budget: usize) -> TraceRequest {
    let mut request = TraceRequest::conditional(
        (0..dates).map(|date| date as f64).collect(),
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
    request.memory_bytes = budget;
    request
}
fn text(request: &TraceRequest) -> String {
    String::from_utf8(serialization::encode(request).unwrap()).unwrap()
}
fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/resource_boundaries.json"
    ))
    .unwrap()
}
fn verify_wire(name: &str, input: &str) {
    let oracle = oracle();
    assert_eq!(
        input.len(),
        oracle["wire"][name]["bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(input.as_bytes())),
        oracle["wire"][name]["sha256"]
    );
}
fn owned_workflow(
    input: &str,
    cancel: &Cancellation,
) -> Result<(String, TraceRequest, TraceReport, Vec<u8>)> {
    // Same production guard/decode/track/check/encode chain, with additional retention.
    preflight(input, &[], RequestKind::Trace)?;
    let owned = serialization::owned_text(input)?;
    let request: TraceRequest = serialization::decode_with_cancel(owned.as_bytes(), cancel)?;
    let report = track_with_cancel(&request, cancel)?;
    eigentrail_check::check_with_cancel(&request, &report, cancel)?;
    let output = serialization::encode_with_cancel(&report, cancel)?;
    Ok((owned, request, report, output))
}

#[test]
fn borrowed_refusals_and_whole_owned_scalar_workflow_stay_within_measured_budgets() {
    start();
    let mut known = Vec::<u8>::with_capacity(4096);
    known.resize(4096, 1);
    let first = COUNTS.with(|slot| slot.get().unwrap());
    known.reserve_exact(4096);
    let grown = COUNTS.with(|slot| slot.get().unwrap());
    drop(known);
    let empty = stop();
    assert_eq!(first.live, 4096);
    assert_eq!(grown.live, 8192);
    assert_eq!((empty.live, empty.peak, empty.underflows), (0, 8192, 0));
    let cancel = Cancellation::default();
    let ordinary = text(&scalar(2, 3 * 1024 * 1024));
    start();
    let budget = preflight_with_cancel(&ordinary, &[], RequestKind::Trace, &cancel);
    let measured = stop();
    assert_eq!(budget.unwrap(), 3 * 1024 * 1024);
    assert_eq!(
        (measured.live, measured.peak, measured.underflows),
        (0, 0, 0)
    );
    let mut request = scalar(2, 4096);
    request.features[0] = "x".repeat(4096);
    let a = text(&request);
    verify_wire("A", &a);
    start();
    let result = owned_workflow(&a, &cancel);
    let measured = stop();
    assert!(matches!(result, Err(Error::Resource(_))));
    assert_eq!(measured.underflows, 0);
    assert!(measured.peak <= 4096, "A peak {}", measured.peak);
    println!(
        "A pre-copy Resource: wire={} peak={} retained={}",
        a.len(),
        measured.peak,
        measured.live
    );
    drop(result);

    let mut b = text(&scalar(2, 3 * 1024 * 1024));
    b.pop();
    b.push_str(",\"zz_unknown\":[");
    for index in 0..500000 {
        if index > 0 {
            b.push(',');
        }
        b.push_str("[]");
    }
    b.push_str("]}");
    verify_wire("B", &b);
    start();
    let result = owned_workflow(&b, &cancel);
    let measured = stop();
    assert!(matches!(
        result,
        Err(Error::Invalid(_) | Error::Resource(_))
    ));
    assert_eq!(measured.underflows, 0);
    assert!(measured.peak <= 3 * 1024 * 1024, "B peak {}", measured.peak);
    println!(
        "B unknown-field refusal: wire={} peak={} retained={}",
        b.len(),
        measured.peak,
        measured.live
    );
    drop(result);

    for (name, limit) in [
        ("C_tight", 3 * 1024 * 1024),
        ("C_default", resources::DEFAULT_BYTES),
    ] {
        let input = text(&scalar(2000, limit));
        verify_wire(name, &input);
        start();
        let result = owned_workflow(&input, &cancel);
        let measured = stop();
        assert_eq!(
            measured.underflows, 0,
            "untracked deallocation invalidates C"
        );
        assert!(
            measured.peak <= limit,
            "{name} peak {} exceeds {limit}",
            measured.peak
        );
        match result {
            Ok((owned, request, report, output)) => {
                assert_eq!(name, "C_default");
                assert_eq!(owned, input);
                assert_eq!(request.times.len(), 2000);
                assert_eq!(report.summaries.len(), 2000);
                assert!(
                    report
                        .summaries
                        .iter()
                        .all(|row| row[0].strength.lower == 1. && row[0].strength.upper == 1.)
                );
                assert_eq!(
                    serialization::decode::<TraceReport>(&output).unwrap(),
                    report
                );
                println!(
                    "C default completed: peak={} retained={} wire={} output={}",
                    measured.peak,
                    measured.live,
                    owned.len(),
                    output.len()
                );
            }
            Err(Error::Resource(reason)) => {
                assert_eq!(name, "C_tight");
                println!("C tight Resource: peak={} reason={reason}", measured.peak);
            }
            Err(error) => panic!("unexpected C outcome: {error}"),
        }
    }
    // The shared ownership ledger/global cap remains M2/M3; this measures only these paths.
}

fn history() -> String {
    let request = families::HistoryRequest {
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
    };
    String::from_utf8(serialization::encode(&request).unwrap()).unwrap()
}
#[test]
fn trace_and_nested_history_guards_enforce_aggregate_input_before_copy() {
    for (name, input, kind, budget) in [
        (
            "E_trace",
            text(&scalar(2, 3 * 1024 * 1024)),
            RequestKind::Trace,
            3 * 1024 * 1024,
        ),
        (
            "E_history",
            history(),
            RequestKind::History,
            16 * 1024 * 1024,
        ),
    ] {
        verify_wire(name, &input);
        assert_eq!(preflight(&input, &[], kind).unwrap(), budget);
        let mut companion = "x".repeat(budget - input.len());
        assert_eq!(preflight(&input, &[&companion], kind).unwrap(), budget);
        companion.push('x');
        assert!(matches!(
            preflight(&input, &[&companion], kind),
            Err(Error::Resource(_))
        ));
        let split = companion.len() / 2;
        assert!(matches!(
            preflight(&input, &[&companion[..split], &companion[split..]], kind),
            Err(Error::Resource(_))
        ));
        let header = inspect_request(
            Cursor::new(input.as_bytes()),
            kind,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(header.memory_bytes, budget);
        assert_eq!(header.bytes, input.len());
    }
}

#[test]
fn malformed_duplicate_escaped_budget_and_cancelled_inspection_cannot_bypass_guard() {
    let ordinary = text(&scalar(2, 3 * 1024 * 1024));
    for budget in ["-1", "0.5", "3e6", "\"3145728\"", "18446744073709551616"] {
        let bad = ordinary.replace(
            "\"memory_bytes\":3145728",
            &format!("\"memory_bytes\":{budget}"),
        );
        assert!(
            preflight(&bad, &[], RequestKind::Trace).is_err(),
            "accepted budget {budget}"
        );
    }
    for key in ["memory_bytes", "memory_\\u0062ytes"] {
        let mut duplicate = ordinary.clone();
        duplicate.pop();
        duplicate.push_str(&format!(",\"{key}\":536870912}}"));
        assert!(preflight(&duplicate, &[], RequestKind::Trace).is_err());
    }
    let escaped = ordinary.replace("\"memory_bytes\"", "\"memory_\\u0062ytes\"");
    assert!(preflight(&escaped, &[], RequestKind::Trace).is_err());
    for broken in [
        "",
        "{",
        "[]",
        "null",
        "{\"memory_bytes\":3145728}",
        "{\"memory_bytes\":",
    ] {
        assert!(preflight(broken, &[], RequestKind::Trace).is_err());
    }
    let mut quoted = scalar(2, 3 * 1024 * 1024);
    quoted.features[0] = "literal \"memory_bytes\": 536870912 and \\u0062".into();
    assert_eq!(
        preflight(&text(&quoted), &[], RequestKind::Trace).unwrap(),
        3 * 1024 * 1024
    );
    struct NeverRead;
    impl Read for NeverRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("cancelled scanner read input")
        }
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        inspect_request(NeverRead, RequestKind::Trace, &cancel),
        Err(Error::Cancelled)
    ));
}
