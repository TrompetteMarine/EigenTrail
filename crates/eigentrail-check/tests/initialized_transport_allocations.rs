//! Independent System observation of owned W5 transport, codec and checker allocations.
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, Operation},
    },
    *,
};
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
thread_local! { static COUNTS:Cell<Option<Counts>>=const {Cell::new(None)}; }
struct Probe;
fn allocation(old: usize, new: usize) {
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut value) = slot.get() {
            if old > value.live {
                value.underflows += 1;
                value.live = 0;
            } else {
                value.live -= old;
            }
            value.live += new;
            value.peak = value.peak.max(value.live);
            slot.set(Some(value));
        }
    });
}
unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            allocation(0, layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            allocation(0, layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        allocation(layout.size(), 0);
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let next = unsafe { System.realloc(p, layout, new) };
        if !next.is_null() {
            allocation(layout.size(), new);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: Probe = Probe;
fn start() {
    COUNTS.with(|s| {
        assert!(s.get().is_none());
        s.set(Some(Counts::default()));
    });
}
fn current() -> Counts {
    COUNTS.with(|s| s.get().unwrap())
}
fn stop() -> Counts {
    COUNTS.with(|s| s.replace(None).unwrap())
}
fn measured(name: &str, stats: memory::Stats, counts: Counts) {
    assert_eq!((counts.live, counts.underflows), (0, 0), "{name}");
    assert!(
        counts.peak <= stats.peak && stats.peak <= stats.limit,
        "{name}: System {} ledger {} limit {}",
        counts.peak,
        stats.peak,
        stats.limit
    );
    println!(
        "{name}: System peak={} ledger peak={} limit={} live_after_drop={}",
        counts.peak, stats.peak, stats.limit, counts.live
    );
}

fn pipeline(name: &str, input: &[u8], history: bool) {
    let cancel = Cancellation::default();
    start();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let result = operation.own(|| {
        let input = memory::copy(input)?;
        if history {
            let request: families::HistoryRequest = serialization::decode_in(&input, &cancel)?;
            let report = families::analyze(&request, &cancel)?;
            let output = serialization::encode_with_cancel(&report, &cancel)?;
            memory::scope(|| {
                let replay: families::HistoryReport = serialization::decode_in(&output, &cancel)?;
                eigentrail_check::families::check(&request, &replay, &cancel)
            })?;
            if !report.completed {
                return Err(Error::Invalid("unfinished history".into()));
            }
            Ok((input, (Some((request, report)), None), output))
        } else {
            let request: transport_workflow::TransportRequest =
                serialization::decode_in(&input, &cancel)?;
            let report = transport_workflow::analyze(&request, &cancel)?;
            let output = serialization::encode_with_cancel(&report, &cancel)?;
            memory::scope(|| {
                let replay: transport_workflow::TransportAssessment =
                    serialization::decode_in(&output, &cancel)?;
                eigentrail_check::transport::check(&request, &replay, &cancel)
            })?;
            if !report.completed {
                return Err(Error::Invalid("unfinished geometry".into()));
            }
            if name == "maximum-scalar-mesh" {
                assert_eq!(request.times.len(), 65537);
                assert_eq!(report.steps.len(), 65536);
                assert_eq!(report.endpoints.len(), 65537);
                assert_eq!(report.outcome, transport_workflow::Outcome::Resolved);
            }
            Ok((input, (None, Some((request, report))), output))
        }
    });
    let stats = operation.stats();
    let observed = current();
    let succeeded = result.is_ok();
    if let Ok(value) = &result {
        assert!(stats.live >= observed.live);
        assert!(stats.live >= value.2.capacity());
    }
    if let Err(error) = &result {
        eprintln!("{name}: {error:?}");
    }
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    drop(operation);
    let counts = stop();
    assert!(succeeded, "{name}: owned transport pipeline failed");
    measured(name, stats, counts);
}

#[test]
fn all_geometry_and_covariance_histories_charge_owned_proofs_codec_and_checker() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/initialized_transport.json"
    ))
    .unwrap();
    for (key, history) in [("cases", false), ("history_cases", true)] {
        for case in oracle[key].as_array().unwrap() {
            pipeline(
                case["id"].as_str().unwrap(),
                &serde_json::to_vec(&case["request"]).unwrap(),
                history,
            );
        }
    }
}

#[test]
fn maximum_scalar_transport_mesh_keeps_all_nodes_and_owned_replay_within_cap() {
    let n = 65537;
    let request = transport_workflow::TransportRequest {
        schema: "eigentrail.transport.v1".into(),
        dimension: 1,
        times: (0..n).map(|j| j as f64 / 65536.).collect(),
        reference_nodes: vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            n
        ],
        nodes: vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            n
        ],
        block: [0, 1],
        initial_frame: Matrix {
            n: 1,
            data: vec![1.],
        },
        initial_gauge: Matrix {
            n: 1,
            data: vec![1.],
        },
        reference_speed_limit: 0.,
        max_speed: 0.,
        endpoint_error_limit: 0.,
        initial_correction_limit: 0.,
        initial_mismatch_limit: 0.,
        target: 1e-6,
        memory_bytes: resources::DEFAULT_BYTES,
    };
    pipeline(
        "maximum-scalar-mesh",
        &serialization::encode(&request).unwrap(),
        false,
    );
}
