//! Initialized exact-polar geometry; covariance uncertainty has a separate scope.
use crate::{
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "eigentrail.transport.v1";
pub const SCOPE: &str = "initialized exact-polar short-geodesic geometry; exact-path comparison is uniform; numerical frames are checked at returned nodes; no covariance or whole-history validity is implied";
pub const COVARIANCE_SCOPE: &str = "conditional population transport on the common covariance event and declared global regularity; population-compatible initial family via principal rotation; observation spacing and initial uncertainty retained; no whole-history feasibility claim";
pub const RESOURCE_SCOPE: &str = "pending audit of native allocations and interruption latency";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportRequest {
    pub schema: String,
    pub dimension: usize,
    pub times: Vec<f64>,
    pub reference_nodes: Vec<Matrix>,
    pub nodes: Vec<Matrix>,
    pub block: [usize; 2],
    pub initial_frame: Matrix,
    pub initial_gauge: Matrix,
    pub reference_speed_limit: f64,
    pub max_speed: f64,
    pub endpoint_error_limit: f64,
    pub initial_correction_limit: f64,
    pub initial_mismatch_limit: f64,
    pub target: f64,
    pub memory_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameProof {
    pub polar: f64,
    pub projector_error: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistanceProof {
    pub eigensystem: Eigensystem,
    pub distance: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializationProof {
    pub requested_polar: f64,
    pub gauge_polar: f64,
    pub reference_basis: Matrix,
    pub basis: Matrix,
    pub reference_error: f64,
    pub basis_error: f64,
    pub correction: f64,
    pub mismatch: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepProof {
    pub reference: DistanceProof,
    pub reconstructed: DistanceProof,
    pub rotation: Vec<RotationProof>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationProof {
    pub matrix: Matrix,
    pub reference_angle: f64,
    pub angle: f64,
    pub local_error: f64,
    pub product_error: f64,
    pub frame: Matrix,
    pub frame_error: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportBudget {
    pub reference_speed: f64,
    pub speed: f64,
    pub endpoint_error: f64,
    pub initial_correction: f64,
    pub initial_mismatch: f64,
    pub product_error: f64,
    pub numerical_error: f64,
    pub mesh: f64,
    pub comparison_error: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CovarianceBudget {
    pub projector_speed: f64,
    pub endpoint_error: f64,
    pub initial_uncertainty: f64,
    pub mesh: f64,
    pub comparison_error: f64,
    pub total_error: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CovarianceProof {
    pub observation_indices: Vec<usize>,
    pub distances: Vec<DistanceProof>,
    pub gaps: Vec<f64>,
    pub node_gaps: Vec<f64>,
    pub budget: Vec<CovarianceBudget>,
    pub outcome: Outcome,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gate {
    InitialFibre,
    InitialMismatch,
    EndpointError,
    CutLocus,
    ReferenceSpeed,
    Speed,
    Accuracy,
    CovarianceGap,
    CovarianceInitial,
    CovarianceAccuracy,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Outcome {
    Resolved,
    Unresolved { gate: Gate },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportAssessment {
    pub schema: String,
    pub request_hash: String,
    pub reference_bounds: Vec<FrameProof>,
    pub node_bounds: Vec<FrameProof>,
    pub initial: Vec<InitializationProof>,
    pub endpoints: Vec<DistanceProof>,
    pub steps: Vec<StepProof>,
    pub rotation: Vec<Matrix>,
    pub frame: Vec<Matrix>,
    pub budget: Vec<TransportBudget>,
    pub covariance: Vec<CovarianceProof>,
    pub outcome: Outcome,
    pub scope: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}

pub fn preflight(p: usize, nodes: usize, limit: usize) -> Result<usize> {
    if nodes > 65537 {
        return Err(Error::Exhausted(
            "transport refinement limit 65536 cells".into(),
        ));
    }
    if !(1..=256).contains(&p) || nodes < 2 {
        return Err(Error::Invalid(
            "transport dimension 1..256 and at least two nodes required".into(),
        ));
    }
    let bytes = p
        .checked_mul(p)
        .and_then(|n| n.checked_mul(832))
        .and_then(|n| n.checked_add(3072))
        .and_then(|n| n.checked_mul(nodes))
        .and_then(|n| n.checked_add(p * p * 1024))
        .and_then(|n| n.checked_add(4 * 1024 * 1024))
        .ok_or_else(|| Error::Resource("transport reservation overflow".into()))?;
    if limit > resources::DEFAULT_BYTES || bytes > limit {
        return Err(Error::Resource(
            "transport managed allocation reservation exceeds budget".into(),
        ));
    }
    Ok(bytes)
}
fn frame_shape(a: &Matrix, n: usize) -> Result<()> {
    if a.n != n || a.data.len() != n * n || a.data.iter().any(|x| !x.is_finite()) {
        return Err(Error::Invalid(
            "finite square transport frame required".into(),
        ));
    }
    Ok(())
}
pub fn validate_request(r: &TransportRequest) -> Result<usize> {
    let reserved = preflight(r.dimension, r.times.len(), r.memory_bytes)?;
    if r.schema != SCHEMA
        || r.block[0] >= r.block[1]
        || r.block[1] > r.dimension
        || r.reference_nodes.len() != r.times.len()
        || r.nodes.len() != r.times.len()
        || r.times.iter().any(|x| !x.is_finite())
        || r.times.windows(2).any(|x| x[0] >= x[1])
    {
        return Err(Error::Invalid(
            "transport schema, ordered times, complete frames and block required".into(),
        ));
    }
    for (x, name) in [
        (r.reference_speed_limit, "reference speed"),
        (r.max_speed, "speed"),
        (r.endpoint_error_limit, "endpoint error"),
        (r.initial_correction_limit, "initial correction"),
        (r.initial_mismatch_limit, "initial mismatch"),
        (r.target, "target"),
    ] {
        nonnegative(x, name)?;
    }
    upper(point(*r.times.last().unwrap()) - point(r.times[0]))?;
    for a in r.reference_nodes.iter().chain(&r.nodes) {
        frame_shape(a, r.dimension)?;
    }
    frame_shape(&r.initial_frame, r.dimension)?;
    frame_shape(&r.initial_gauge, r.block[1] - r.block[0])?;
    Ok(reserved)
}
pub fn validate_history(obs: &TraceRequest, r: &TransportRequest) -> Result<usize> {
    let reserved = validate_request(r)?;
    if obs.features.len() != r.dimension
        || obs.memory_bytes != r.memory_bytes
        || obs.times.first() != r.times.first()
        || obs.times.last() != r.times.last()
        || obs
            .times
            .iter()
            .any(|t| r.times.binary_search_by(|x| x.total_cmp(t)).is_err())
    {
        return Err(Error::Invalid("transport model must contain all observation times with matching dimension, budget and domain".into()));
    }
    Ok(reserved)
}

fn identity(n: usize) -> Result<Matrix> {
    let mut data = resources::zeros(n * n)?;
    for i in 0..n {
        data[i + i * n] = 1.;
    }
    Ok(Matrix { n, data })
}

fn projector(a: &Matrix, block: [usize; 2], cancel: &Cancellation) -> Result<(Matrix, FrameProof)> {
    memory::scope(|| {
        let d = crate::numerics::frame_polar(a, cancel)?;
        if block == [0, a.n] {
            return Ok((
                identity(a.n)?,
                FrameProof {
                    polar: d,
                    projector_error: 0.,
                },
            ));
        }
        let mut data = resources::zeros(a.n * a.n)?;
        let mut round = point(0.);
        for j in 0..a.n {
            cancel.poll()?;
            for i in 0..a.n {
                let x = (block[0]..block[1])
                    .fold(point(0.), |s, k| s + point(a.at(i, k)) * point(a.at(j, k)));
                let mid = x.inf() / 2. + x.sup() / 2.;
                data[i + j * a.n] = mid;
                round += square(x - point(mid));
            }
        }
        let error = upper(round.sqrt() + (point(2.) + point(d)) * point(d))?;
        Ok((
            Matrix { n: a.n, data },
            FrameProof {
                polar: d,
                projector_error: error,
            },
        ))
    })
}
fn distance(
    a: &Matrix,
    da: f64,
    b: &Matrix,
    db: f64,
    cancel: &Cancellation,
) -> Result<DistanceProof> {
    memory::scope(|| {
        let mut difference = Matrix {
            n: a.n,
            data: resources::zeros(a.n * a.n)?,
        };
        let mut rounding = point(0.);
        for (i, value) in difference.data.iter_mut().enumerate() {
            let x = point(a.data[i]) - point(b.data[i]);
            *value = x.inf() / 2. + x.sup() / 2.;
            rounding += square(x - point(*value));
        }
        let eigensystem = if difference.data.iter().all(|x| *x == 0.) {
            Eigensystem {
                values: resources::zeros(a.n)?,
                vectors: identity(a.n)?,
                backward: 0.,
                polar: 0.,
            }
        } else {
            numerics::eigensystem(&difference, cancel)?
        };
        let norm = eigensystem.values.iter().fold(0f64, |s, x| s.max(x.abs()));
        let distance = upper(
            point(norm) + point(eigensystem.backward) + rounding.sqrt() + point(da) + point(db),
        )?;
        Ok(DistanceProof {
            eigensystem,
            distance,
        })
    })
}
fn product(a: &Matrix, b: &Matrix, cancel: &Cancellation) -> Result<(Matrix, f64)> {
    let mut data = resources::zeros(a.n * a.n)?;
    let mut rounding = point(0.);
    for j in 0..a.n {
        cancel.poll()?;
        for i in 0..a.n {
            let x = (0..a.n).fold(point(0.), |s, k| s + point(a.at(i, k)) * point(b.at(k, j)));
            let mid = x.inf() / 2. + x.sup() / 2.;
            data[i + j * a.n] = mid;
            rounding += square(x - point(mid));
        }
    }
    Ok((Matrix { n: a.n, data }, upper(rounding.sqrt())?))
}
fn basis(
    a: &Matrix,
    g: &Matrix,
    block: [usize; 2],
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    let mut data = resources::zeros(a.n * a.n)?;
    let mut rounding = point(0.);
    for j in block[0]..block[1] {
        cancel.poll()?;
        for i in 0..a.n {
            let x = (block[0]..block[1]).fold(point(0.), |s, k| {
                s + point(a.at(i, k)) * point(g.at(k - block[0], j - block[0]))
            });
            let mid = x.inf() / 2. + x.sup() / 2.;
            data[i + j * a.n] = mid;
            rounding += square(x - point(mid));
        }
    }
    Ok((Matrix { n: a.n, data }, upper(rounding.sqrt())?))
}
fn difference_norm(
    a: &Matrix,
    b: &Matrix,
    block: [usize; 2],
    cancel: &Cancellation,
) -> Result<f64> {
    let mut sum = point(0.);
    for j in block[0]..block[1] {
        cancel.poll()?;
        for i in 0..a.n {
            sum += square(point(a.at(i, j)) - point(b.at(i, j)));
        }
    }
    upper(sum.sqrt())
}
fn initialize(
    r: &TransportRequest,
    dp: f64,
    dq: f64,
    cancel: &Cancellation,
) -> Result<InitializationProof> {
    memory::scope(|| {
        let requested_polar = crate::numerics::frame_polar(&r.initial_frame, cancel)?;
        let gauge_polar = crate::numerics::frame_polar(&r.initial_gauge, cancel)?;
        let (reference_basis, ref_round) =
            basis(&r.reference_nodes[0], &r.initial_gauge, r.block, cancel)?;
        let (basis, round) = basis(&r.nodes[0], &r.initial_gauge, r.block, cancel)?;
        let reference_error = upper(
            point(dp) + point(gauge_polar) + point(dp) * point(gauge_polar) + point(ref_round),
        )?;
        let basis_error =
            upper(point(dq) + point(gauge_polar) + point(dq) * point(gauge_polar) + point(round))?;
        let correction = upper(
            point(difference_norm(
                &reference_basis,
                &r.initial_frame,
                r.block,
                cancel,
            )?) + point(reference_error)
                + point(requested_polar),
        )?;
        let mismatch = upper(
            point(difference_norm(&reference_basis, &basis, r.block, cancel)?)
                + point(reference_error)
                + point(basis_error),
        )?;
        Ok(InitializationProof {
            requested_polar,
            gauge_polar,
            reference_basis,
            basis,
            reference_error,
            basis_error,
            correction,
            mismatch,
        })
    })
}
fn rotation(
    a: &Matrix,
    da: f64,
    b: &Matrix,
    db: f64,
    distance: f64,
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    memory::scope(|| {
        let n = a.n;
        let mut s = memory::capacity(n * n)?;
        let mut mid = resources::zeros(n * n)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..n {
                let product =
                    (0..n).fold(point(0.), |v, k| v + point(b.at(i, k)) * point(a.at(k, j)));
                let x = point(f64::from(i == j)) - point(a.at(i, j)) - point(b.at(i, j))
                    + point(2.) * product;
                mid[i + j * n] = x.inf() / 2. + x.sup() / 2.;
                s.push(x);
            }
        }
        let matrix = transport::polar_proposal(&mid, n, cancel)?;
        let d = crate::numerics::frame_polar(&matrix, cancel)?;
        let mut residual = point(0.);
        for j in 0..n {
            for i in 0..n {
                residual += square(s[i + j * n] - point(matrix.at(i, j)));
            }
        }
        let source = point(3.) * (point(da) + point(db)) + point(2.) * point(da) * point(db);
        let sigma = (point(1.) - square(point(distance))).sqrt();
        let local = upper(
            point(2.) * (residual.sqrt() + source + point(d)) / (sigma + point(1.)) + point(d),
        )?;
        Ok((matrix, local))
    })
}

pub fn analyze(r: &TransportRequest, cancel: &Cancellation) -> Result<TransportAssessment> {
    cancel.poll()?;
    validate_request(r)?;
    memory::ensure_operation(r.memory_bytes, r.heap_bytes()?, || analyze_owned(r, cancel))
}
fn analyze_owned(r: &TransportRequest, cancel: &Cancellation) -> Result<TransportAssessment> {
    let managed_reservation = validate_request(r)?;
    let n = r.times.len();
    let mut out = TransportAssessment {
        schema: memory::text(SCHEMA)?,
        request_hash: serialization::digest_with_cancel(r, cancel)?,
        reference_bounds: memory::capacity(n)?,
        node_bounds: memory::capacity(n)?,
        initial: memory::capacity(1)?,
        endpoints: Vec::new(),
        steps: Vec::new(),
        rotation: Vec::new(),
        frame: Vec::new(),
        budget: Vec::new(),
        covariance: Vec::new(),
        outcome: Outcome::Resolved,
        scope: memory::text(SCOPE)?,
        completed: true,
        managed_reservation,
        resource_qualification: memory::text(RESOURCE_SCOPE)?,
    };
    for i in 0..n {
        cancel.poll()?;
        let bounds = memory::scope(|| {
            Ok((
                projector(&r.reference_nodes[i], r.block, cancel)?.1,
                projector(&r.nodes[i], r.block, cancel)?.1,
            ))
        })?;
        out.reference_bounds.push(bounds.0);
        out.node_bounds.push(bounds.1);
    }
    let initial = initialize(
        r,
        out.reference_bounds[0].polar,
        out.node_bounds[0].polar,
        cancel,
    )?;
    let correction = initial.correction;
    let mismatch = initial.mismatch;
    let mut numerical_error = initial.basis_error;
    out.initial.push(initial);
    for (bad, gate) in [
        (correction > r.initial_correction_limit, Gate::InitialFibre),
        (mismatch > r.initial_mismatch_limit, Gate::InitialMismatch),
    ] {
        if bad {
            out.outcome = Outcome::Unresolved { gate };
            return Ok(out);
        }
    }
    if numerical_error > 0.001 {
        return Err(Error::Exhausted(
            "transport numerical frame allowance .001".into(),
        ));
    }
    out.endpoints = memory::capacity(n)?;
    let mut endpoint_error = 0f64;
    for i in 0..n {
        cancel.poll()?;
        let proof = memory::scope(|| {
            let (a, da) = projector(&r.reference_nodes[i], r.block, cancel)?;
            let (b, db) = projector(&r.nodes[i], r.block, cancel)?;
            distance(&a, da.projector_error, &b, db.projector_error, cancel)
        })?;
        endpoint_error = endpoint_error.max(proof.distance);
        out.endpoints.push(proof);
    }
    if endpoint_error > r.endpoint_error_limit {
        out.outcome = Outcome::Unresolved {
            gate: Gate::EndpointError,
        };
        return Ok(out);
    }
    out.steps = memory::capacity(n - 1)?;
    let mut accumulated = identity(r.dimension)?;
    let mut product_error = 0.;
    let mut reference_speed = 0f64;
    let mut speed = 0f64;
    let mut mesh = 0f64;
    for i in 1..n {
        cancel.poll()?;
        let (step, next) = memory::scope(|| {
            let (pa, dpa) = projector(&r.reference_nodes[i - 1], r.block, cancel)?;
            let (pb, dpb) = projector(&r.reference_nodes[i], r.block, cancel)?;
            let reference = distance(&pa, dpa.projector_error, &pb, dpb.projector_error, cancel)?;
            let (qa, dqa) = projector(&r.nodes[i - 1], r.block, cancel)?;
            let (qb, dqb) = projector(&r.nodes[i], r.block, cancel)?;
            let reconstructed =
                distance(&qa, dqa.projector_error, &qb, dqb.projector_error, cancel)?;
            if reference.distance >= 1. || reconstructed.distance >= 1. {
                return Ok((
                    StepProof {
                        reference,
                        reconstructed,
                        rotation: Vec::new(),
                    },
                    None,
                ));
            }
            let reference_angle = upper(asin(point(reference.distance))?)?;
            let angle = upper(asin(point(reconstructed.distance))?)?;
            let (matrix, local_error) = rotation(
                &qa,
                dqa.projector_error,
                &qb,
                dqb.projector_error,
                reconstructed.distance,
                cancel,
            )?;
            let (next, round) = product(&matrix, &accumulated, cancel)?;
            let next_error = upper(
                point(product_error)
                    + point(local_error) * (point(1.) + point(product_error))
                    + point(round),
            )?;
            let (frame, frame_round) = product(&next, &out.initial[0].basis, cancel)?;
            let frame_error = upper(
                point(out.initial[0].basis_error)
                    + point(next_error) * (point(1.) + point(out.initial[0].basis_error))
                    + point(frame_round),
            )?;
            let mut rotation = memory::capacity(1)?;
            rotation.push(RotationProof {
                matrix,
                reference_angle,
                angle,
                local_error,
                product_error: next_error,
                frame,
                frame_error,
            });
            Ok((
                StepProof {
                    reference,
                    reconstructed,
                    rotation,
                },
                Some(next),
            ))
        })?;
        if step.rotation.is_empty() {
            out.steps.push(step);
            out.outcome = Outcome::Unresolved {
                gate: Gate::CutLocus,
            };
            return Ok(out);
        }
        let proof = &step.rotation[0];
        product_error = proof.product_error;
        numerical_error = numerical_error.max(proof.frame_error);
        if numerical_error > 0.001 {
            return Err(Error::Exhausted(
                "transport numerical frame allowance .001".into(),
            ));
        }
        let dt = point(r.times[i]) - point(r.times[i - 1]);
        mesh = mesh.max(upper(dt)?);
        reference_speed = reference_speed.max(upper(point(proof.reference_angle) / dt)?);
        speed = speed.max(upper(point(proof.angle) / dt)?);
        accumulated = next.unwrap();
        out.steps.push(step);
        let gate = if reference_speed > r.reference_speed_limit {
            Some(Gate::ReferenceSpeed)
        } else if speed > r.max_speed {
            Some(Gate::Speed)
        } else {
            None
        };
        if let Some(gate) = gate {
            out.outcome = Outcome::Unresolved { gate };
            return Ok(out);
        }
    }
    let duration = point(r.times[n - 1]) - point(r.times[0]);
    let epsilon =
        point(endpoint_error) + (point(reference_speed) + point(speed)) * point(mesh) / point(2.);
    let comparison_error = upper(
        (point(4.) + point(12.) * point(reference_speed.max(speed)) * duration) * epsilon
            + point(correction)
            + point(mismatch)
            + point(numerical_error),
    )?;
    out.rotation = memory::capacity(1)?;
    out.rotation.push(accumulated);
    out.frame = memory::capacity(1)?;
    out.frame.push(Matrix {
        n: r.dimension,
        data: memory::copy(&out.steps.last().unwrap().rotation[0].frame.data)?,
    });
    out.budget = memory::capacity(1)?;
    out.budget.push(TransportBudget {
        reference_speed,
        speed,
        endpoint_error,
        initial_correction: correction,
        initial_mismatch: mismatch,
        product_error,
        numerical_error,
        mesh,
        comparison_error,
    });
    if comparison_error >= r.target {
        out.outcome = Outcome::Unresolved {
            gate: Gate::Accuracy,
        };
    }
    Ok(out)
}

pub(crate) fn analyze_history(
    obs: &TraceRequest,
    baseline: &TraceReport,
    r: &TransportRequest,
    cancel: &Cancellation,
) -> Result<TransportAssessment> {
    cancel.poll()?;
    validate_history(obs, r)?;
    memory::scope(|| {
        let mut report = analyze(r, cancel)?;
        if report.outcome == Outcome::Resolved {
            report.covariance = memory::capacity(1)?;
            report
                .covariance
                .push(covariance(obs, baseline, r, &report, cancel)?);
        }
        Ok(report)
    })
}
fn covariance(
    obs: &TraceRequest,
    base: &TraceReport,
    r: &TransportRequest,
    report: &TransportAssessment,
    cancel: &Cancellation,
) -> Result<CovarianceProof> {
    memory::scope(|| {
        let n = obs.times.len();
        let p = r.dimension;
        let q = r.block[1] - r.block[0];
        if base.radii.len() != n || base.eigensystems.len() != n {
            return Err(Error::Invalid(
                "transport covariance baseline incomplete".into(),
            ));
        }
        let mut out = CovarianceProof {
            observation_indices: memory::capacity(n)?,
            distances: memory::capacity(n)?,
            gaps: Vec::new(),
            node_gaps: Vec::new(),
            budget: Vec::new(),
            outcome: Outcome::Resolved,
            scope: memory::text(COVARIANCE_SCOPE)?,
        };
        let mut boundaries = [0usize; 2];
        let mut count = 0;
        if r.block[0] > 0 {
            boundaries[count] = r.block[0] - 1;
            count += 1;
        }
        if r.block[1] < p {
            boundaries[count] = r.block[1] - 1;
            count += 1;
        }
        out.node_gaps = memory::capacity(n * count)?;
        out.gaps = memory::capacity((n - 1) * count)?;
        let root_q = point(q as f64).sqrt();
        let mut node_gate = false;
        let mut delta = 0f64;
        let mut delta0 = 0.;
        for j in 0..n {
            cancel.poll()?;
            let index = r
                .times
                .binary_search_by(|t| t.total_cmp(&obs.times[j]))
                .map_err(|_| Error::Invalid("missing observation transport node".into()))?;
            out.observation_indices.push(index);
            let proof = memory::scope(|| {
                let (a, da) = projector(&r.reference_nodes[index], r.block, cancel)?;
                let (b, db) = projector(&base.eigensystems[j].vectors, r.block, cancel)?;
                distance(&a, da.projector_error, &b, db.projector_error, cancel)
            })?;
            let e = point(base.radii[j]) + point(base.eigensystems[j].backward);
            let mut min_gap = f64::MAX;
            for &boundary in &boundaries[..count] {
                let values = &base.eigensystems[j].values;
                let gap = point(values[boundary]) - point(values[boundary + 1]) - point(2.) * e;
                let lower = gap.inf();
                out.node_gaps.push(lower);
                min_gap = min_gap.min(lower);
                node_gate |= lower <= (point(2.) * e).sup();
            }
            let error = if count == 0 {
                0.
            } else if min_gap <= 0. {
                1.
            } else {
                let d = point(base.eigensystems[j].polar);
                upper(point(2.) * root_q * e / point(min_gap) + (point(2.) + d) * d)?.min(1.)
            };
            let dj = upper(point(error) + point(proof.distance))?;
            delta = delta.max(dj);
            if j == 0 {
                delta0 = dj;
            }
            out.distances.push(proof);
        }
        let mut gap = f64::MAX;
        let mut mesh = 0f64;
        for j in 0..n - 1 {
            cancel.poll()?;
            let dt = point(obs.times[j + 1]) - point(obs.times[j]);
            mesh = mesh.max(upper(dt)?);
            for k in 0..count {
                let a = point(out.node_gaps[j * count + k]);
                let b = point(out.node_gaps[(j + 1) * count + k]);
                let kd = point(obs.lipschitz) * dt;
                let g = (a - point(2.) * kd)
                    .inf()
                    .max((b - point(2.) * kd).inf())
                    .max(((a + b) / point(2.) - kd).inf());
                out.gaps.push(g);
                gap = gap.min(g);
                node_gate |= g <= 0.;
            }
        }
        if node_gate {
            out.outcome = Outcome::Unresolved {
                gate: Gate::CovarianceGap,
            };
            return Ok(out);
        }
        if delta0 >= 1. {
            out.outcome = Outcome::Unresolved {
                gate: Gate::CovarianceInitial,
            };
            return Ok(out);
        }
        let speed = if count == 0 {
            0.
        } else {
            upper(point(2.) * root_q * point(obs.lipschitz) / point(gap))?
        };
        let lp = report.budget[0].reference_speed;
        let duration = point(*obs.times.last().unwrap()) - point(obs.times[0]);
        let initial = upper(point(2.) * point(delta0))?;
        let epsilon = point(delta) + (point(speed) + point(lp)) * point(mesh) / point(2.);
        let extra = upper(
            (point(4.) + point(12.) * point(speed.max(lp)) * duration) * epsilon + point(initial),
        )?;
        let total = upper(point(report.budget[0].comparison_error) + point(extra))?;
        out.budget = memory::capacity(1)?;
        out.budget.push(CovarianceBudget {
            projector_speed: speed,
            endpoint_error: delta,
            initial_uncertainty: initial,
            mesh,
            comparison_error: extra,
            total_error: total,
        });
        if total >= r.target {
            out.outcome = Outcome::Unresolved {
                gate: Gate::CovarianceAccuracy,
            };
        }
        Ok(out)
    })
}

pub(crate) fn try_clone(r: &TransportRequest) -> Result<TransportRequest> {
    let clone_nodes = |nodes: &[Matrix]| -> Result<Vec<Matrix>> {
        let mut out = memory::capacity(nodes.len())?;
        for a in nodes {
            out.push(Matrix {
                n: a.n,
                data: memory::copy(&a.data)?,
            });
        }
        Ok(out)
    };
    Ok(TransportRequest {
        schema: memory::text(&r.schema)?,
        dimension: r.dimension,
        times: memory::copy(&r.times)?,
        reference_nodes: clone_nodes(&r.reference_nodes)?,
        nodes: clone_nodes(&r.nodes)?,
        block: r.block,
        initial_frame: Matrix {
            n: r.initial_frame.n,
            data: memory::copy(&r.initial_frame.data)?,
        },
        initial_gauge: Matrix {
            n: r.initial_gauge.n,
            data: memory::copy(&r.initial_gauge.data)?,
        },
        reference_speed_limit: r.reference_speed_limit,
        max_speed: r.max_speed,
        endpoint_error_limit: r.endpoint_error_limit,
        initial_correction_limit: r.initial_correction_limit,
        initial_mismatch_limit: r.initial_mismatch_limit,
        target: r.target,
        memory_bytes: r.memory_bytes,
    })
}
