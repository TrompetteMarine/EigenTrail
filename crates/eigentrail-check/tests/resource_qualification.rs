//! Frozen finite native-resource cases; the allocator only observes System.
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, HeapBytes, Operation},
    },
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
fn fixture<T: serde::Serialize>(name: &str, record: &T) -> Vec<u8> {
    let encoded = serialization::encode(record).unwrap();
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/resource_qualification.json"
    ))
    .unwrap();
    assert_eq!(
        encoded.len(),
        oracle["wire"][name]["bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&encoded)),
        oracle["wire"][name]["sha256"]
    );
    encoded
}
fn unit(summary: &Summary) {
    assert_eq!(summary.strength, Range::closed(1., 1.));
    assert_eq!(summary.share, Quantity::Interval(Range::closed(1., 1.)));
    assert_eq!(summary.rank, [1, 1]);
}

#[test]
fn maximum_dates_owned_pipeline_keeps_all_dates_and_refunds() {
    let wire = fixture("dates", &scalar(10000));
    let cancel = Cancellation::default();
    start();
    let op = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = op.stats().live;
    let result = op.own(|| {
        let owned = memory::copy(&wire)?;
        let request: TraceRequest = serialization::decode_in(&owned, &cancel)?;
        let report = track_with_cancel(&request, &cancel)?;
        eigentrail_check::check_with_cancel(&request, &report, &cancel)?;
        let output = serialization::encode_with_cancel(&report, &cancel)?;
        Ok((owned, request, report, output))
    });
    let retained = current();
    let stats = op.stats();
    let success = result.is_ok();
    if let Ok(value) = &result {
        let (_, request, report, _) = &**value;
        assert_eq!(request.times.len(), 10000);
        assert!(report.completed);
        assert_eq!(report.summaries.len(), 10000);
        for row in &report.summaries {
            assert_eq!(row.len(), 1);
            unit(&row[0]);
        }
        assert_eq!(report.segments.len(), 9999);
        for segment in &report.segments {
            assert!(matches!(&segment.association, Association::Unique(m) if m.as_slice() == [0]));
        }
        assert!(stats.live >= retained.live);
    }
    drop(result);
    assert_eq!(op.stats().live, baseline);
    drop(op);
    let counts = stop();
    assert!(success, "maximum-date producer/checker failed");
    measured("max_dates", stats, counts);
}

#[test]
fn all_sixteen_configurations_and_query_outputs_coexist_with_replay() {
    let request = families::HistoryRequest {
        schema: families::SCHEMA.into(),
        observations: scalar(2),
        configurations: (0..16)
            .map(|i| families::Configuration {
                id: format!("cfg{i:02}"),
                lipschitz: 0.,
                witnesses: vec![],
                class: families::PathClass::CommonFrame {
                    model: common_frame::CommonFrameModel {
                        weights: vec![1., 0.],
                        common_frame_assumed: true,
                    },
                },
            })
            .collect(),
        query_times: vec![0., 0.5, 1.],
    };
    let wire = fixture("configurations", &request);
    let cancel = Cancellation::default();
    start();
    let op = Operation::new(resources::DEFAULT_BYTES).unwrap();
    let baseline = op.stats().live;
    let result = op.own(|| {
        let owned = memory::copy(&wire)?;
        let request: families::HistoryRequest = serialization::decode_in(&owned, &cancel)?;
        let report = families::analyze(&request, &cancel)?;
        let output = serialization::encode_with_cancel(&report, &cancel)?;
        let replay: families::HistoryReport = serialization::decode_in(&output, &cancel)?;
        eigentrail_check::families::check(&request, &replay, &cancel)?;
        Ok((owned, request, report, output))
    });
    let stats = op.stats();
    let retained = current();
    let success = result.is_ok();
    if let Ok(value) = &result {
        let (_, request, report, _) = &**value;
        assert!(report.completed);
        assert_eq!(report.configurations.len(), 16);
        for (cfg, expected) in report.configurations.iter().zip(&request.configurations) {
            assert_eq!(cfg.id, expected.id);
            assert_eq!(cfg.common_frame_queries.len(), 1);
            let workflow = &cfg.common_frame_queries[0];
            assert_eq!(workflow.queries.len(), 3);
            for (row, &time) in workflow.queries.iter().zip(&request.query_times) {
                assert_eq!(row.time, time);
                let common_frame_queries::QueryState::Bounds { summaries, trace } = &row.state
                else {
                    panic!("scalar common-frame query unresolved");
                };
                assert_eq!(*trace, Range::closed(1., 1.));
                assert_eq!(summaries.len(), 1);
                unit(&summaries[0]);
            }
        }
        assert!(stats.live >= retained.live);
    }
    drop(result);
    assert_eq!(op.stats().live, baseline);
    drop(op);
    let counts = stop();
    assert!(success, "configuration ceiling failed");
    measured("max_configurations", stats, counts);
}

#[test]
fn oversized_borrowed_capacity_cannot_bypass_active_admission() {
    let limit = 8 * 1024 * 1024;
    let mut request = scalar(2);
    request.memory_bytes = limit;
    let Calibration::Conditional { assumptions, .. } = &mut request.calibration else {
        unreachable!()
    };
    *assumptions = String::with_capacity(limit);
    assumptions.push_str("declared nodal envelope and global regularity");
    assert!(request.heap_bytes().unwrap() > limit);
    let cancel = Cancellation::default();
    for active in [false, true] {
        start();
        let op = Operation::new(limit).unwrap();
        let baseline = op.stats().live;
        let result = if active {
            op.run(|| track_with_cancel(&request, &cancel))
        } else {
            track_with_cancel(&request, &cancel)
        };
        let refused = matches!(result, Err(Error::Resource(_)));
        drop(result);
        let stats = op.stats();
        assert_eq!(stats.live, baseline);
        drop(op);
        let counts = stop();
        assert!(refused, "borrowed capacity accepted (active={active})");
        measured(
            if active {
                "borrowed_active"
            } else {
                "borrowed_standalone"
            },
            stats,
            counts,
        );
    }
    assert_eq!(256_u64 * 256 * 10000 * 8, 5_242_880_000);
    assert!(matches!(
        resources::preflight(256, 10000, resources::DEFAULT_BYTES),
        Err(Error::Resource(_))
    ));
}

fn endpoint() -> Eigensystem {
    Eigensystem {
        values: vec![2., 1.],
        vectors: Matrix {
            n: 2,
            data: vec![1., 0., 0., 1.],
        },
        backward: 0.,
        polar: 0.,
    }
}
fn stationary_contract() -> transport::TransportContract {
    transport::TransportContract {
        reference_speed: 0.,
        max_speed: 0.,
        endpoint_error: 0.,
        initial_mismatch: 0.,
        target: 0.01,
    }
}

#[test]
fn transport_counts_actual_endpoint_capacity_and_refunds_cell_scratch() {
    let cancel = Cancellation::default();
    let limit = 8 * 1024 * 1024;
    let mut oversized = vec![endpoint(), endpoint()];
    oversized[0].values = Vec::with_capacity(limit / 8);
    oversized[0].values.extend_from_slice(&[2., 1.]);
    start();
    let op = Operation::new(limit).unwrap();
    let result = op.run(|| {
        transport::transport_blocks(
            &[0., 1.],
            &oversized,
            [0, 1],
            &stationary_contract(),
            &cancel,
        )
    });
    let refused = matches!(result, Err(Error::Resource(_)));
    drop(result);
    let stats = op.stats();
    drop(op);
    let counts = stop();
    assert!(refused, "unowned endpoint capacity accepted");
    measured("transport_borrowed_capacity", stats, counts);

    let times: Vec<_> = (0..=4096).map(|i| i as f64 / 4096.).collect();
    let nodes = vec![endpoint(); times.len()];
    let visible = std::mem::size_of_val(times.as_slice())
        + std::mem::size_of_val(nodes.as_slice())
        + nodes.iter().map(|e| e.heap_bytes().unwrap()).sum::<usize>();
    start();
    let op = Operation::new(limit).unwrap();
    let baseline = op.stats().live;
    let result = op.own(|| {
        transport::transport_blocks(&times, &nodes, [0, 1], &stationary_contract(), &cancel)
    });
    let stats = op.stats();
    let success = result.is_ok();
    if let Ok(value) = &result {
        assert_eq!(value.rotation.data, [1., 0., 0., 1.]);
        assert_eq!(value.speed_upper, 0.);
        assert!(value.product_error <= 0.001 && value.comparison_error < 0.01);
    }
    drop(result);
    assert_eq!(op.stats().live, baseline);
    drop(op);
    let counts = stop();
    assert!(success, "stationary transport failed");
    assert!(
        stats.peak <= visible + 8192 + 64 * 1024,
        "per-cell charges retained: {} > visible {visible} + fixed allowance",
        stats.peak
    );
    measured("stationary_transport_4096", stats, counts);
}

#[test]
fn cancellation_polling_and_exhaustion_are_explicit() {
    let wire = fixture("dates", &scalar(10000));
    let cancel = Cancellation::default();
    cancel.cancel();
    let op = Operation::new(resources::DEFAULT_BYTES).unwrap();
    assert!(matches!(
        op.run(|| serialization::decode_in::<TraceRequest>(&wire, &cancel)),
        Err(Error::Cancelled)
    ));
    let fresh = Cancellation::default();
    let mut polls = 0;
    let result = serialization::input::preflight_with_poll(
        std::str::from_utf8(&wire).unwrap(),
        &[],
        serialization::input::RequestKind::Trace,
        || {
            polls += 1;
            if polls == 4 {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(polls, 4);
    let times: Vec<_> = (0..65538).map(|i| i as f64).collect();
    let nodes = vec![[1., 0.]; times.len()];
    assert!(matches!(
        transport::transport_lines(&times, &nodes, &stationary_contract(), &fresh),
        Err(Error::Exhausted(_))
    ));
}
