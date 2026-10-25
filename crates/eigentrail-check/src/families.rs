//! Reconstruct whole-history scopes without using the producer's family decisions.
use eigentrail_core::{
    families::*,
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
fn bad(reason: &str) -> Error {
    Error::Invalid(format!("history checker: {reason}"))
}

fn grid(request: &HistoryRequest, cancel: &Cancellation) -> Result<Vec<GridNode>> {
    memory::scope(|| {
        let capacity = request
            .observations
            .times
            .len()
            .checked_add(request.query_times.len())
            .ok_or_else(|| Error::Resource(String::new()))?;
        let mut result: Vec<GridNode> = memory::capacity(capacity)?;
        for (i, &time) in request.observations.times.iter().enumerate() {
            cancel.poll()?;
            let mut observations = memory::capacity(1)?;
            observations.push(i);
            result.push(GridNode {
                time,
                observations,
                queries: Vec::new(),
            });
        }
        for (i, &time) in request.query_times.iter().enumerate() {
            cancel.poll()?;
            match result.binary_search_by(|node| node.time.partial_cmp(&time).unwrap()) {
                Ok(j) => memory::push(&mut result[j].queries, i)?,
                Err(j) => {
                    let mut queries = memory::capacity(1)?;
                    queries.push(i);
                    result.insert(
                        j,
                        GridNode {
                            time,
                            observations: Vec::new(),
                            queries,
                        },
                    );
                }
            }
        }
        Ok(result)
    })
}

pub(crate) fn difference(a: &Matrix, b: &Matrix, cancel: &Cancellation) -> Result<f64> {
    memory::scope(|| {
        let n = a.n;
        if n == 1 {
            return upper((point(a.data[0]) - point(b.data[0])).abs());
        }
        if n == 2 {
            let x = point(a.at(0, 0)) - point(b.at(0, 0));
            let z = point(a.at(1, 1)) - point(b.at(1, 1));
            let y = point(a.at(0, 1)) - point(b.at(0, 1));
            return upper(
                ((x + z).abs() + (square(x - z) + point(4.) * square(y)).sqrt()) / point(2.),
            );
        }
        let mut data = resources::zeros(n * n)?;
        let mut rounding = point(0.);
        for (i, value) in data.iter_mut().enumerate() {
            let exact = point(a.data[i]) - point(b.data[i]);
            *value = a.data[i] - b.data[i];
            if !value.is_finite() {
                return Err(Error::Exhausted("witness difference overflow".into()));
            }
            rounding += square(exact - point(*value));
        }
        let intended = Matrix { n, data };
        let e = numerics::eigensystem(&intended, cancel)?;
        crate::eigensystem::inspect(&intended, &e, cancel)?;
        upper(
            point(e.values.iter().fold(0f64, |s, x| s.max(x.abs())))
                + point(e.backward)
                + rounding.sqrt(),
        )
    })
}

fn feasibility(
    request: &TraceRequest,
    cfg: &Configuration,
    report: &TraceReport,
    workflow: Option<&common_frame_queries::Workflow>,
    cancel: &Cancellation,
) -> Result<Feasibility> {
    memory::scope(|| {
        if matches!(cfg.class, PathClass::CommonFrame { .. }) {
            return match cfg.witnesses.first() {
                Some(witness) => crate::common_frame_queries::explicit_feasibility(
                    request,
                    &report.radii,
                    witness,
                    cancel,
                ),
                None => workflow
                    .and_then(|w| w.witnesses.first())
                    .map(|w| w.feasibility.clone())
                    .ok_or_else(|| bad("missing checked common-frame witness")),
            };
        }
        if !matches!(cfg.class, PathClass::Continuous) {
            return Ok(if cfg.witnesses.is_empty() {
                Feasibility::Unresolved
            } else {
                Feasibility::UnsupportedModel
            });
        }
        let observations = calibration::normalized(request)?;
        let (nodes, widening) = cfg
            .witnesses
            .first()
            .map_or((observations.as_slice(), 0.), |w| {
                (w.nodes.as_slice(), w.widening)
            });
        for (j, node) in nodes.iter().enumerate() {
            cancel.poll()?;
            let proposal = numerics::eigensystem(node, cancel)?;
            crate::eigensystem::inspect(node, &proposal, cancel)?;
            if !crate::eigensystem::psd_checked(node, &proposal, cancel)? {
                return Ok(Feasibility::Unresolved);
            }
            let radius = (point(report.radii[j]) + point(widening)).inf();
            if difference(node, &observations[j], cancel)? > radius {
                return Ok(Feasibility::Unresolved);
            }
            if j > 0 {
                let step = point(request.lipschitz)
                    * (point(request.times[j]) - point(request.times[j - 1]));
                if difference(node, &nodes[j - 1], cancel)? > step.inf() {
                    return Ok(Feasibility::Unresolved);
                }
            }
        }
        Ok(if widening == 0. {
            Feasibility::OriginalAffineWitness
        } else {
            Feasibility::WidenedAffineWitness {
                additional: widening,
            }
        })
    })
}

pub fn check(
    request: &HistoryRequest,
    report: &HistoryReport,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.observations.memory_bytes, retained, || {
        cancel.poll()?;
        let reservation = eigentrail_core::families::validate_request(request)?;
        if report.schema != request.schema
            || !report.completed
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.scope != SCOPE
            || report.resource_qualification != RESOURCE_SCOPE
            || report.managed_reservation != reservation
            || report.configurations.len() != request.configurations.len()
        {
            return Err(bad(
                "schema, binding, configuration universe, scope or resources",
            ));
        }
        let expected_grid = grid(request, cancel)?;
        for (cfg, r) in request.configurations.iter().zip(&report.configurations) {
            memory::scope(|| {
                cancel.poll()?;
                if r.id != cfg.id
                    || r.class != cfg.class
                    || r.lipschitz != cfg.lipschitz
                    || r.grid != expected_grid
                {
                    return Err(bad("configuration constraints or joint query grid"));
                }
                let mut scoped = request.observations.try_clone()?;
                scoped.lipschitz = cfg.lipschitz;
                crate::check_with_cancel(&scoped, &r.continuous_conditional, cancel)?;
                match &cfg.class {
                    PathClass::InitializedTransport { model } => {
                        if request.schema != eigentrail_core::families::SCHEMA_V2
                            || r.transports.len() != 1
                        {
                            return Err(bad("transport model schema or assessment universe"));
                        }
                        crate::transport::check_history(
                            &scoped,
                            &r.continuous_conditional,
                            model,
                            &r.transports[0],
                            cancel,
                        )?;
                    }
                    _ if !r.transports.is_empty() => {
                        return Err(bad("transport evidence in a different history model"));
                    }
                    _ => {}
                }
                match &cfg.class {
                    PathClass::TransverseCrossings { windows } => {
                        if request.schema != eigentrail_core::families::SCHEMA_V2
                            || r.crossings.len() != windows.len()
                        {
                            return Err(bad("crossing model schema or complete window universe"));
                        }
                        for (window, assessment) in windows.iter().zip(&r.crossings) {
                            cancel.poll()?;
                            crate::crossings::check(&scoped, window, assessment, cancel)?;
                            if assessment.baseline != r.continuous_conditional {
                                return Err(bad(
                                    "crossing assessment uses a different common event or baseline",
                                ));
                            }
                        }
                    }
                    _ if !r.crossings.is_empty() => {
                        return Err(bad("crossing evidence in a different history model"));
                    }
                    _ => {}
                }
                match &cfg.class {
                    PathClass::CommonFrame { model } => {
                        if r.common_frame.len() != 1 || r.common_frame_queries.len() != 1 {
                            return Err(bad("missing common-frame assessment"));
                        }
                        crate::common_frame_queries::check(
                            &scoped,
                            model,
                            &r.common_frame[0],
                            &request.query_times,
                            cfg.witnesses.is_empty(),
                            &r.common_frame_queries[0],
                            cancel,
                        )?;
                        let (radii, calibration_scope) = match &r.common_frame[0] {
                            common_frame::CommonFrameAssessment::Resolved { report } => {
                                (&report.radii, &report.calibration_scope)
                            }
                            common_frame::CommonFrameAssessment::UnresolvedAnchor {
                                radii,
                                calibration_scope,
                                ..
                            } => (radii, calibration_scope),
                        };
                        if *radii != r.continuous_conditional.radii
                            || *calibration_scope != r.continuous_conditional.calibration_scope
                        {
                            return Err(bad(
                                "common-frame assessment uses a different nodal event",
                            ));
                        }
                    }
                    _ if !r.common_frame.is_empty() || !r.common_frame_queries.is_empty() => {
                        return Err(bad("common-frame evidence in a different model"));
                    }
                    _ => {}
                }
                match &cfg.class {
                    PathClass::Continuous
                    | PathClass::CommonFrame { .. }
                    | PathClass::TransverseCrossings { .. }
                    | PathClass::InitializedTransport { .. }
                        if !r.jumps.is_empty() =>
                    {
                        return Err(bad(
                            "jump model introduced into bare continuous configuration",
                        ));
                    }
                    PathClass::Continuous
                    | PathClass::CommonFrame { .. }
                    | PathClass::TransverseCrossings { .. }
                    | PathClass::InitializedTransport { .. } => {}
                    PathClass::Jumps { model } => {
                        if r.jumps.len() != 1 {
                            return Err(bad("missing checked occupancy family"));
                        }
                        crate::jumps::check_with_cancel(&scoped, model, &r.jumps[0], cancel)?;
                        if r.jumps[0].radii != r.continuous_conditional.radii
                            || r.jumps[0].calibration_scope
                                != r.continuous_conditional.calibration_scope
                        {
                            return Err(bad("jump exclusions use a different nodal envelope"));
                        }
                    }
                }
                let count = scoped.times.len() - 1;
                if r.cells.len() != count {
                    return Err(bad("missing cells"));
                }
                let mut smooth = memory::capacity(count)?;
                for j in 0..count {
                    cancel.poll()?;
                    let expected = match r.jumps.first() {
                        Some(jump) if jump.family.mandatory[j] || jump.family.optional[j] => {
                            CellScope::JumpPossible {
                                cell: j,
                                mandatory: jump.family.mandatory[j],
                            }
                        }
                        _ => CellScope::Smooth { segment: j },
                    };
                    smooth.push(matches!(expected, CellScope::Smooth { .. }));
                    if r.cells[j] != expected {
                        return Err(bad(
                            "continuous assertion crosses a retained jump alternative",
                        ));
                    }
                }
                let mut expected_overlaps = memory::capacity(0)?;
                for date in 1..count {
                    if !smooth[date - 1] || !smooth[date] {
                        continue;
                    }
                    let left = &r.continuous_conditional.segments[date - 1].tubes;
                    let right = &r.continuous_conditional.segments[date].tubes;
                    for (a, x) in left.iter().enumerate() {
                        cancel.poll()?;
                        for (b, y) in right.iter().enumerate() {
                            let common_start = x.start.max(y.start);
                            let common_end = x.end.min(y.end);
                            if common_start < common_end {
                                memory::push(
                                    &mut expected_overlaps,
                                    PartitionOverlap {
                                        date,
                                        left_block: a,
                                        right_block: b,
                                        rank: common_end - common_start,
                                    },
                                )?;
                            }
                        }
                    }
                }
                if r.partition_overlaps != expected_overlaps {
                    return Err(bad("incomplete block merge/split constraints"));
                }
                let jumps_v2 = request.schema == eigentrail_core::families::SCHEMA_V2
                    && matches!(cfg.class, PathClass::Jumps { .. });
                if r.jump_workflow.len() != usize::from(jumps_v2) {
                    return Err(bad("jump workflow schema or model scope"));
                }
                let expected_feasibility = if jumps_v2 {
                    crate::jump_queries::check(
                        &scoped,
                        cfg,
                        &r.continuous_conditional,
                        &r.jumps[0],
                        &request.query_times,
                        &r.jump_workflow[0],
                        cancel,
                    )?
                } else {
                    feasibility(
                        &scoped,
                        cfg,
                        &r.continuous_conditional,
                        r.common_frame_queries.first(),
                        cancel,
                    )?
                };
                if r.feasibility != expected_feasibility {
                    return Err(bad(
                        "whole witness feasibility or configuration/widening scope",
                    ));
                }
                let continuous_v2 = request.schema == eigentrail_core::families::SCHEMA_V2
                    && cfg.class == PathClass::Continuous;
                if r.continuous_workflow.len() != usize::from(continuous_v2) {
                    return Err(bad("continuous workflow schema or model scope"));
                }
                if continuous_v2 {
                    crate::continuous_queries::check(
                        &scoped,
                        cfg,
                        &r.continuous_conditional,
                        &request.query_times,
                        &expected_feasibility,
                        &r.continuous_workflow[0],
                        cancel,
                    )?;
                }
                Ok(())
            })?;
        }
        cancel.poll()?;
        Ok(())
    })
}
