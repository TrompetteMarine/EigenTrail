//! Independently reconstruct conditional transverse-crossing decisions and scope.
use eigentrail_core::{
    crossings::{
        CollisionBudget, CrossingAssessment, CrossingOutcome, CrossingProof, CrossingWindow, Gate,
        RESOURCE_SCOPE, SCHEMA, SCOPE, TimingContrast,
    },
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};

fn bad(message: &str) -> Error {
    Error::Invalid(format!("crossing checker: {message}"))
}

fn finite_lower(value: Interval) -> Result<f64> {
    let bound = value.inf();
    if bound.is_finite() {
        Ok(bound)
    } else {
        Err(Error::Exhausted("nonfinite crossing lower bound".into()))
    }
}

fn admission(request: &TraceRequest, window: &CrossingWindow) -> Result<usize> {
    let base = eigentrail_core::validate(request)?;
    let p = request.features.len();
    let m = request.times.len();
    let [start, end] = window.block;
    let [a, b] = window.secant;
    let [left, right] = window.attachments;
    let [lo, hi] = window.bracket;
    nonnegative(window.curvature, "crossing curvature")?;
    if start >= end
        || end > p
        || end - start < 2
        || a >= b
        || b >= m
        || left >= right
        || right >= m
        || a < left
        || b > right
        || window.centre < left
        || window.centre > right
        || !lo.is_finite()
        || !hi.is_finite()
        || lo > hi
        || request.times[left] >= lo
        || request.times[right] <= hi
        || request.times[a] > lo
        || request.times[b] < hi
    {
        return Err(bad(
            "block, containing secant, or strictly straddling attachments",
        ));
    }
    let reserved = p
        .checked_mul(p)
        .and_then(|v| v.checked_mul(1024))
        .and_then(|v| v.checked_add(16384))
        .and_then(|v| v.checked_add(base))
        .ok_or_else(|| Error::Resource(String::new()))?;
    if reserved > request.memory_bytes {
        return Err(Error::Resource(
            "crossing shape exceeds memory limit".into(),
        ));
    }
    Ok(reserved)
}

fn polar_correction(a: &Matrix, cancel: &Cancellation) -> Result<f64> {
    let n = a.n;
    if a.data.len() != n * n || a.data.iter().any(|x| !x.is_finite()) {
        return Err(bad("anchor shape or finite values"));
    }
    let mut defect = point(0.);
    for row in 0..n {
        cancel.poll()?;
        for col in 0..n {
            let mut gram = point(0.);
            for k in 0..n {
                gram += point(a.at(k, row)) * point(a.at(k, col));
            }
            defect += square(gram - point(f64::from(row == col)));
        }
    }
    let tau = defect.sqrt();
    if tau.sup() >= 1. {
        return Err(bad("anchor polar gate"));
    }
    upper(tau / (point(1.) + (point(1.) - tau).sqrt()))
}

/// Reconstruct the intended secant/compression; no producer proposal routine is used.
fn compression(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    matrices: &[Matrix],
    proposed: &Eigensystem,
    cancel: &Cancellation,
) -> Result<f64> {
    memory::scope(|| {
        let p = request.features.len();
        let [start, end] = window.block;
        let q = end - start;
        let [a, b] = window.secant;
        let h = point(request.times[b]) - point(request.times[a]);
        let centre = &baseline.eigensystems[window.centre];
        let full = q == p;
        let basis = |row, column| {
            if full {
                f64::from(row == column)
            } else {
                centre.vectors.at(row, start + column)
            }
        };
        let mut secant = memory::capacity(p * p)?;
        secant.resize(p * p, point(0.));
        for col in 0..p {
            cancel.poll()?;
            for row in 0..p {
                secant[row + col * p] =
                    (point(matrices[b].at(row, col)) - point(matrices[a].at(row, col))) / h;
            }
        }
        let mut product = memory::capacity(p * q)?;
        product.resize(p * q, point(0.));
        for col in 0..q {
            for row in 0..p {
                cancel.poll()?;
                let mut value = point(0.);
                for k in 0..p {
                    value += secant[row + k * p] * point(basis(k, col));
                }
                product[row + col * p] = value;
            }
        }
        let mut intended = Matrix {
            n: q,
            data: resources::zeros(q * q)?,
        };
        let mut rounding = point(0.);
        for col in 0..q {
            cancel.poll()?;
            for row in 0..q {
                let mut exact = point(0.);
                for k in 0..p {
                    exact += point(basis(k, row)) * product[k + col * p];
                }
                let midpoint = exact.inf() / 2. + exact.sup() / 2.;
                if !midpoint.is_finite() {
                    return Err(Error::Exhausted("crossing compression midpoint".into()));
                }
                intended.data[row + col * q] = midpoint;
                rounding += square(exact - point(midpoint));
            }
        }
        let mut repair = point(0.);
        for col in 0..q {
            cancel.poll()?;
            for row in 0..col {
                let old_a = intended.at(row, col);
                let old_b = intended.at(col, row);
                let midpoint = old_a / 2. + old_b / 2.;
                if !midpoint.is_finite() {
                    return Err(Error::Exhausted(
                        "crossing compression symmetrization".into(),
                    ));
                }
                repair = repair
                    + square(point(midpoint) - point(old_a))
                    + square(point(midpoint) - point(old_b));
                intended.data[row + col * q] = midpoint;
                intended.data[col + row * q] = midpoint;
            }
        }
        crate::eigensystem::inspect(&intended, proposed, cancel)?;
        let dc = if full { 0. } else { centre.polar };
        let norm = point(request.lipschitz)
            + (point(baseline.radii[a]) + point(baseline.radii[b])) / h
            + point(window.curvature) * h / point(2.);
        upper(
            rounding.sqrt()
                + repair.sqrt()
                + norm * (point(2.) * point(dc) + square(point(dc)))
                + point(proposed.backward),
        )
    })
}

fn anchors(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    proof: &CrossingProof,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        let p = request.features.len();
        let [start, end] = window.block;
        let q = end - start;
        let full = q == p;
        let centre = &baseline.eigensystems[window.centre];
        let mut represented = Matrix {
            n: p,
            data: if full {
                resources::zeros(p * p)?
            } else {
                memory::copy(&centre.vectors.data)?
            },
        };
        if full {
            for i in 0..p {
                represented.data[i + i * p] = 1.;
            }
        }
        let mut rounding = point(0.);
        for j in 0..q {
            for row in 0..p {
                cancel.poll()?;
                let mut value = point(0.);
                for k in 0..q {
                    let basis = if full {
                        f64::from(row == k)
                    } else {
                        centre.vectors.at(row, start + k)
                    };
                    value += point(basis) * point(proof.compressed.vectors.at(k, q - 1 - j));
                }
                let midpoint = value.inf() / 2. + value.sup() / 2.;
                if !midpoint.is_finite() {
                    return Err(Error::Exhausted("crossing anchor product".into()));
                }
                represented.data[row + (start + j) * p] = midpoint;
                rounding += square(value - point(midpoint));
            }
        }
        if represented != proof.slope_anchors {
            return Err(bad("lifted anchor values or ascending slope labels"));
        }
        let polar = polar_correction(&represented, cancel)?;
        let dc = if full { 0. } else { centre.polar };
        let product_error = point(dc)
            + point(proof.compressed.polar)
            + point(dc) * point(proof.compressed.polar)
            + rounding.sqrt();
        let display = upper(
            (point(2.) + product_error) * product_error + (point(2.) + point(polar)) * point(polar),
        )?;
        if proof.anchor_polar != polar || proof.display_error != display {
            return Err(bad("anchor polar/product representation errors"));
        }
        Ok(())
    })
}

fn scalar_budget(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    proof: &CrossingProof,
) -> Result<std::result::Result<CollisionBudget, Gate>> {
    let p = request.features.len();
    let [start, end] = window.block;
    let q = end - start;
    let [a, b] = window.secant;
    let [left, right] = window.attachments;
    let centre = &baseline.eigensystems[window.centre];
    let bracket = interval(window.bracket[0], window.bracket[1])?;
    let wc = upper((point(request.times[window.centre]) - bracket).abs())?;
    let horizon = upper((interval(request.times[left], request.times[right])? - bracket).abs())?;
    let k = point(request.lipschitz);
    let m = point(window.curvature);
    let radius = upper(point(baseline.radii[window.centre]) + point(centre.backward))?;
    let r0 = point(radius) + k * point(wc);
    upper(r0)?;
    let mut external = f64::MAX;
    if start > 0 {
        external =
            external.min((point(centre.values[start - 1]) - point(centre.values[start])).inf());
    }
    if end < p {
        external = external.min((point(centre.values[end - 1]) - point(centre.values[end])).inf());
    }
    let g = point(external) - point(2.) * r0;
    let dp = if q == p {
        point(0.)
    } else {
        if g.inf() <= 0.
            || r0.sup() >= (g / point(2.)).inf()
            || (k * point(horizon)).sup() >= (g / point(2.)).inf()
        {
            return Ok(Err(Gate::ExternalPlane));
        }
        r0 / (g - r0)
    };
    let h = point(request.times[b]) - point(request.times[a]);
    let rd = (point(baseline.radii[a]) + point(baseline.radii[b])) / h
        + m * h / point(2.)
        + point(proof.compression_error)
        + point(4.) * k * dp;
    upper(rd)?;
    let mut raw_gap = f64::MAX;
    for pair in proof.compressed.values.windows(2) {
        raw_gap = raw_gap.min((point(pair[0]) - point(pair[1])).inf());
    }
    let slope = point(raw_gap) - point(2.) * rd;
    if slope.inf() <= 0. {
        return Ok(Err(Gate::SlopeSeparation));
    }
    let eta = point(2.) * dp + point(2.) * rd / slope;
    let gamma = if q == p {
        point(0.)
    } else {
        k * point(horizon) / (g - k * point(horizon))
    };
    let delta = (k * gamma + m * point(horizon) / point(2.)) / (point(1.) - square(gamma)).sqrt();
    upper(delta)?;
    if (point(2.) * delta).sup() >= slope.inf() {
        return Ok(Err(Gate::Perturbation));
    }
    let beta = gamma + delta / (slope - delta);
    let minimum = slope - point(2.) * m * point(horizon);
    if minimum.inf() <= 0. {
        return Ok(Err(Gate::TimingSlope));
    }
    Ok(Ok(CollisionBudget {
        plane_error: upper(dp)?,
        derivative_error: upper(rd)?,
        slope_gap: finite_lower(slope)?,
        anchor_error: upper(point(upper(eta)?) + point(proof.display_error))?,
        branch_error: upper(point(upper(eta + beta)?) + point(proof.display_error))?,
        min_slope: finite_lower(minimum)?,
    }))
}

type AttachmentGraph = (Vec<f64>, Vec<Vec<usize>>, Vec<usize>);
fn attachment_graph(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    proof: &CrossingProof,
    budget: &CollisionBudget,
    date: usize,
    cancel: &Cancellation,
) -> Result<AttachmentGraph> {
    memory::scope(|| {
        let p = request.features.len();
        let [start, end] = window.block;
        let q = end - start;
        let e = &baseline.eigensystems[date];
        let mut errors = memory::capacity(q)?;
        for i in start..end {
            cancel.poll()?;
            let uncertainty = point(baseline.radii[date]) + point(e.backward);
            let mut gap = f64::MAX;
            if i > 0 {
                gap = gap.min(
                    (point(e.values[i - 1]) - point(e.values[i]) - point(2.) * uncertainty).inf(),
                );
            }
            if i + 1 < p {
                gap = gap.min(
                    (point(e.values[i]) - point(e.values[i + 1]) - point(2.) * uncertainty).inf(),
                );
            }
            let error = if gap > 0. {
                upper(
                    point(2.) * uncertainty / point(gap)
                        + (point(2.) + point(e.polar)) * point(e.polar),
                )?
                .min(1.)
            } else {
                1.
            };
            errors.push(error);
        }
        let correction =
            point(proof.anchor_polar) + point(e.polar) + point(proof.anchor_polar) * point(e.polar);
        let mut graph = memory::capacity(q)?;
        for anchor in start..end {
            cancel.poll()?;
            let mut candidates = memory::capacity(q)?;
            for (local, &error) in errors.iter().enumerate() {
                let competitor = start + local;
                let mut dot = point(0.);
                for row in 0..p {
                    dot += point(proof.slope_anchors.at(row, anchor))
                        * point(e.vectors.at(row, competitor));
                }
                let cosine = upper(point(magnitude(dot)) + correction)?.min(1.);
                let lower_distance = (point(1.) - square(point(cosine))).sqrt().inf();
                if lower_distance <= upper(point(budget.branch_error) + point(error))? {
                    candidates.push(competitor);
                }
            }
            graph.push(candidates);
        }
        let maximum = errors.iter().copied().fold(0f64, f64::max);
        let mut matching = memory::capacity(q)?;
        if upper(point(budget.branch_error) + point(maximum))? < 0.5
            && graph.iter().all(|row| row.len() == 1)
        {
            for row in &graph {
                if matching.contains(&row[0]) {
                    matching.clear();
                    break;
                }
                matching.push(row[0]);
            }
        }
        Ok((errors, graph, matching))
    })
}

fn timing_rows(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    matrices: &[Matrix],
    proof: &CrossingProof,
    budget: &CollisionBudget,
    cancel: &Cancellation,
) -> Result<(Vec<TimingContrast>, Vec<Range>)> {
    memory::scope(|| {
        let p = request.features.len();
        let [start, end] = window.block;
        let q = end - start;
        let centre = &baseline.eigensystems[window.centre];
        let bracket = interval(window.bracket[0], window.bracket[1])?;
        let wc = upper((point(request.times[window.centre]) - bracket).abs())?;
        let [left, right] = window.attachments;
        let horizon =
            upper((interval(request.times[left], request.times[right])? - bracket).abs())?;
        let rc = upper(point(baseline.radii[window.centre]) + point(centre.backward))?;
        let r0 = upper(point(rc) + point(request.lipschitz) * point(wc))?;
        let diameter = upper(
            point(centre.values[0]) - point(centre.values[p - 1])
                + point(2.) * point(centre.backward)
                + point(2.) * point(r0),
        )?;
        let maximum = upper(
            point(proof.compressed.values[0]) - point(proof.compressed.values[q - 1])
                + point(2.) * point(budget.derivative_error)
                + point(2.) * point(window.curvature) * point(horizon),
        )?;
        let denominator = interval(budget.min_slope, maximum)?;
        let mut rows = memory::capacity(2)?;
        let mut lower_bound = window.bracket[0];
        let mut upper_bound = window.bracket[1];
        for date in [left, right] {
            cancel.poll()?;
            let mut f = point(0.);
            for row in 0..p {
                cancel.poll()?;
                for col in 0..p {
                    let high = point(proof.slope_anchors.at(row, end - 1))
                        * point(proof.slope_anchors.at(col, end - 1));
                    let low = point(proof.slope_anchors.at(row, start))
                        * point(proof.slope_anchors.at(col, start));
                    f += (high - low) * point(matrices[date].at(row, col));
                }
            }
            let w = upper((point(request.times[date]) - bracket).abs())?;
            let e = &baseline.eigensystems[date];
            let norm = point(e.values.iter().fold(0f64, |s, value| s.max(value.abs())))
                + point(e.backward);
            let radius = upper(
                point(2.) * point(baseline.radii[date])
                    + point(2.) * point(diameter) * square(point(budget.plane_error))
                    + point(4.) * point(request.lipschitz) * point(w) * point(budget.anchor_error)
                    + point(2.) * norm * point(proof.display_error),
            )?;
            let estimate = Range::closed(finite_lower(f)?, upper(f)?);
            let strength =
                Range::closed(finite_lower(f - point(radius))?, upper(f + point(radius))?);
            let inferred = point(request.times[date])
                - interval(strength.lower, strength.upper)? / denominator;
            let candidate = Range::closed(finite_lower(inferred)?, upper(inferred)?);
            lower_bound = lower_bound.max(candidate.lower);
            upper_bound = upper_bound.min(candidate.upper);
            rows.push(TimingContrast {
                date,
                estimate,
                radius,
                strength,
                candidate,
            });
        }
        let mut final_intervals = memory::capacity(usize::from(lower_bound <= upper_bound))?;
        if lower_bound <= upper_bound {
            final_intervals.push(Range::closed(lower_bound, upper_bound));
        }
        Ok((rows, final_intervals))
    })
}

fn downstream_empty(proof: &CrossingProof) -> bool {
    proof.left_errors.is_empty()
        && proof.right_errors.is_empty()
        && proof.left_candidates.is_empty()
        && proof.right_candidates.is_empty()
        && proof.left.is_empty()
        && proof.right.is_empty()
        && proof.contrasts.is_empty()
        && proof.timing.is_empty()
}

pub fn check(
    request: &TraceRequest,
    window: &CrossingWindow,
    report: &CrossingAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        let reservation = admission(request, window)?;
        if report.schema != SCHEMA
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.window_hash != serialization::digest_with_cancel(window, cancel)?
            || report.scope != SCOPE
            || !report.completed
            || report.managed_reservation != reservation
            || report.resource_qualification != RESOURCE_SCOPE
        {
            return Err(bad(
                "schema, input/window binding, scope, completion or resources",
            ));
        }
        crate::check_with_cancel(request, &report.baseline, cancel)?;
        if !window.exact_collision_assumed {
            if !report.proof.is_empty()
                || report.outcome
                    != (CrossingOutcome::Unresolved {
                        gate: Gate::StructuralPremise,
                    })
            {
                return Err(bad("exact versus avoided structural premise"));
            }
            return Ok(());
        }
        if report.proof.len() != 1 {
            return Err(bad("missing or extra crossing proof"));
        }
        let proof = &report.proof[0];
        let matrices = calibration::normalized(request)?;
        let compression_error = compression(
            request,
            window,
            &report.baseline,
            &matrices,
            &proof.compressed,
            cancel,
        )?;
        if proof.compression_error != compression_error {
            return Err(bad("compressed-derivative representation error"));
        }
        anchors(request, window, &report.baseline, proof, cancel)?;
        let budget = match scalar_budget(request, window, &report.baseline, proof)? {
            Ok(budget) => budget,
            Err(gate) => {
                if !proof.budget.is_empty()
                    || !downstream_empty(proof)
                    || report.outcome != (CrossingOutcome::Unresolved { gate })
                {
                    return Err(bad("scalar gate, budget or downstream claims"));
                }
                return Ok(());
            }
        };
        if proof.budget.as_slice() != [budget] {
            return Err(bad("canonical transverse budget"));
        }
        let budget = &proof.budget[0];
        let (left_errors, left_graph, left) = attachment_graph(
            request,
            window,
            &report.baseline,
            proof,
            budget,
            window.attachments[0],
            cancel,
        )?;
        let (right_errors, right_graph, right) = attachment_graph(
            request,
            window,
            &report.baseline,
            proof,
            budget,
            window.attachments[1],
            cancel,
        )?;
        if proof.left_errors != left_errors
            || proof.right_errors != right_errors
            || proof.left_candidates != left_graph
            || proof.right_candidates != right_graph
        {
            return Err(bad("incomplete attachment errors or competitor graphs"));
        }
        if left.is_empty() || right.is_empty() {
            if !proof.left.is_empty()
                || !proof.right.is_empty()
                || !proof.contrasts.is_empty()
                || !proof.timing.is_empty()
                || report.outcome
                    != (CrossingOutcome::Unresolved {
                        gate: Gate::Attachments,
                    })
            {
                return Err(bad("strict attachment gate or invented matching"));
            }
            return Ok(());
        }
        if proof.left != left || proof.right != right {
            return Err(bad("wrong or incomplete endpoint attachments"));
        }
        let (contrasts, timing) = timing_rows(
            request,
            window,
            &report.baseline,
            &matrices,
            proof,
            budget,
            cancel,
        )?;
        let outcome = if timing.is_empty() {
            CrossingOutcome::Unresolved {
                gate: Gate::TimingIntersection,
            }
        } else {
            CrossingOutcome::Resolved
        };
        if proof.contrasts != contrasts || proof.timing != timing || report.outcome != outcome {
            return Err(bad("timing contrast, intersection or outcome"));
        }
        cancel.poll()?;
        Ok(())
    })
}
