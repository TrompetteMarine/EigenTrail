//! Conditional finite-multiplicity crossings; an exact collision remains a premise.
use crate::{
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "eigentrail.crossings.v1";
pub const SCOPE: &str = "conditional exact finite-multiplicity collision, C2 bounds, and supplied collision bracket; not identification of exact versus avoided crossing; checked reconnection is not whole-history feasibility";
pub const RESOURCE_SCOPE: &str = "pending audit of native allocations and interruption latency";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionBudget {
    pub plane_error: f64,
    pub derivative_error: f64,
    pub slope_gap: f64,
    pub anchor_error: f64,
    pub branch_error: f64,
    pub min_slope: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossingWindow {
    pub block: [usize; 2],
    pub centre: usize,
    pub secant: [usize; 2],
    pub attachments: [usize; 2],
    pub bracket: [f64; 2],
    pub curvature: f64,
    pub exact_collision_assumed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Gate {
    StructuralPremise,
    ExternalPlane,
    SlopeSeparation,
    Perturbation,
    TimingSlope,
    Attachments,
    TimingIntersection,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CrossingOutcome {
    Resolved,
    Unresolved { gate: Gate },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingContrast {
    pub date: usize,
    pub estimate: Range,
    pub radius: f64,
    pub strength: Range,
    pub candidate: Range,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossingProof {
    pub compressed: Eigensystem,
    pub slope_anchors: Matrix,
    pub anchor_polar: f64,
    pub compression_error: f64,
    pub display_error: f64,
    pub budget: Vec<CollisionBudget>,
    pub left_errors: Vec<f64>,
    pub right_errors: Vec<f64>,
    pub left_candidates: Vec<Vec<usize>>,
    pub right_candidates: Vec<Vec<usize>>,
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    pub contrasts: Vec<TimingContrast>,
    pub timing: Vec<Range>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossingAssessment {
    pub schema: String,
    pub request_hash: String,
    pub window_hash: String,
    pub baseline: TraceReport,
    pub proof: Vec<CrossingProof>,
    pub outcome: CrossingOutcome,
    pub scope: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}
pub struct CrossingReport {
    pub budget: CollisionBudget,
    pub slope_anchors: Matrix,
    pub slopes: Vec<f64>,
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    pub timing: Range,
    pub scope: String,
}
pub struct CollisionInput<'a> {
    pub full_space: bool,
    pub collision_assumed: bool,
    pub k: f64,
    pub curvature: f64,
    pub centre_radius: f64,
    pub centre_width: f64,
    pub external_gap: f64,
    pub secant_width: f64,
    pub endpoint_radii: [f64; 2],
    pub derivative_gaps: &'a [f64],
    pub window_radius: f64,
}

pub fn transverse(x: &CollisionInput<'_>) -> Result<CollisionBudget> {
    if !x.external_gap.is_finite()
        || !x.secant_width.is_finite()
        || x.secant_width <= 0.
        || x.derivative_gaps.is_empty()
        || x.derivative_gaps.iter().any(|g| !g.is_finite())
    {
        return Err(Error::Invalid(
            "finite collision geometry and positive secant required".into(),
        ));
    }
    for (v, name) in [
        (x.k, "K"),
        (x.curvature, "M"),
        (x.centre_radius, "centre radius"),
        (x.centre_width, "centre width"),
        (x.window_radius, "window radius"),
        (x.endpoint_radii[0], "left error"),
        (x.endpoint_radii[1], "right error"),
    ] {
        nonnegative(v, name)?;
    }
    if !x.collision_assumed {
        return Err(Error::Exhausted("exact collision premise absent".into()));
    }
    scalar_budget(x, point(x.secant_width), 0.)?
        .map_err(|gate| Error::Exhausted(format!("crossing unresolved: {gate:?}")))
}
fn scalar_budget(
    x: &CollisionInput<'_>,
    h: Interval,
    numerical: f64,
) -> Result<std::result::Result<CollisionBudget, Gate>> {
    let k = point(x.k);
    let m = point(x.curvature);
    let r = point(x.centre_radius) + k * point(x.centre_width);
    let g = point(x.external_gap) - point(2.) * r;
    upper(r)?;
    let dp = if x.full_space {
        point(0.)
    } else {
        if g.inf() <= 0.
            || r.sup() >= (g / point(2.)).inf()
            || (k * point(x.window_radius)).sup() >= (g / point(2.)).inf()
        {
            return Ok(Err(Gate::ExternalPlane));
        }
        r / (g - r)
    };
    let ed = (point(x.endpoint_radii[0]) + point(x.endpoint_radii[1])) / h + m * h / point(2.);
    let rd = ed + point(numerical) + point(4.) * k * dp;
    upper(rd)?;
    let raw = x.derivative_gaps.iter().copied().fold(f64::MAX, f64::min);
    let slope = point(raw) - point(2.) * rd;
    if slope.inf() <= 0. {
        return Ok(Err(Gate::SlopeSeparation));
    }
    let anchor = point(2.) * dp + point(2.) * rd / slope;
    let gamma = if x.full_space {
        point(0.)
    } else {
        k * point(x.window_radius) / (g - k * point(x.window_radius))
    };
    let delta =
        (k * gamma + m * point(x.window_radius) / point(2.)) / (point(1.) - square(gamma)).sqrt();
    upper(delta)?;
    if (point(2.) * delta).sup() >= slope.inf() {
        return Ok(Err(Gate::Perturbation));
    }
    let beta = gamma + delta / (slope - delta);
    let min_slope = slope - point(2.) * m * point(x.window_radius);
    if min_slope.inf() <= 0. {
        return Ok(Err(Gate::TimingSlope));
    }
    Ok(Ok(CollisionBudget {
        plane_error: upper(dp)?,
        derivative_error: upper(rd)?,
        slope_gap: slope.inf(),
        anchor_error: upper(anchor)?,
        branch_error: upper(anchor + beta)?,
        min_slope: min_slope.inf(),
    }))
}

pub fn validate_window(request: &TraceRequest, window: &CrossingWindow) -> Result<usize> {
    let base = crate::validate(request)?;
    nonnegative(window.curvature, "curvature")?;
    let p = request.features.len();
    let n = request.times.len();
    let [start, end] = window.block;
    let [a, b] = window.secant;
    let [left, right] = window.attachments;
    let [lo, hi] = window.bracket;
    if end.saturating_sub(start) < 2
        || end > p
        || a >= b
        || b >= n
        || left >= right
        || right >= n
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
        return Err(Error::Invalid("collision block, bracket-containing secant and strictly straddling attachments required".into()));
    }
    let reservation = p
        .checked_mul(p)
        .and_then(|v| v.checked_mul(1024))
        .and_then(|v| v.checked_add(16384))
        .and_then(|v| v.checked_add(base))
        .ok_or_else(|| Error::Resource("crossing reservation overflow".into()))?;
    if reservation > request.memory_bytes {
        return Err(Error::Resource(
            "crossing reservation exceeds managed budget".into(),
        ));
    }
    Ok(reservation)
}

pub fn analyze(
    request: &TraceRequest,
    window: &CrossingWindow,
    cancel: &Cancellation,
) -> Result<CrossingAssessment> {
    cancel.poll()?;
    let reservation = validate_window(request, window)?;
    memory::ensure_operation(request.memory_bytes, request.heap_bytes()?, || {
        let baseline = crate::track_with_cancel(request, cancel)?;
        let mut proof = Vec::new();
        let outcome = if !window.exact_collision_assumed {
            CrossingOutcome::Unresolved {
                gate: Gate::StructuralPremise,
            }
        } else {
            let (evidence, outcome) =
                memory::scope(|| build_proof(request, window, &baseline, cancel))?;
            memory::push(&mut proof, evidence)?;
            outcome
        };
        cancel.poll()?;
        Ok(CrossingAssessment {
            schema: memory::text(SCHEMA)?,
            request_hash: serialization::digest_with_cancel(request, cancel)?,
            window_hash: serialization::digest_with_cancel(window, cancel)?,
            baseline,
            proof,
            outcome,
            scope: memory::text(SCOPE)?,
            completed: true,
            managed_reservation: reservation,
            resource_qualification: memory::text(RESOURCE_SCOPE)?,
        })
    })
}

fn build_proof(
    request: &TraceRequest,
    window: &CrossingWindow,
    baseline: &TraceReport,
    cancel: &Cancellation,
) -> Result<(CrossingProof, CrossingOutcome)> {
    let p = request.features.len();
    let [start, end] = window.block;
    let q = end - start;
    let [a, b] = window.secant;
    let [left, right] = window.attachments;
    let [lo, hi] = window.bracket;
    let full = q == p;
    let matrices = calibration::normalized(request)?;
    let eigen = &baseline.eigensystems;
    let radii = &baseline.radii;
    let centre = &eigen[window.centre];
    let h = point(request.times[b]) - point(request.times[a]);
    let wc = upper((point(request.times[window.centre]) - interval(lo, hi)?).abs())?;
    let horizon =
        upper((interval(request.times[left], request.times[right])? - interval(lo, hi)?).abs())?;
    let rc = upper(point(radii[window.centre]) + point(centre.backward))?;
    let mut external = if full { 0. } else { f64::MAX };
    if start > 0 {
        external =
            external.min((point(centre.values[start - 1]) - point(centre.values[start])).inf());
    }
    if end < p {
        external = external.min((point(centre.values[end - 1]) - point(centre.values[end])).inf());
    }
    let basis = |row: usize, col: usize| {
        if full {
            if row == col { 1. } else { 0. }
        } else {
            centre.vectors.at(row, start + col)
        }
    };
    let centre_polar = if full { 0. } else { centre.polar };
    let mut derivative = memory::capacity(p * p)?;
    for i in 0..p * p {
        if i % p == 0 {
            cancel.poll()?;
        }
        derivative.push((point(matrices[b].data[i]) - point(matrices[a].data[i])) / h);
    }
    let mut product = memory::capacity(p * q)?;
    for j in 0..q {
        cancel.poll()?;
        for row in 0..p {
            let mut sum = point(0.);
            for col in 0..p {
                sum += derivative[row + col * p] * point(basis(col, j));
            }
            product.push(sum);
        }
    }
    let mut compressed = Matrix {
        n: q,
        data: resources::zeros(q * q)?,
    };
    let mut rounding = point(0.);
    for j in 0..q {
        cancel.poll()?;
        for i in 0..q {
            let mut dot = point(0.);
            for row in 0..p {
                dot += point(basis(row, i)) * product[row + j * p];
            }
            let midpoint = dot.inf() / 2. + dot.sup() / 2.;
            if !midpoint.is_finite() {
                return Err(Error::Exhausted("compressed secant overflow".into()));
            }
            compressed.data[i + j * q] = midpoint;
            rounding += square(dot - point(midpoint));
        }
    }
    let mut repair = point(0.);
    for j in 0..q {
        for i in 0..j {
            let x = compressed.at(i, j);
            let y = compressed.at(j, i);
            let average = x / 2. + y / 2.;
            repair = repair + square(point(average) - point(x)) + square(point(average) - point(y));
            compressed.data[i + j * q] = average;
            compressed.data[j + i * q] = average;
        }
    }
    let slopes = numerics::eigensystem(&compressed, cancel)?;
    let norm = point(request.lipschitz)
        + (point(radii[a]) + point(radii[b])) / h
        + point(window.curvature) * h / point(2.);
    let compression_error = upper(
        rounding.sqrt()
            + repair.sqrt()
            + norm * (point(2.) * point(centre_polar) + square(point(centre_polar)))
            + point(slopes.backward),
    )?;
    let mut anchors = Matrix {
        n: p,
        data: if full {
            resources::zeros(p * p)?
        } else {
            memory::copy(&centre.vectors.data)?
        },
    };
    let mut anchor_rounding = point(0.);
    for j in 0..q {
        cancel.poll()?;
        for row in 0..p {
            let mut sum = point(0.);
            for col in 0..q {
                sum += point(basis(row, col)) * point(slopes.vectors.at(col, q - 1 - j));
            }
            let value = sum.inf() / 2. + sum.sup() / 2.;
            if !value.is_finite() {
                return Err(Error::Exhausted("crossing anchor overflow".into()));
            }
            anchors.data[row + (start + j) * p] = value;
            anchor_rounding += square(sum - point(value));
        }
    }
    let anchor_polar = numerics::frame_polar(&anchors, cancel)?;
    let product_error = point(centre_polar)
        + point(slopes.polar)
        + point(centre_polar) * point(slopes.polar)
        + anchor_rounding.sqrt();
    let display_error = upper(
        (point(2.) + product_error) * product_error
            + (point(2.) + point(anchor_polar)) * point(anchor_polar),
    )?;
    let mut gaps = memory::capacity(q - 1)?;
    for adjacent in slopes.values.windows(2) {
        gaps.push((point(adjacent[0]) - point(adjacent[1])).inf());
    }
    let input = CollisionInput {
        full_space: full,
        collision_assumed: true,
        k: request.lipschitz,
        curvature: window.curvature,
        centre_radius: rc,
        centre_width: wc,
        external_gap: external,
        secant_width: h.inf(),
        endpoint_radii: [radii[a], radii[b]],
        derivative_gaps: &gaps,
        window_radius: horizon,
    };
    let mut proof = CrossingProof {
        compressed: slopes,
        slope_anchors: anchors,
        anchor_polar,
        compression_error,
        display_error,
        budget: Vec::new(),
        left_errors: Vec::new(),
        right_errors: Vec::new(),
        left_candidates: Vec::new(),
        right_candidates: Vec::new(),
        left: Vec::new(),
        right: Vec::new(),
        contrasts: Vec::new(),
        timing: Vec::new(),
    };
    let unresolved = |gate| CrossingOutcome::Unresolved { gate };
    let mut budget = match scalar_budget(&input, h, compression_error)? {
        Ok(budget) => budget,
        Err(gate) => return Ok((proof, unresolved(gate))),
    };
    budget.anchor_error = upper(point(budget.anchor_error) + point(display_error))?;
    budget.branch_error = upper(point(budget.branch_error) + point(display_error))?;
    let (left_errors, left_graph, left_map) =
        attachment_graph(&eigen[left], radii[left], window, &proof, &budget, cancel)?;
    let (right_errors, right_graph, right_map) =
        attachment_graph(&eigen[right], radii[right], window, &proof, &budget, cancel)?;
    proof.left_errors = left_errors;
    proof.right_errors = right_errors;
    proof.left_candidates = left_graph;
    proof.right_candidates = right_graph;
    let attached = left_map.len() == q && right_map.len() == q;
    memory::push(&mut proof.budget, budget)?;
    if !attached {
        return Ok((proof, unresolved(Gate::Attachments)));
    }
    proof.left = left_map;
    proof.right = right_map;
    let budget = &proof.budget[0];
    let r0 = upper(point(rc) + point(request.lipschitz) * point(wc))?;
    let diameter = upper(
        point(centre.values[0]) - point(centre.values[p - 1])
            + point(2.) * point(centre.backward)
            + point(2.) * point(r0),
    )?;
    let max_slope = upper(
        point(proof.compressed.values[0]) - point(proof.compressed.values[q - 1])
            + point(2.) * point(budget.derivative_error)
            + point(2.) * point(window.curvature) * point(horizon),
    )?;
    let mut lower = lo;
    let mut upper_bound = hi;
    for date in [left, right] {
        cancel.poll()?;
        let mut contrast = point(0.);
        for row in 0..p {
            cancel.poll()?;
            for col in 0..p {
                let high = point(proof.slope_anchors.at(row, end - 1))
                    * point(proof.slope_anchors.at(col, end - 1));
                let low = point(proof.slope_anchors.at(row, start))
                    * point(proof.slope_anchors.at(col, start));
                contrast += (high - low) * point(matrices[date].at(row, col));
            }
        }
        let w = upper((point(request.times[date]) - interval(lo, hi)?).abs())?;
        let covariance_norm = point(eigen[date].values.iter().fold(0f64, |s, v| s.max(v.abs())))
            + point(eigen[date].backward);
        let radius = upper(
            point(2.) * point(radii[date])
                + point(2.) * point(diameter) * square(point(budget.plane_error))
                + point(4.) * point(request.lipschitz) * point(w) * point(budget.anchor_error)
                + point(2.) * covariance_norm * point(display_error),
        )?;
        let strength = interval(
            (contrast - point(radius)).inf(),
            upper(contrast + point(radius))?,
        )?;
        let candidate =
            point(request.times[date]) - strength / interval(budget.min_slope, max_slope)?;
        let range = Range::closed(candidate.inf(), upper(candidate)?);
        if !range.lower.is_finite() {
            return Err(Error::Exhausted("collision timing overflow".into()));
        }
        lower = lower.max(range.lower);
        upper_bound = upper_bound.min(range.upper);
        memory::push(
            &mut proof.contrasts,
            TimingContrast {
                date,
                estimate: Range::closed(contrast.inf(), upper(contrast)?),
                radius,
                strength: Range::closed(strength.inf(), strength.sup()),
                candidate: range,
            },
        )?;
    }
    if lower > upper_bound {
        return Ok((proof, unresolved(Gate::TimingIntersection)));
    }
    memory::push(&mut proof.timing, Range::closed(lower, upper_bound))?;
    Ok((proof, CrossingOutcome::Resolved))
}

type AttachmentGraph = (Vec<f64>, Vec<Vec<usize>>, Vec<usize>);
fn attachment_graph(
    eigen: &Eigensystem,
    radius: f64,
    window: &CrossingWindow,
    proof: &CrossingProof,
    budget: &CollisionBudget,
    cancel: &Cancellation,
) -> Result<AttachmentGraph> {
    let [start, end] = window.block;
    let q = end - start;
    let p = eigen.values.len();
    let mut errors = memory::capacity(q)?;
    for rank in start..end {
        let mut gap = f64::MAX;
        let noise = point(radius) + point(eigen.backward);
        if rank > 0 {
            gap = gap.min(
                (point(eigen.values[rank - 1]) - point(eigen.values[rank]) - point(2.) * noise)
                    .inf(),
            );
        }
        if rank + 1 < p {
            gap = gap.min(
                (point(eigen.values[rank]) - point(eigen.values[rank + 1]) - point(2.) * noise)
                    .inf(),
            );
        }
        errors.push(if gap > 0. {
            upper(
                point(2.) * noise / point(gap)
                    + (point(2.) + point(eigen.polar)) * point(eigen.polar),
            )?
            .min(1.)
        } else {
            1.
        });
    }
    let correction = point(proof.anchor_polar)
        + point(eigen.polar)
        + point(proof.anchor_polar) * point(eigen.polar);
    let mut graph = memory::capacity(q)?;
    let mut mapping = memory::capacity(q)?;
    let mut unique =
        upper(point(budget.branch_error) + point(errors.iter().copied().fold(0f64, f64::max)))?
            < 0.5;
    for mode in start..end {
        cancel.poll()?;
        let mut row = memory::capacity(q)?;
        for (offset, &error) in errors.iter().enumerate() {
            let rank = start + offset;
            let mut dot = point(0.);
            for k in 0..p {
                dot += point(proof.slope_anchors.at(k, mode)) * point(eigen.vectors.at(k, rank));
            }
            let cosine = upper(point(magnitude(dot)) + correction)?.min(1.);
            let lower = (point(1.) - square(point(cosine))).sqrt().inf();
            if lower <= upper(point(budget.branch_error) + point(error))? {
                row.push(rank);
            }
        }
        if row.len() != 1 || mapping.contains(&row[0]) {
            unique = false;
        } else {
            mapping.push(row[0]);
        }
        graph.push(row);
    }
    if !unique {
        mapping.clear();
    }
    Ok((errors, graph, mapping))
}

/// Compatibility helper for callers that require a fully resolved crossing.
pub fn reconnect(
    request: &TraceRequest,
    window: &CrossingWindow,
    cancel: &Cancellation,
) -> Result<CrossingReport> {
    let mut assessment = analyze(request, window, cancel)?;
    if let CrossingOutcome::Unresolved { gate } = assessment.outcome {
        return Err(Error::Exhausted(format!("crossing unresolved: {gate:?}")));
    }
    let mut proof = assessment.proof.pop().unwrap();
    proof.compressed.values.reverse();
    Ok(CrossingReport {
        budget: proof.budget.pop().unwrap(),
        slope_anchors: proof.slope_anchors,
        slopes: proof.compressed.values,
        left: proof.left,
        right: proof.right,
        timing: proof.timing.pop().unwrap(),
        scope: assessment.scope,
    })
}

pub fn attachment(branch_error: f64, endpoint_error: f64, straddles: bool) -> bool {
    if [branch_error, endpoint_error]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return false;
    }
    straddles && (point(branch_error) + point(endpoint_error)).sup() < 0.5
}
/// The timing derivative uses the selected pair's gap, not the minimum adjacent gap.
pub fn timing(
    bracket: [f64; 2],
    date: f64,
    strength: crate::Range,
    min_slope: f64,
    max_slope: f64,
) -> Result<crate::Range> {
    if min_slope <= 0. || max_slope < min_slope || strength.lower > strength.upper {
        return Err(Error::Invalid("timing slopes/strength".into()));
    }
    let quotient = interval(strength.lower, strength.upper)? / interval(min_slope, max_slope)?;
    let inferred = point(date) - quotient;
    let lo = bracket[0].max(inferred.inf());
    let hi = bracket[1].min(inferred.sup());
    if lo > hi {
        return Err(Error::Exhausted(
            "empty collision timing intersection; input/model inconsistency".into(),
        ));
    }
    Ok(crate::Range::closed(lo, hi))
}
