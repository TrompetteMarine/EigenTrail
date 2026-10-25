//! Independent System observation of owned W6 jump queries and witness replay.
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

fn pipeline(name: &str, input: &[u8], maximum: bool) {
    let cancel = Cancellation::default();
    start();
    let operation = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = operation.stats().live;
    let result = operation.own(|| {
        let input = memory::copy(input)?;
        let request: families::HistoryRequest = serialization::decode_in(&input, &cancel)?;
        let report = families::analyze(&request, &cancel)?;
        let output = serialization::encode_with_cancel(&report, &cancel)?;
        memory::scope(|| {
            let replay: families::HistoryReport = serialization::decode_in(&output, &cancel)?;
            eigentrail_check::families::check(&request, &replay, &cancel)
        })?;
        assert!(report.completed);
        if maximum {
            let workflow = &report.configurations[0].jump_workflow[0];
            assert_eq!(workflow.queries.len(), 10000);
            assert!(workflow.components.is_empty());
            assert_eq!(workflow.witnesses.len(), 1);
            assert_eq!(workflow.witnesses[0].values.len(), 10000);
            assert!(workflow.witnesses[0].cells.is_empty());
            assert_eq!(
                report.configurations[0].feasibility,
                families::Feasibility::OriginalStepWitness
            );
            for query in &workflow.queries {
                let jump_queries::State::JumpPossible {
                    sides, mandatory, ..
                } = &query.state
                else {
                    panic!("lost retained jump alternatives")
                };
                assert!(!mandatory);
                assert_eq!(sides.len(), 2);
            }
        }
        Ok((input, request, report, output))
    });
    let stats = operation.stats();
    let observed = current();
    let success = result.is_ok();
    if let Ok(value) = &result {
        assert!(stats.live >= observed.live);
        assert!(stats.live >= value.3.capacity());
    }
    if let Err(error) = &result {
        eprintln!("{name}: {error:?}");
    }
    drop(result);
    assert_eq!(operation.stats().live, baseline);
    drop(operation);
    let counts = stop();
    assert!(success, "{name}: owned integrated pipeline failed");
    measured(name, stats, counts);
}

#[test]
fn all_integrated_histories_charge_both_sides_components_witnesses_codec_and_checker() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/integrated_histories.json"
    ))
    .unwrap();
    for case in oracle["cases"].as_array().unwrap() {
        pipeline(
            case["id"].as_str().unwrap(),
            &serde_json::to_vec(&case["request"]).unwrap(),
            false,
        );
    }
}

#[test]
fn ten_thousand_jump_queries_keep_both_alternatives_and_full_step_witness_replay() {
    let observations = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    );
    let request = families::HistoryRequest {
        schema: families::SCHEMA_V2.into(),
        observations,
        configurations: vec![families::Configuration {
            id: "maximum-jump-queries".into(),
            lipschitz: 0.,
            class: families::PathClass::Jumps {
                model: jumps::JumpModel {
                    at_most_one_per_cell: true,
                    minimum_jump: None,
                    minimum_applies_to_all_candidates: false,
                },
            },
            witnesses: vec![],
        }],
        query_times: vec![0.5; 10000],
    };
    pipeline(
        "maximum-jump-queries",
        &serialization::encode(&request).unwrap(),
        true,
    );
}
