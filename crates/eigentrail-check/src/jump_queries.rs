//! Reconstruct jump components, both one-sided alternatives and the whole step witness.
use eigentrail_core::{
    families::{Configuration, Feasibility, PathClass},
    jump_queries::{Component, SCOPE, State, Workflow},
    jumps::JumpReport,
    resources::{Cancellation, memory},
    scalar::*,
    *,
};

fn bad() -> Error {
    Error::Invalid("jump history checker: component, query, summary or step witness claim".into())
}

fn check_summary(
    eigen: &Eigensystem,
    radius: f64,
    trace: &Range,
    modes: &[Summary],
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        let n = eigen.values.len();
        if modes.len() != n {
            return Err(bad());
        }
        let uncertainty = point(radius) + point(eigen.backward);
        let sum = eigen.values.iter().fold(point(0.), |s, &v| s + point(v));
        let lower_trace = (sum - point(n as f64) * uncertainty).inf();
        let upper_trace = (sum + point(n as f64) * uncertainty).sup();
        if *trace != Range::closed(lower_trace.max(0.), upper_trace.max(0.)) {
            return Err(bad());
        }
        let mut strengths = memory::capacity(n)?;
        for &value in &eigen.values {
            cancel.poll()?;
            strengths.push(Range::closed(
                (point(value) - uncertainty).inf().max(0.),
                (point(value) + uncertainty).sup().max(0.),
            ));
        }
        for (mode, strength) in modes.iter().zip(&strengths) {
            cancel.poll()?;
            let rank = [
                1 + strengths
                    .iter()
                    .filter(|s| s.lower > strength.upper)
                    .count(),
                n - strengths
                    .iter()
                    .filter(|s| s.upper < strength.lower)
                    .count(),
            ];
            if mode.strength != *strength || mode.rank != rank {
                return Err(bad());
            }
            if lower_trace > 0. {
                let expected = Range::closed(
                    (point(strength.lower) / point(upper_trace)).inf().max(0.),
                    (point(strength.upper) / point(lower_trace)).sup().min(1.),
                );
                if mode.share != Quantity::Interval(expected) {
                    return Err(bad());
                }
            } else if !matches!(&mode.share, Quantity::Undefined(reason) if reason == "trace may be zero")
            {
                return Err(bad());
            }
        }
        Ok(())
    })
}

fn components(
    baseline: &TraceReport,
    jump: &JumpReport,
    cancel: &Cancellation,
) -> Result<Vec<Component>> {
    memory::scope(|| {
        let mut result = Vec::new();
        let count = baseline.segments.len();
        let mut position = 0;
        while position < count {
            cancel.poll()?;
            if jump.family.mandatory[position] || jump.family.optional[position] {
                position += 1;
                continue;
            }
            let first = position;
            while position < count
                && !jump.family.mandatory[position]
                && !jump.family.optional[position]
            {
                position += 1;
            }
            let lineage =
                crate::continuous_queries::derive_lineage_range(baseline, first, position, cancel)?;
            memory::push(
                &mut result,
                Component {
                    cells: [first, position],
                    lineage,
                },
            )?;
        }
        Ok(result)
    })
}

fn check_step(
    request: &TraceRequest,
    configuration: &Configuration,
    baseline: &TraceReport,
    jump: &JumpReport,
    times: &[f64],
    workflow: &Workflow,
    cancel: &Cancellation,
) -> Result<Feasibility> {
    memory::scope(|| {
        if !configuration.witnesses.is_empty() {
            if !workflow.witnesses.is_empty() {
                return Err(bad());
            }
            return Ok(Feasibility::UnsupportedModel);
        }
        let PathClass::Jumps { model } = &configuration.class else {
            return Err(bad());
        };
        let nodes = calibration::normalized(request)?;
        let mut changed = Vec::new();
        for cell in 0..nodes.len() - 1 {
            cancel.poll()?;
            // Numerical equality treats the two encodings of zero as one covariance entry.
            if nodes[cell].data == nodes[cell + 1].data {
                continue;
            }
            memory::push(&mut changed, cell)?;
            if let Some(minimum) = model
                .minimum_jump
                .filter(|_| model.minimum_applies_to_all_candidates)
            {
                let mut rounding = point(0.);
                for (entry, (&left, &right)) in nodes[cell]
                    .data
                    .iter()
                    .zip(&nodes[cell + 1].data)
                    .enumerate()
                {
                    if entry % 256 == 0 {
                        cancel.poll()?;
                    }
                    let represented = right - left;
                    if !represented.is_finite() {
                        return Err(Error::Exhausted("step witness difference overflow".into()));
                    }
                    rounding += square(point(right) - point(left) - point(represented));
                }
                // The complete difference proposal was checked against these intended
                // global matrices by jumps::check_with_cancel before this workflow.
                let proposal = &jump.differences[cell];
                let spectral = proposal
                    .values
                    .iter()
                    .fold(0f64, |bound, value| bound.max(value.abs()));
                let norm_lower = (point(spectral) - point(proposal.backward) - rounding.sqrt())
                    .inf()
                    .max(0.);
                if norm_lower < minimum {
                    if !workflow.witnesses.is_empty() {
                        return Err(bad());
                    }
                    return Ok(Feasibility::Unresolved);
                }
            }
        }
        if workflow.witnesses.len() != 1 {
            return Err(bad());
        }
        let witness = &workflow.witnesses[0];
        if witness.cells != changed || witness.values.len() != times.len() {
            return Err(bad());
        }
        for (&time, value) in times.iter().zip(&witness.values) {
            cancel.poll()?;
            let date = request
                .times
                .partition_point(|&node| node <= time)
                .saturating_sub(1);
            let intended = &nodes[date];
            if value.n != intended.n || value.data.len() != intended.data.len() {
                return Err(bad());
            }
            for (entry, (&actual, &source)) in value.data.iter().zip(&intended.data).enumerate() {
                if entry % 256 == 0 {
                    cancel.poll()?;
                }
                if actual.to_bits() != source.to_bits() {
                    return Err(bad());
                }
            }
        }
        // The independently checked baseline establishes PSD at every intended
        // normalized centre. Constant pieces need no interpolated PSD premise.
        if baseline.eigensystems.len() != nodes.len() {
            return Err(bad());
        }
        Ok(Feasibility::OriginalStepWitness)
    })
}

pub(crate) fn check(
    request: &TraceRequest,
    configuration: &Configuration,
    baseline: &TraceReport,
    jump: &JumpReport,
    times: &[f64],
    workflow: &Workflow,
    cancel: &Cancellation,
) -> Result<Feasibility> {
    memory::scope(|| {
        cancel.poll()?;
        if workflow.scope != SCOPE || workflow.queries.len() != times.len() {
            return Err(bad());
        }
        let count = request.times.len() - 1;
        if baseline.segments.len() != count
            || jump.family.mandatory.len() != count
            || jump.family.optional.len() != count
            || jump.differences.len() != count
        {
            return Err(bad());
        }
        let expected_components = components(baseline, jump, cancel)?;
        if workflow.components != expected_components {
            return Err(bad());
        }
        for (&time, query) in times.iter().zip(&workflow.queries) {
            memory::scope(|| {
                cancel.poll()?;
                if query.time.to_bits() != time.to_bits() {
                    return Err(bad());
                }
                let right = request.times.partition_point(|&node| node < time);
                if right < request.times.len() && request.times[right] == time {
                    let State::Observed { date, trace, modes } = &query.state else {
                        return Err(bad());
                    };
                    if *date != right {
                        return Err(bad());
                    }
                    return check_summary(
                        &baseline.eigensystems[right],
                        baseline.radii[right],
                        trace,
                        modes,
                        cancel,
                    );
                }
                let cell = right
                    .checked_sub(1)
                    .filter(|&value| value < count)
                    .ok_or_else(bad)?;
                if jump.family.mandatory[cell] || jump.family.optional[cell] {
                    let State::JumpPossible {
                        cell: actual_cell,
                        mandatory,
                        sides,
                    } = &query.state
                    else {
                        return Err(bad());
                    };
                    if *actual_cell != cell
                        || *mandatory != jump.family.mandatory[cell]
                        || sides.len() != 2
                    {
                        return Err(bad());
                    }
                    let radii = [
                        upper(
                            point(baseline.radii[cell])
                                + point(request.lipschitz)
                                    * (point(time) - point(request.times[cell])),
                        )?,
                        upper(
                            point(baseline.radii[cell + 1])
                                + point(request.lipschitz)
                                    * (point(request.times[cell + 1]) - point(time)),
                        )?,
                    ];
                    for (offset, side) in sides.iter().enumerate() {
                        let date = cell + offset;
                        if side.date != date || side.radius != radii[offset] {
                            return Err(bad());
                        }
                        check_summary(
                            &baseline.eigensystems[date],
                            radii[offset],
                            &side.trace,
                            &side.modes,
                            cancel,
                        )?;
                    }
                    return Ok(());
                }
                let expected_index = expected_components
                    .iter()
                    .position(|component| component.cells[0] <= cell && cell < component.cells[1])
                    .ok_or_else(bad)?;
                let State::Smooth {
                    component,
                    query: inner,
                } = &query.state
                else {
                    return Err(bad());
                };
                if *component != expected_index {
                    return Err(bad());
                }
                let expected = &expected_components[expected_index];
                crate::continuous_queries::check_component_query(
                    request,
                    baseline,
                    &expected.lineage,
                    expected.cells,
                    time,
                    inner,
                    cancel,
                )
            })?;
        }
        check_step(
            request,
            configuration,
            baseline,
            jump,
            times,
            workflow,
            cancel,
        )
    })
}
