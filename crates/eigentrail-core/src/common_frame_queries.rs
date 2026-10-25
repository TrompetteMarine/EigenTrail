//! Same-label query enclosures and constructive common-frame feasibility witnesses.
use crate::{
    common_frame::CommonFrameAssessment, families::Feasibility, resources::Cancellation, scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

pub const SCOPE: &str = "same-anchor query summaries are marginal outer enclosures on one exact common-frame history family; polar affine witnesses certify unlabelled family existence only; widened witnesses do not enlarge the original reported family";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub queries: Vec<QueryRow>,
    pub witnesses: Vec<PolarAffineWitness>,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRow {
    pub time: f64,
    pub state: QueryState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum QueryState {
    Bounds {
        summaries: Vec<Summary>,
        trace: Range,
    },
    UnresolvedAnchor,
    InconsistentOuterBounds,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolarAffineWitness {
    pub diagonal_nodes: Vec<Vec<f64>>,
    pub nodal_error_bounds: Vec<f64>,
    pub step_norm_bounds: Vec<f64>,
    pub feasibility: Feasibility,
}

fn capacity<T>(n: usize) -> Result<Vec<T>> {
    resources::memory::capacity(n)
}

fn axes_and_radii(assessment: &CommonFrameAssessment) -> (&Eigensystem, &[f64]) {
    match assessment {
        CommonFrameAssessment::Resolved { report } => (&report.frame.axes, &report.radii),
        CommonFrameAssessment::UnresolvedAnchor { axes, radii, .. } => (axes, radii),
    }
}

fn validate_assessment(request: &TraceRequest, assessment: &CommonFrameAssessment) -> Result<()> {
    let p = request.features.len();
    let (axes, radii) = axes_and_radii(assessment);
    let shape = |e: &Eigensystem| {
        e.values.len() == p
            && e.values.iter().all(|x| x.is_finite())
            && e.vectors.n == p
            && e.vectors.data.len() == p * p
            && e.vectors.data.iter().all(|x| x.is_finite())
            && e.polar.is_finite()
            && e.polar >= 0.
            && e.backward.is_finite()
            && e.backward >= 0.
    };
    let eigen = match assessment {
        CommonFrameAssessment::Resolved { report } => &report.eigensystems,
        CommonFrameAssessment::UnresolvedAnchor { eigensystems, .. } => eigensystems,
    };
    if !shape(axes)
        || eigen.len() != request.times.len()
        || eigen.iter().any(|e| !shape(e))
        || radii.len() != request.times.len()
    {
        return Err(Error::Invalid("common-frame workflow dimensions".into()));
    }
    nonnegative(axes.polar, "anchor polar error")?;
    for &radius in radii {
        nonnegative(radius, "common-frame radius")?;
    }
    if let CommonFrameAssessment::Resolved { report } = assessment
        && (!report.frame.axis_error.is_finite()
            || !(0. ..=1.).contains(&report.frame.axis_error)
            || report.frame.possible_ranks.len() != request.times.len()
            || report.frame.possible_ranks.iter().any(|row| {
                row.len() != p || row.iter().any(|r| r[0] == 0 || r[0] > r[1] || r[1] > p)
            })
            || report.frame.labelled_strengths.len() != request.times.len()
            || report.frame.labelled_strengths.iter().any(|row| {
                row.len() != p
                    || row.iter().any(|s| {
                        !s.lower.is_finite()
                            || !s.upper.is_finite()
                            || s.lower < 0.
                            || s.lower > s.upper
                            || !s.lower_closed
                            || !s.upper_closed
                    })
            }))
    {
        return Err(Error::Invalid("common-frame labelled strengths".into()));
    }
    Ok(())
}

fn query_state(
    request: &TraceRequest,
    assessment: &CommonFrameAssessment,
    time: f64,
    cancel: &Cancellation,
) -> Result<QueryState> {
    crate::resources::memory::scope(|| {
        let CommonFrameAssessment::Resolved { report } = assessment else {
            return Ok(QueryState::UnresolvedAnchor);
        };
        let p = request.features.len();
        let j = request.times.partition_point(|&t| t < time);
        let mut strengths = capacity(p)?;
        if request.times[j] == time {
            for strength in &report.frame.labelled_strengths[j] {
                resources::memory::push(&mut strengths, strength.clone())?;
            }
        } else {
            let left = point(request.lipschitz) * (point(time) - point(request.times[j - 1]));
            let right = point(request.lipschitz) * (point(request.times[j]) - point(time));
            upper(left)?;
            upper(right)?;
            for mode in 0..p {
                cancel.poll()?;
                let a = &report.frame.labelled_strengths[j - 1][mode];
                let b = &report.frame.labelled_strengths[j][mode];
                let lower = (point(a.lower) - left)
                    .inf()
                    .max((point(b.lower) - right).inf())
                    .max(0.);
                let upper = upper(point(a.upper) + left)?.min(upper(point(b.upper) + right)?);
                if lower > upper {
                    return Ok(QueryState::InconsistentOuterBounds);
                }
                strengths.push(Range::closed(lower, upper));
            }
        }
        let mut trlo = point(0.);
        let mut trhi = point(0.);
        for strength in &strengths {
            trlo += point(strength.lower);
            trhi += point(strength.upper);
        }
        upper(trlo)?;
        let trace = Range::closed(trlo.inf().max(0.), upper(trhi)?);
        let mut summaries = capacity(p)?;
        for strength in &strengths {
            cancel.poll()?;
            let share = if trace.lower <= 0. {
                Quantity::Undefined(resources::memory::text("trace may be zero")?)
            } else {
                let lo = point(strength.lower) / point(trace.upper);
                let hi = point(strength.upper) / point(trace.lower);
                upper(lo)?;
                upper(hi)?;
                Quantity::Interval(Range::closed(lo.inf().max(0.), hi.sup().min(1.)))
            };
            summaries.push(Summary {
                strength: strength.clone(),
                share,
                rank: [
                    1 + strengths
                        .iter()
                        .filter(|s| s.lower > strength.upper)
                        .count(),
                    p - strengths
                        .iter()
                        .filter(|s| s.upper < strength.lower)
                        .count(),
                ],
            });
        }
        Ok(QueryState::Bounds { summaries, trace })
    })
}

fn automatic_witness(
    request: &TraceRequest,
    assessment: &CommonFrameAssessment,
    cancel: &Cancellation,
) -> Result<PolarAffineWitness> {
    crate::resources::memory::scope(|| {
        let (axes, radii) = axes_and_radii(assessment);
        let p = request.features.len();
        let matrices = calibration::normalized(request)?;
        let mut diagonal_nodes = capacity(matrices.len())?;
        let mut nodal_error_bounds = capacity(matrices.len())?;
        let mut step_norm_bounds = capacity(matrices.len() - 1)?;
        let mut verified = true;
        for (j, a) in matrices.iter().enumerate() {
            cancel.poll()?;
            let mut diagonal = capacity(p)?;
            for mode in 0..p {
                cancel.poll()?;
                let mut rayleigh = point(0.);
                for x in 0..p {
                    for y in 0..p {
                        rayleigh += point(axes.vectors.at(x, mode))
                            * point(a.at(x, y))
                            * point(axes.vectors.at(y, mode));
                    }
                }
                upper(rayleigh)?;
                if !rayleigh.inf().is_finite() {
                    return Err(Error::Exhausted(
                        "common-frame witness Rayleigh overflow".into(),
                    ));
                }
                diagonal.push((rayleigh.inf() / 2. + rayleigh.sup() / 2.).max(0.));
            }
            let mut residual = point(0.);
            for x in 0..p {
                cancel.poll()?;
                for y in 0..p {
                    let mut reconstructed = point(0.);
                    for (mode, &value) in diagonal.iter().enumerate() {
                        reconstructed += point(axes.vectors.at(x, mode))
                            * point(value)
                            * point(axes.vectors.at(y, mode));
                    }
                    residual += square(point(a.at(x, y)) - reconstructed);
                }
            }
            let maximum = diagonal.iter().copied().fold(0., f64::max);
            let correction = point(maximum) * point(axes.polar) * (point(2.) + point(axes.polar));
            let error = upper(residual.sqrt() + correction)?;
            verified &= error <= radii[j];
            nodal_error_bounds.push(error);
            if j > 0 {
                let previous: &Vec<f64> = &diagonal_nodes[j - 1];
                let mut increment = 0f64;
                for (&a, &b) in diagonal.iter().zip(previous) {
                    increment = increment.max(upper((point(a) - point(b)).abs())?);
                }
                let allowance = point(request.lipschitz)
                    * (point(request.times[j]) - point(request.times[j - 1]));
                verified &= increment <= allowance.inf();
                step_norm_bounds.push(increment);
            }
            diagonal_nodes.push(diagonal);
        }
        Ok(PolarAffineWitness {
            diagonal_nodes,
            nodal_error_bounds,
            step_norm_bounds,
            feasibility: if verified {
                Feasibility::OriginalAffineWitness
            } else {
                Feasibility::Unresolved
            },
        })
    })
}

pub(crate) fn analyze(
    request: &TraceRequest,
    assessment: &CommonFrameAssessment,
    query_times: &[f64],
    auto_witness: bool,
    cancel: &Cancellation,
) -> Result<Workflow> {
    crate::resources::memory::scope(|| {
        cancel.poll()?;
        validate(request)?;
        validate_assessment(request, assessment)?;
        if query_times.len() > 10000
            || query_times.iter().any(|&t| {
                !t.is_finite() || t < request.times[0] || t > *request.times.last().unwrap()
            })
        {
            return Err(Error::Invalid("common-frame query domain or count".into()));
        }
        let mut queries = capacity(query_times.len())?;
        for &time in query_times {
            cancel.poll()?;
            queries.push(QueryRow {
                time,
                state: query_state(request, assessment, time, cancel)?,
            });
        }
        let mut witnesses = capacity(usize::from(auto_witness))?;
        if auto_witness {
            witnesses.push(automatic_witness(request, assessment, cancel)?);
        }
        Ok(Workflow {
            queries,
            witnesses,
            scope: resources::memory::text(SCOPE)?,
        })
    })
}

pub(crate) fn explicit_feasibility(
    request: &TraceRequest,
    radii: &[f64],
    witness: &families::AffineWitness,
    cancel: &Cancellation,
) -> Result<Feasibility> {
    crate::resources::memory::scope(|| {
        cancel.poll()?;
        validate(request)?;
        nonnegative(witness.widening, "witness widening")?;
        if witness.nodes.len() != request.times.len() || radii.len() != request.times.len() {
            return Err(Error::Invalid(
                "whole common-frame witness chain required".into(),
            ));
        }
        for &radius in radii {
            nonnegative(radius, "common-frame radius")?;
        }
        for node in &witness.nodes {
            cancel.poll()?;
            numerics::validate_matrix(node)?;
            if node.n != request.features.len() {
                return Err(Error::Invalid("common-frame witness dimensions".into()));
            }
        }
        for node in &witness.nodes {
            cancel.poll()?;
            for j in 0..node.n {
                for i in 0..node.n {
                    if i != j && node.at(i, j) != 0. {
                        return Ok(Feasibility::UnsupportedModel);
                    }
                }
            }
        }
        let observed = calibration::normalized(request)?;
        for (date, node) in witness.nodes.iter().enumerate() {
            cancel.poll()?;
            if (0..node.n).any(|i| node.at(i, i) < 0.)
                || numerics::difference_norm_upper(node, &observed[date], cancel)?
                    > (point(radii[date]) + point(witness.widening)).inf()
            {
                return Ok(Feasibility::Unresolved);
            }
            if date > 0 {
                let mut step = 0f64;
                for i in 0..node.n {
                    step = step.max(upper(
                        (point(node.at(i, i)) - point(witness.nodes[date - 1].at(i, i))).abs(),
                    )?);
                }
                let allowance = point(request.lipschitz)
                    * (point(request.times[date]) - point(request.times[date - 1]));
                if step > allowance.inf() {
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
