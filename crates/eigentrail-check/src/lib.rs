//! Independent reconstruction of TRACE certificates.
pub mod association;
mod calibration;
pub mod campaign_oracle;
pub mod common_frame;
pub mod common_frame_queries;
mod continuous_queries;
pub mod crossings;
pub mod curves;
pub mod eigensystem;
pub mod evidence;
pub mod families;
mod jump_queries;
pub mod jumps;
pub mod transport;
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};

pub(crate) fn retained_bytes(values: &[usize]) -> Result<usize> {
    values.iter().try_fold(0usize, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| Error::Resource(String::new()))
    })
}

fn refusal(message: &str) -> Error {
    Error::Invalid(format!("checker: {message}"))
}

/// Uniform accuracy across the complete request-declared family, on its common
/// nodal event and conditional on truth belonging to at least one configuration.
pub fn check_uniform_accuracy(
    requests: &[TraceRequest],
    reports: &[TraceReport],
    block: [usize; 2],
    target: f64,
) -> Result<()> {
    let first_budget = requests
        .first()
        .map_or(resources::DEFAULT_BYTES, |request| request.memory_bytes);
    let retained = requests.iter().try_fold(0usize, |sum, value| {
        retained_bytes(&[sum, value.heap_bytes()?])
    })?;
    let retained = reports.iter().try_fold(retained, |sum, value| {
        retained_bytes(&[sum, value.heap_bytes()?])
    })?;
    memory::ensure_operation(first_budget, retained, || {
        nonnegative(target, "uniform target")?;
        let first = requests
            .first()
            .ok_or_else(|| refusal("empty configuration family"))?;
        if requests.len() != reports.len()
            || block[0] >= block[1]
            || block[1] > first.features.len()
        {
            return Err(refusal("incomplete family or requested block"));
        }
        for (request, report) in requests.iter().zip(reports) {
            if request.covariances != first.covariances
                || request.times != first.times
                || request.features != first.features
                || request.calibration != first.calibration
                || request.correction != first.correction
            {
                return Err(refusal(
                    "uniform family must share intended data and the same calibrated nodal event",
                ));
            }
            check(request, report)?;
            for segment in &report.segments {
                if !segment.tubes.iter().any(|tube| {
                    tube.start == block[0] && tube.end == block[1] && tube.bound < target
                }) {
                    return Err(refusal("accuracy claim fails in a retained configuration"));
                }
            }
        }
        Ok(())
    })
}
pub fn check(request: &TraceRequest, report: &TraceReport) -> Result<()> {
    let retained = retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        check_with_cancel(request, report, &Cancellation::default())
    })
}
pub fn check_with_cancel(
    request: &TraceRequest,
    report: &TraceReport,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        let reserved = eigentrail_core::validate(request)?;
        if report.schema != SCHEMA
            || !report.completed
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.managed_reservation != reserved
        {
            return Err(refusal("schema, input binding, completeness or resources"));
        }
        if report.resource_qualification
            != "pending audit of native allocations and interruption latency"
        {
            return Err(refusal("unsupported platform/resource promotion"));
        }
        let matrices = crate::calibration::inspect(
            request,
            &report.eigensystems,
            &report.radii,
            &report.calibration_scope,
            cancel,
        )?;
        let m = matrices.len();
        let p = request.features.len();
        if report.segments.len() != m - 1 || report.summaries.len() != m {
            return Err(refusal("incomplete records"));
        }
        for j in 0..m - 1 {
            memory::scope(|| {
                cancel.poll()?;
                let a = &report.eigensystems[j];
                let b = &report.eigensystems[j + 1];
                let dt = (point(request.times[j + 1]) - point(request.times[j])).sup();
                let eta = (point(report.radii[j].max(report.radii[j + 1]))
                    + point(request.lipschitz) * point(dt) / point(2.)
                    + point(a.backward.max(b.backward)))
                .sup();
                let mut gaps = memory::capacity(p.saturating_sub(1))?;
                for k in 0..p - 1 {
                    let left = (point(a.values[k])
                        - point(a.values[k + 1])
                        - point(2.) * (point(report.radii[j]) + point(a.backward)))
                    .inf();
                    let right = (point(b.values[k])
                        - point(b.values[k + 1])
                        - point(2.) * (point(report.radii[j + 1]) + point(b.backward)))
                    .inf();
                    let drift = point(request.lipschitz) * point(dt);
                    gaps.push(
                        (point(left) - point(2.) * drift)
                            .inf()
                            .max((point(right) - point(2.) * drift).inf())
                            .max(((point(left) + point(right)) / point(2.) - drift).inf()),
                    );
                }
                let mut blocks = memory::capacity(p)?;
                let mut start = 0;
                for end in 1..=p {
                    if end < p && gaps[end - 1] <= 2. * eta {
                        continue;
                    }
                    let gap = if start == 0 && end == p {
                        0.
                    } else {
                        let l = if start == 0 {
                            f64::MAX
                        } else {
                            gaps[start - 1]
                        };
                        let r = if end == p { f64::MAX } else { gaps[end - 1] };
                        l.min(r)
                    };
                    let bound = if start == 0 && end == p {
                        0.
                    } else {
                        (point(2.) * point((end - start) as f64).sqrt() * point(eta) / point(gap)
                            + (point(2.) + point(a.polar.max(b.polar)))
                                * point(a.polar.max(b.polar)))
                        .sup()
                        .min(1.)
                    };
                    blocks.push(Tube {
                        start,
                        end,
                        eta,
                        gap,
                        bound,
                        informative: bound < request.target,
                    });
                    start = end;
                }
                for block in &mut blocks {
                    block.informative &= eigentrail_core::budget::numerical_component(
                        request,
                        &report.eigensystems,
                        j,
                        block,
                    )? <= request.numerical_allowance;
                }
                let s = &report.segments[j];
                if s.tubes != blocks {
                    return Err(refusal("incorrect block universe or tube bound"));
                }
                let graph = association::reconstruct(a, b, &blocks, request.lipschitz, dt, cancel)?;
                if s.edges != graph
                    || s.association != association::classify_checked(&graph, cancel)?
                {
                    return Err(refusal("incomplete competitors or false matching claim"));
                }

                Ok(())
            })?;
        }
        // Marginal summaries are checked independently, and never promoted to a complete history.
        for j in 0..m {
            memory::scope(|| {
                let e = &report.eigensystems[j];
                let err = point(report.radii[j]) + point(e.backward);
                if report.summaries[j].len() != p {
                    return Err(refusal("missing strengths"));
                }
                let mut low = memory::capacity(p)?;
                let mut high = memory::capacity(p)?;
                for &x in &e.values {
                    low.push((point(x) - err).inf().max(0.));
                    high.push((point(x) + err).sup().max(0.));
                }
                let tr = e.values.iter().fold(point(0.), |s, &x| s + point(x));
                let tl = (tr - point(p as f64) * err).inf();
                let tu = (tr + point(p as f64) * err).sup();
                for k in 0..p {
                    let s = &report.summaries[j][k];
                    let ranks = [
                        1 + low.iter().filter(|x| **x > high[k]).count(),
                        p - high.iter().filter(|x| **x < low[k]).count(),
                    ];
                    let share = if tl > 0. {
                        Quantity::Interval(Range::closed(
                            (point(low[k]) / point(tu)).inf().max(0.),
                            (point(high[k]) / point(tl)).sup().min(1.),
                        ))
                    } else {
                        Quantity::Undefined(memory::text("trace may be zero")?)
                    };
                    if s.strength != Range::closed(low[k], high[k])
                        || s.rank != ranks
                        || s.share != share
                    {
                        return Err(refusal("summary interval, rank or trace gate"));
                    }
                }

                Ok(())
            })?;
        }
        let mut chain_verified = true;
        for j in 1..m {
            let current = &matrices[j];
            let previous = &matrices[j - 1];
            let change = if p == 1 {
                (point(current.data[0]) - point(previous.data[0]))
                    .abs()
                    .sup()
            } else if p == 2 {
                let x = point(current.at(0, 0)) - point(previous.at(0, 0));
                let z = point(current.at(1, 1)) - point(previous.at(1, 1));
                let y = point(current.at(0, 1)) - point(previous.at(0, 1));
                (((x + z).abs() + (square(x - z) + point(4.) * square(y)).sqrt()) / point(2.)).sup()
            } else {
                let mut data = memory::capacity(p * p)?;
                let mut rounded = point(0.);
                for (&x, &y) in current.data.iter().zip(&previous.data) {
                    let v = x - y;
                    rounded += square(point(x) - point(y) - point(v));
                    data.push(v);
                }
                let d = Matrix { n: p, data };
                let proposal = numerics::eigensystem(&d, cancel)?;
                eigensystem::inspect(&d, &proposal, cancel)?;
                (point(proposal.values.iter().fold(0f64, |s, x| s.max(x.abs())))
                    + point(proposal.backward)
                    + rounded.sqrt())
                .sup()
            };
            let limit =
                point(request.lipschitz) * (point(request.times[j]) - point(request.times[j - 1]));
            if change > limit.inf() {
                chain_verified = false;
                break;
            }
        }
        let expected = if chain_verified {
            "checked affine witness in original bare continuous class"
        } else {
            "unresolved: observed chain did not verify; no infeasibility claim"
        };
        if report.witness != expected {
            return Err(refusal("witness scope"));
        }
        Ok(())
    })
}
