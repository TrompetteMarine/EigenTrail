//! One test executable: actual native helper, cold worker and cross-thread System counts.
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, Operation},
        worker::{CONTROL_BYTES, STACK_BYTES, Worker},
    },
    *,
};
use sha2::{Digest, Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    time::Duration,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static UNDERFLOWS: AtomicUsize = AtomicUsize::new(0);
struct Probe;
fn allocation(old: usize, new: usize) {
    if ACTIVE.load(SeqCst) {
        let before = LIVE.fetch_sub(old, SeqCst);
        if before < old {
            UNDERFLOWS.fetch_add(1, SeqCst);
        }
        let live = LIVE.fetch_add(new, SeqCst).wrapping_add(new);
        PEAK.fetch_max(live, SeqCst);
    }
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
    assert!(!ACTIVE.load(SeqCst));
    LIVE.store(0, SeqCst);
    PEAK.store(0, SeqCst);
    UNDERFLOWS.store(0, SeqCst);
    ACTIVE.store(true, SeqCst);
}
fn stop() -> (usize, usize, usize) {
    ACTIVE.store(false, SeqCst);
    (
        LIVE.load(SeqCst),
        PEAK.load(SeqCst),
        UNDERFLOWS.load(SeqCst),
    )
}
fn scalar(dates: usize) -> TraceRequest {
    TraceRequest::conditional(
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
    )
}
fn wire<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serialization::encode(value).unwrap()
}
fn checked_wire(name: &str, record: &TraceRequest) -> Vec<u8> {
    let value = wire(record);
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/resource_qualification.json"
    ))
    .unwrap();
    assert_eq!(
        value.len(),
        oracle["wire"][name]["bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&value)),
        oracle["wire"][name]["sha256"]
    );
    value
}
fn analyze(input: &[u8], kind: &str, cancel: &Cancellation) -> Result<Vec<u8>> {
    if kind == "history" {
        let request: families::HistoryRequest = serialization::decode_in(input, cancel)?;
        let report = families::analyze(&request, cancel)?;
        eigentrail_check::families::check(&request, &report, cancel)?;
        return serialization::encode_with_cancel(&report, cancel);
    }
    let request: TraceRequest = serialization::decode_in(input, cancel)?;
    match kind {
        "common-frame" => {
            let model = common_frame::CommonFrameModel {
                weights: memory::copy(&[1., 0.])?,
                common_frame_assumed: true,
            };
            let report = common_frame::analyze(&request, &model, cancel)?;
            eigentrail_check::common_frame::check(&request, &model, &report, cancel)?;
            serialization::encode_with_cancel(&report, cancel)
        }
        "jumps" => {
            let model = jumps::JumpModel {
                at_most_one_per_cell: true,
                minimum_jump: None,
                minimum_applies_to_all_candidates: false,
            };
            let report = jumps::analyze_with_cancel(&request, &model, cancel)?;
            eigentrail_check::jumps::check_with_cancel(&request, &model, &report, cancel)?;
            serialization::encode_with_cancel(&report, cancel)
        }
        _ => {
            let report = track_with_cancel(&request, cancel)?;
            eigentrail_check::check_with_cancel(&request, &report, cancel)?;
            if kind == "cold256" {
                assert!(report.completed);
                assert_eq!(report.eigensystems.len(), 2);
                assert_eq!(report.segments.len(), 1);
                let tubes = &report.segments[0].tubes;
                assert_eq!(tubes.len(), 1);
                assert_eq!((tubes[0].start, tubes[0].end, tubes[0].bound), (0, 256, 0.));
                for row in &report.summaries {
                    assert_eq!(row.len(), 256);
                    for summary in row {
                        assert!(summary.strength.lower <= 1. && summary.strength.upper >= 1.);
                        let Quantity::Interval(share) = &summary.share else {
                            panic!("identity trace undefined");
                        };
                        assert!(share.lower <= 1. / 256. && share.upper >= 1. / 256.);
                        assert_eq!(summary.rank, [1, 256]);
                    }
                }
            }
            serialization::encode_with_cancel(&report, cancel)
        }
    }
}
fn measured_worker(name: &str, input: &[u8], kind: &str) {
    start();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let (success, covered_retained) = std::thread::scope(|scope| {
        let worker = Worker::spawn(scope, &operation, |cancel| {
            let owned = memory::copy(input)?;
            analyze(&owned, kind, &cancel)
        })
        .unwrap();
        let result = worker.recv_timeout(Duration::from_secs(300)).unwrap();
        let retained = operation.stats().live >= LIVE.load(SeqCst);
        let successful = result.is_ok();
        drop(result);
        drop(worker);
        (successful, retained)
    });
    let stats = operation.stats();
    let refunded = stats.live == baseline;
    drop(operation);
    let (live, peak, underflows) = stop();
    assert!(success, "{name} failed producer/checker worker");
    assert!(
        covered_retained && refunded,
        "{name} handoff/refund mismatch"
    );
    assert_eq!((live, underflows), (0, 0), "{name}");
    assert!(
        peak <= stats.peak && stats.peak <= stats.limit,
        "{name}: cold System {peak} ledger {} limit {}",
        stats.peak,
        stats.limit
    );
    println!(
        "{name}: cross-thread System peak={peak} ledger peak={} limit={} live_after_drop={live}",
        stats.peak, stats.limit
    );
}

#[test]
fn actual_shared_worker_cold_native_scratch_handoff_refusal_and_cancellation() {
    assert_eq!((STACK_BYTES, CONTROL_BYTES), (2 * 1024 * 1024, 64 * 1024));
    let _caller_thread = std::thread::current();
    // I012's std-only diagnostic identifies this 48-byte caller TLS cache; EVD threads stay cold.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<()>(0);
    assert!(matches!(
        receiver.recv_timeout(Duration::from_millis(1)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop((sender, receiver));
    let started = AtomicBool::new(false);
    start();
    let tiny = Operation::new(8192).unwrap();
    let refused = std::thread::scope(|scope| {
        let attempt = Worker::spawn(scope, &tiny, |_| -> Result<Vec<u8>> {
            started.store(true, SeqCst);
            Ok(Vec::new())
        });
        matches!(attempt, Err(Error::Resource(_)))
    });
    let stats = tiny.stats();
    drop(tiny);
    let (live, peak, underflows) = stop();
    assert!(refused && !started.load(SeqCst));
    assert_eq!((live, underflows), (0, 0));
    assert!(peak <= stats.peak && stats.peak <= stats.limit);
    println!(
        "worker pre-spawn refusal: System peak={peak} ledger peak={} no_worker_started=true",
        stats.peak
    );
    let mut dimension = scalar(2);
    dimension.features = (0..256).map(|i| format!("feature_{i}")).collect();
    let identity: Vec<_> = (0..256)
        .flat_map(|j| (0..256).map(move |i| if i == j { 1. } else { 0. }))
        .collect();
    dimension.covariances = vec![
        Matrix {
            n: 256,
            data: identity
        };
        2
    ];
    let dimension = checked_wire("dimension", &dimension);
    // No numerical operation precedes this new thread's first maximum-dimension EVD.
    measured_worker("cold_dimension256", &dimension, "cold256");
    let scalar2000 = wire(&scalar(2000));
    measured_worker("retained_scalar2000", &scalar2000, "trace");
    let small = wire(&scalar(2));
    measured_worker("common_frame", &small, "common-frame");
    measured_worker("jumps", &small, "jumps");
    let history = families::HistoryRequest {
        schema: families::SCHEMA.into(),
        observations: scalar(2),
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
    measured_worker("history", &wire(&history), "history");

    for cancelled in [false, true] {
        start();
        let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
        let baseline = operation.stats().live;
        let expected = std::thread::scope(|scope| {
            let worker = Worker::spawn(scope, &operation, |cancel| -> Result<Vec<u8>> {
                if cancelled {
                    loop {
                        cancel.poll()?;
                        std::thread::yield_now();
                    }
                }
                memory::capacity(resources::DEFAULT_BYTES)
            })
            .unwrap();
            if cancelled {
                worker.cancel();
            }
            let result = worker.recv_timeout(Duration::from_secs(5)).unwrap();
            let expected = if cancelled {
                matches!(result, Err(Error::Cancelled))
            } else {
                matches!(result, Err(Error::Resource(_)))
            };
            drop(result);
            drop(worker);
            expected
        });
        let stats = operation.stats();
        assert_eq!(stats.live, baseline);
        drop(operation);
        let (live, peak, underflows) = stop();
        assert!(expected);
        assert_eq!((live, underflows), (0, 0));
        assert!(peak <= stats.peak && stats.peak <= stats.limit);
        println!(
            "worker {}: cross-thread System peak={peak} ledger peak={} live_after_drop={live}",
            if cancelled { "cancelled" } else { "refused" },
            stats.peak
        );
    }
}
