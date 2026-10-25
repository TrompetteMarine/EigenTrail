//! W4 independent System allocation observation; the allocator only observes System.
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

fn pipeline(name: &str, input: &[u8], model: &[u8]) {
    let cancel = Cancellation::default();
    start();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let result = operation.own(|| {
        let input = memory::copy(input)?;
        let model = memory::copy(model)?;
        let request: TraceRequest = serialization::decode_in(&input, &cancel)?;
        let window: crossings::CrossingWindow = serialization::decode_in(&model, &cancel)?;
        let report = crossings::analyze(&request, &window, &cancel)?;
        let output = serialization::encode_with_cancel(&report, &cancel)?;
        memory::scope(|| {
            let replay: crossings::CrossingAssessment = serialization::decode_in(&output, &cancel)?;
            eigentrail_check::crossings::check(&request, &window, &replay, &cancel)
        })?;
        Ok((input, model, (request, report), output))
    });
    let stats = operation.stats();
    let observed = current();
    let succeeded = result.is_ok();
    if let Ok(value) = &result {
        assert!(value.2.1.completed);
        assert!(stats.live >= observed.live);
        assert!(stats.live >= value.3.capacity());
    }
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    drop(operation);
    let counts = stop();
    assert!(succeeded, "{name}: owned crossing pipeline failed");
    measured(name, stats, counts);
}

#[test]
fn all_frozen_crossings_charge_owned_inputs_proposals_graphs_and_checker() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/transverse_crossings.json"
    ))
    .unwrap();
    for case in oracle["cases"].as_array().unwrap() {
        pipeline(
            case["id"].as_str().unwrap(),
            &serde_json::to_vec(&case["request"]).unwrap(),
            &serde_json::to_vec(&case["window"]).unwrap(),
        );
    }
}

#[test]
fn full_space_rank_64_charges_interval_scratch_and_complete_attachment_graphs() {
    let p = 64;
    let nodes = [-1. / 64., 0., 1. / 64.]
        .iter()
        .map(|&t| {
            let mut data = vec![0.; p * p];
            for i in 0..p {
                data[i + i * p] = 100. + (i as f64 - 31.) * t;
            }
            Matrix { n: p, data }
        })
        .collect();
    let request = TraceRequest::conditional(
        vec![-1. / 64., 0., 1. / 64.],
        nodes,
        vec![1. / (1u64 << 40) as f64; 3],
        32.,
    );
    let window = crossings::CrossingWindow {
        block: [0, p],
        centre: 1,
        secant: [0, 2],
        attachments: [0, 2],
        bracket: [-1. / 4096., 1. / 4096.],
        curvature: 0.,
        exact_collision_assumed: true,
    };
    let cancel = Cancellation::default();
    let report = crossings::analyze(&request, &window, &cancel).unwrap();
    assert_eq!(report.outcome, crossings::CrossingOutcome::Resolved);
    assert_eq!(report.proof[0].left_candidates.len(), p);
    assert_eq!(report.proof[0].right_candidates.len(), p);
    pipeline(
        "rank64_fullspace",
        &serialization::encode(&request).unwrap(),
        &serialization::encode(&window).unwrap(),
    );
}
