//! Additional W3 allocation regressions; the allocator only observes System.
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

fn owned_pipeline(name: &str, wire: &[u8], expected_queries: usize, exact_scalar: bool) {
    let cancel = Cancellation::default();
    start();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let result = operation.own(|| {
        let owned = memory::copy(wire)?;
        let request: families::HistoryRequest = serialization::decode_in(&owned, &cancel)?;
        let report = families::analyze(&request, &cancel)?;
        let output = serialization::encode_with_cancel(&report, &cancel)?;
        memory::scope(|| {
            let replay: families::HistoryReport = serialization::decode_in(&output, &cancel)?;
            eigentrail_check::families::check(&request, &replay, &cancel)
        })?;
        Ok((owned, request, report, output))
    });
    let stats = operation.stats();
    let actual_retained = current();
    let successful = result.is_ok();
    if let Ok(value) = &result {
        let (_, request, report, output) = &**value;
        assert!(report.completed);
        assert_eq!(report.configurations.len(), request.configurations.len());
        assert!(stats.live >= actual_retained.live);
        assert!(stats.live >= output.capacity());
        for cfg in &report.configurations {
            assert_eq!(cfg.continuous_workflow.len(), 1);
            let workflow = &cfg.continuous_workflow[0];
            assert_eq!(workflow.queries.len(), expected_queries);
            if exact_scalar {
                assert_eq!(workflow.witnesses.len(), 1);
                assert_eq!(workflow.witnesses[0].values.len(), expected_queries);
                for (row, &time) in workflow.queries.iter().zip(&request.query_times) {
                    assert_eq!(row.time, time);
                    let continuous_queries::QueryState::Bounds { trace, blocks } = &row.state
                    else {
                        panic!("constant scalar query unresolved");
                    };
                    assert_eq!(*trace, Range::closed(2., 2.));
                    assert_eq!(blocks.len(), 1);
                    assert_eq!(blocks[0].strength, Range::closed(2., 2.));
                    assert_eq!(blocks[0].share, Quantity::Interval(Range::closed(1., 1.)));
                    assert_eq!(blocks[0].rank, [1, 1]);
                    assert_eq!(blocks[0].origins.as_slice(), [0]);
                    assert!(blocks[0].persistent);
                }
            }
        }
    }
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    drop(operation);
    let counts = stop();
    assert!(
        successful,
        "{name}: owned decode/producer/retained output/replay failed"
    );
    measured(name, stats, counts);
}

#[test]
fn all_seven_owned_v2_workflows_charge_checker_codec_and_retained_outputs() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/continuous_workflow.json"
    ))
    .unwrap();
    for case in oracle["cases"].as_array().unwrap() {
        let wire = serde_json::to_vec(&case["request"]).unwrap();
        owned_pipeline(
            case["id"].as_str().unwrap(),
            &wire,
            case["request"]["query_times"].as_array().unwrap().len(),
            false,
        );
    }
}

#[test]
fn maximum_scalar_query_count_remains_informative_with_full_owned_replay() {
    let request = families::HistoryRequest {
        schema: families::SCHEMA_V2.into(),
        observations: TraceRequest::conditional(
            vec![0., 1.],
            vec![
                Matrix {
                    n: 1,
                    data: vec![2.]
                };
                2
            ],
            vec![0.; 2],
            0.,
        ),
        configurations: vec![families::Configuration {
            id: "constant-continuous".into(),
            lipschitz: 0.,
            class: families::PathClass::Continuous,
            witnesses: vec![],
        }],
        query_times: (0..10000).map(|j| j as f64 / 9999.).collect(),
    };
    let wire = serialization::encode(&request).unwrap();
    owned_pipeline("maximum_10000_scalar_queries", &wire, 10000, true);
}
