//! Independent same-anchor query bounds and exact-polar family witnesses.
use eigentrail_core::{
    common_frame::{CommonFrameAssessment, CommonFrameModel},
    common_frame_queries::{QueryState, SCOPE, Workflow},
    families::{AffineWitness, Feasibility},
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};

fn bad(reason: &str) -> Error {
    Error::Invalid(format!("common-frame workflow checker: {reason}"))
}

fn checked_row(
    request: &TraceRequest,
    strengths: &[Vec<Range>],
    time: f64,
    cancel: &Cancellation,
) -> Result<Option<Vec<Range>>> {
    memory::scope(|| {
        let p = request.features.len();
        let mut ranges = memory::capacity(p)?;
        match request
            .times
            .binary_search_by(|candidate| candidate.partial_cmp(&time).unwrap())
        {
            Ok(date) => ranges.extend_from_slice(&strengths[date]),
            Err(right) => {
                let left = right - 1;
                let before = point(request.lipschitz) * (point(time) - point(request.times[left]));
                let after = point(request.lipschitz) * (point(request.times[right]) - point(time));
                for (a, b) in strengths[left].iter().zip(&strengths[right]) {
                    cancel.poll()?;
                    let lo = (point(a.lower) - before)
                        .inf()
                        .max((point(b.lower) - after).inf())
                        .max(0.);
                    let hi = upper(point(a.upper) + before)?.min(upper(point(b.upper) + after)?);
                    if lo > hi {
                        return Ok(None);
                    }
                    ranges.push(Range::closed(lo, hi));
                }
            }
        }
        Ok(Some(ranges))
    })
}

fn check_bounds(
    ranges: &[Range],
    summaries: &[Summary],
    trace: &Range,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        if summaries.len() != ranges.len() {
            return Err(bad("missing persistent query labels"));
        }
        let low = ranges.iter().fold(point(0.), |sum, r| sum + point(r.lower));
        let high = ranges.iter().fold(point(0.), |sum, r| sum + point(r.upper));
        let expected_trace = Range::closed(low.inf(), upper(high)?);
        if *trace != expected_trace {
            return Err(bad("query trace bound"));
        }
        for (label, range) in ranges.iter().enumerate() {
            cancel.poll()?;
            let share = if expected_trace.lower > 0. {
                Quantity::Interval(Range::closed(
                    (point(range.lower) / point(expected_trace.upper))
                        .inf()
                        .max(0.),
                    upper(point(range.upper) / point(expected_trace.lower))?.min(1.),
                ))
            } else {
                Quantity::Undefined(memory::text("trace may be zero")?)
            };
            let rank = [
                1 + ranges
                    .iter()
                    .enumerate()
                    .filter(|(other, r)| *other != label && r.lower > range.upper)
                    .count(),
                ranges.len()
                    - ranges
                        .iter()
                        .enumerate()
                        .filter(|(other, r)| *other != label && r.upper < range.lower)
                        .count(),
            ];
            let got = &summaries[label];
            if got.strength != *range || got.share != share || got.rank != rank {
                return Err(bad("query strength, share or complete rank competitors"));
            }
        }
        Ok(())
    })
}

pub fn check(
    request: &TraceRequest,
    model: &CommonFrameModel,
    assessment: &CommonFrameAssessment,
    query_times: &[f64],
    auto_witness: bool,
    report: &Workflow,
    cancel: &Cancellation,
) -> Result<()> {
    let queries = query_times
        .len()
        .checked_mul(std::mem::size_of::<f64>())
        .ok_or_else(|| Error::Resource(String::new()))?;
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        model.heap_bytes()?,
        assessment.heap_bytes()?,
        report.heap_bytes()?,
        queries,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        crate::common_frame::check_assessment(request, model, assessment, cancel)?;
        if query_times.len() > 10000 {
            return Err(bad("query count exceeds 10000"));
        }
        if report.scope != SCOPE
            || report.queries.len() != query_times.len()
            || report.witnesses.len() != usize::from(auto_witness)
        {
            return Err(bad("workflow scope, query cardinality or witness count"));
        }
        for (&time, row) in query_times.iter().zip(&report.queries) {
            memory::scope(|| {
                cancel.poll()?;
                if !time.is_finite()
                    || time < request.times[0]
                    || time > *request.times.last().unwrap()
                    || row.time != time
                {
                    return Err(bad("query domain, time or order"));
                }
                match assessment {
                    CommonFrameAssessment::UnresolvedAnchor { .. } => {
                        if !matches!(row.state, QueryState::UnresolvedAnchor) {
                            return Err(bad("individual labels asserted for an unresolved anchor"));
                        }
                    }
                    CommonFrameAssessment::Resolved { report: anchor } => {
                        match checked_row(request, &anchor.frame.labelled_strengths, time, cancel)?
                        {
                            None if matches!(row.state, QueryState::InconsistentOuterBounds) => {}
                            Some(ranges) => match &row.state {
                                QueryState::Bounds { summaries, trace } => {
                                    check_bounds(&ranges, summaries, trace, cancel)?;
                                }
                                _ => return Err(bad("missing nonempty query bounds")),
                            },
                            _ => {
                                return Err(bad("inconsistent query bounds fabricated or omitted"));
                            }
                        }
                    }
                }

                Ok(())
            })?;
        }
        let Some(witness) = report.witnesses.first() else {
            return cancel.poll();
        };
        let (axes, radii) = match assessment {
            CommonFrameAssessment::Resolved { report: anchor } => {
                (&anchor.frame.axes, &anchor.radii)
            }
            CommonFrameAssessment::UnresolvedAnchor { axes, radii, .. } => (axes, radii),
        };
        let m = request.times.len();
        let p = request.features.len();
        if witness.diagonal_nodes.len() != m
            || witness.nodal_error_bounds.len() != m
            || witness.step_norm_bounds.len() != m - 1
            || witness
                .diagonal_nodes
                .iter()
                .any(|row| row.len() != p || row.iter().any(|x| !x.is_finite() || *x < 0.))
            || witness
                .nodal_error_bounds
                .iter()
                .chain(&witness.step_norm_bounds)
                .any(|x| !x.is_finite() || *x < 0.)
        {
            return Err(bad(
                "complete finite nonnegative polar witness data required",
            ));
        }
        let observations = calibration::normalized(request)?;
        let mut accepted = true;
        for (date, matrix) in observations.iter().enumerate() {
            cancel.poll()?;
            let diagonal = &witness.diagonal_nodes[date];
            let mut residual = point(0.);
            for row in 0..p {
                cancel.poll()?;
                for col in 0..p {
                    let reconstruction =
                        diagonal.iter().enumerate().fold(point(0.), |sum, (i, &d)| {
                            sum + point(axes.vectors.at(row, i))
                                * point(d)
                                * point(axes.vectors.at(col, i))
                        });
                    residual += square(point(matrix.at(row, col)) - reconstruction);
                }
            }
            let largest = diagonal.iter().copied().fold(0f64, f64::max);
            let delta = point(axes.polar);
            let required = upper(residual.sqrt() + point(largest) * delta * (point(2.) + delta))?;
            if witness.nodal_error_bounds[date] < required {
                return Err(bad("understated exact-polar nodal error"));
            }
            accepted &= witness.nodal_error_bounds[date] <= radii[date];
            if date > 0 {
                let mut required = 0f64;
                for (label, &value) in diagonal.iter().enumerate() {
                    required = required.max(upper(
                        (point(value) - point(witness.diagonal_nodes[date - 1][label])).abs(),
                    )?);
                }
                if witness.step_norm_bounds[date - 1] < required {
                    return Err(bad("understated exact-polar increment"));
                }
                let allowance = (point(request.lipschitz)
                    * (point(request.times[date]) - point(request.times[date - 1])))
                .inf();
                accepted &= witness.step_norm_bounds[date - 1] <= allowance;
            }
        }
        let expected = if accepted {
            Feasibility::OriginalAffineWitness
        } else {
            Feasibility::Unresolved
        };
        if witness.feasibility != expected {
            return Err(bad("polar witness status or uncharged widening"));
        }
        cancel.poll()
    })
}

/// Check an explicit exact-matrix witness without projecting its supplied nodes.
pub fn explicit_feasibility(
    request: &TraceRequest,
    radii: &[f64],
    witness: &AffineWitness,
    cancel: &Cancellation,
) -> Result<Feasibility> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        witness.heap_bytes()?,
        radii
            .len()
            .checked_mul(std::mem::size_of::<f64>())
            .ok_or_else(|| Error::Resource(String::new()))?,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        validate(request)?;
        nonnegative(witness.widening, "witness widening")?;
        if witness.nodes.len() != request.times.len() || radii.len() != request.times.len() {
            return Err(bad(
                "complete explicit witness chain and nodal radii required",
            ));
        }
        for &radius in radii {
            nonnegative(radius, "checked nodal radius")?;
        }
        let p = request.features.len();
        for node in &witness.nodes {
            cancel.poll()?;
            numerics::validate_matrix(node)?;
            if node.n != p {
                return Err(bad("explicit witness dimension"));
            }
        }
        if witness
            .nodes
            .iter()
            .any(|node| (0..p).any(|row| (0..p).any(|col| row != col && node.at(row, col) != 0.)))
        {
            return Ok(Feasibility::UnsupportedModel);
        }
        let observations = calibration::normalized(request)?;
        for (date, node) in witness.nodes.iter().enumerate() {
            cancel.poll()?;
            if (0..p).any(|label| node.at(label, label) < 0.) {
                return Ok(Feasibility::Unresolved);
            }
            let allowance = (point(radii[date]) + point(witness.widening)).inf();
            if crate::families::difference(node, &observations[date], cancel)? > allowance {
                return Ok(Feasibility::Unresolved);
            }
            if date > 0 {
                let mut step = 0f64;
                for label in 0..p {
                    step = step.max(upper(
                        (point(node.at(label, label))
                            - point(witness.nodes[date - 1].at(label, label)))
                        .abs(),
                    )?);
                }
                let allowance = (point(request.lipschitz)
                    * (point(request.times[date]) - point(request.times[date - 1])))
                .inf();
                if step > allowance {
                    return Ok(Feasibility::Unresolved);
                }
            }
        }
        Ok(if witness.widening == 0. {
            Feasibility::OriginalAffineWitness
        } else {
            Feasibility::WidenedAffineWitness {
                additional: witness.widening,
            }
        })
    })
}
