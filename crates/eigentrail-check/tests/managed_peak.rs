//! One executable/thread measures managed allocations independently of reservations.
use eigentrail_core::*;
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
        if let Some(mut c) = slot.get() {
            if old > c.live {
                c.underflows += 1;
                c.live = 0;
            } else {
                c.live -= old;
            }
            c.live += new;
            c.peak = c.peak.max(c.live);
            slot.set(Some(c));
        }
    });
}
unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Delegate unchanged layouts; counters use const thread-local cells only.
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
        let q = unsafe { System.realloc(p, layout, new) };
        if !q.is_null() {
            allocation(layout.size(), new);
        }
        q
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

#[test]
fn tight_scalar_stack_is_refused_or_stays_within_actual_managed_peak() {
    // Known allocation/reallocation/drop validates the measuring code itself.
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
    assert_eq!(empty.live, 0);
    assert_eq!(empty.peak, 8192);
    assert_eq!(empty.underflows, 0);
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/resources.json")).unwrap();
    for limit in [
        oracle["tight_budget_bytes"].as_u64().unwrap() as usize,
        oracle["default_budget_bytes"].as_u64().unwrap() as usize,
    ] {
        let count = oracle["dates"].as_u64().unwrap() as usize;
        let mut request = TraceRequest::conditional(
            (0..count).map(|i| i as f64).collect(),
            vec![
                Matrix {
                    n: 1,
                    data: vec![1.]
                };
                count
            ],
            vec![0.; count],
            0.,
        );
        request.memory_bytes = limit;
        start();
        let result = (|| -> Result<_> {
            let report = track(&request)?;
            eigentrail_check::check(&request, &report)?;
            let encoded = serialization::encode(&report)?;
            Ok((report, encoded))
        })();
        let measured = stop();
        assert_eq!(
            measured.underflows, 0,
            "untracked deallocation invalidates measurement"
        );
        match result {
            Ok((report, bytes)) => {
                println!(
                    "limit={limit} observed peak={} retained={} report reservation={} encoded={}",
                    measured.peak,
                    measured.live,
                    report.managed_reservation,
                    bytes.len()
                );
                assert!(
                    measured.peak <= limit,
                    "accepted analysis exceeded managed cap: {} > {limit}",
                    measured.peak
                );
                assert_eq!(report.summaries.len(), count);
                assert!(
                    report
                        .summaries
                        .iter()
                        .all(|s| s[0].strength.lower == 1. && s[0].strength.upper == 1.)
                );
            }
            Err(Error::Resource(reason)) if limit < resources::DEFAULT_BYTES => {
                println!("tight request refused: {reason}; peak={}", measured.peak)
            }
            Err(error) => panic!("default/other failure: {error}"),
        }
    }
}
