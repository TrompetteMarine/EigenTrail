//! Independent reconstruction of initialized geometry and covariance transport.
use eigentrail_core::{
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    transport_workflow::{
        COVARIANCE_SCOPE, CovarianceBudget, DistanceProof, FrameProof, Gate, InitializationProof,
        Outcome, RESOURCE_SCOPE, SCHEMA, SCOPE, TransportAssessment, TransportBudget,
        TransportRequest,
    },
    *,
};

fn bad(message: &str) -> Error {
    Error::Invalid(format!("transport checker: {message}"))
}

fn shape(a: &Matrix, n: usize) -> Result<()> {
    if a.n != n || a.data.len() != n * n || a.data.iter().any(|x| !x.is_finite()) {
        return Err(bad("finite square frame shape"));
    }
    Ok(())
}

fn admission(r: &TransportRequest) -> Result<usize> {
    let p = r.dimension;
    let n = r.times.len();
    if n > 65537 {
        return Err(Error::Exhausted(
            "transport refinement limit 65536 cells".into(),
        ));
    }
    if !(1..=256).contains(&p) || n < 2 {
        return Err(bad("transport dimension or mesh"));
    }
    let reserved = p
        .checked_mul(p)
        .and_then(|v| v.checked_mul(832))
        .and_then(|v| v.checked_add(3072))
        .and_then(|v| v.checked_mul(n))
        .and_then(|v| v.checked_add(p * p * 1024))
        .and_then(|v| v.checked_add(4 * 1024 * 1024))
        .ok_or_else(|| Error::Resource(String::new()))?;
    if r.memory_bytes > resources::DEFAULT_BYTES || reserved > r.memory_bytes {
        return Err(Error::Resource(
            "transport shape exceeds managed budget".into(),
        ));
    }
    if r.schema != SCHEMA
        || r.block[0] >= r.block[1]
        || r.block[1] > p
        || r.reference_nodes.len() != n
        || r.nodes.len() != n
        || r.times.iter().any(|x| !x.is_finite())
        || r.times.windows(2).any(|x| x[0] >= x[1])
    {
        return Err(bad(
            "schema, block, ordered times or complete node universe",
        ));
    }
    for (value, name) in [
        (r.reference_speed_limit, "reference speed"),
        (r.max_speed, "speed"),
        (r.endpoint_error_limit, "endpoint error"),
        (r.initial_correction_limit, "initial correction"),
        (r.initial_mismatch_limit, "initial mismatch"),
        (r.target, "target"),
    ] {
        nonnegative(value, name)?;
    }
    upper(point(r.times[n - 1]) - point(r.times[0]))?;
    for a in r.reference_nodes.iter().chain(&r.nodes) {
        shape(a, p)?;
    }
    shape(&r.initial_frame, p)?;
    shape(&r.initial_gauge, r.block[1] - r.block[0])?;
    Ok(reserved)
}

fn identity(n: usize) -> Result<Matrix> {
    let mut data = resources::zeros(n * n)?;
    for i in 0..n {
        data[i + i * n] = 1.;
    }
    Ok(Matrix { n, data })
}

fn polar(a: &Matrix, cancel: &Cancellation) -> Result<f64> {
    let mut defect = point(0.);
    for row in 0..a.n {
        cancel.poll()?;
        for col in 0..a.n {
            let mut dot = point(0.);
            for k in 0..a.n {
                dot += point(a.at(k, row)) * point(a.at(k, col));
            }
            defect += square(dot - point(f64::from(row == col)));
        }
    }
    let tau = defect.sqrt();
    if tau.sup() >= 1. {
        return Err(Error::Exhausted("transport frame polar gate".into()));
    }
    upper(tau / (point(1.) + (point(1.) - tau).sqrt()))
}

fn projector(a: &Matrix, block: [usize; 2], cancel: &Cancellation) -> Result<(Matrix, FrameProof)> {
    memory::scope(|| {
        let correction = polar(a, cancel)?;
        if block == [0, a.n] {
            return Ok((
                identity(a.n)?,
                FrameProof {
                    polar: correction,
                    projector_error: 0.,
                },
            ));
        }
        let mut data = resources::zeros(a.n * a.n)?;
        let mut rounding = point(0.);
        for col in 0..a.n {
            cancel.poll()?;
            for row in 0..a.n {
                let mut exact = point(0.);
                for k in block[0]..block[1] {
                    exact += point(a.at(row, k)) * point(a.at(col, k));
                }
                let mid = exact.inf() / 2. + exact.sup() / 2.;
                if !mid.is_finite() {
                    return Err(Error::Exhausted("projector midpoint".into()));
                }
                data[row + col * a.n] = mid;
                rounding += square(exact - point(mid));
            }
        }
        let error = upper(rounding.sqrt() + (point(2.) + point(correction)) * point(correction))?;
        Ok((
            Matrix { n: a.n, data },
            FrameProof {
                polar: correction,
                projector_error: error,
            },
        ))
    })
}

fn distance(
    left: &Matrix,
    left_error: f64,
    right: &Matrix,
    right_error: f64,
    proof: &DistanceProof,
    cancel: &Cancellation,
) -> Result<f64> {
    memory::scope(|| {
        let n = left.n;
        let mut intended = Matrix {
            n,
            data: resources::zeros(n * n)?,
        };
        let mut rounding = point(0.);
        for col in 0..n {
            cancel.poll()?;
            for row in 0..n {
                let i = row + col * n;
                let exact = point(left.data[i]) - point(right.data[i]);
                let mid = exact.inf() / 2. + exact.sup() / 2.;
                if !mid.is_finite() {
                    return Err(Error::Exhausted("distance midpoint".into()));
                }
                intended.data[i] = mid;
                rounding += square(exact - point(mid));
            }
        }
        crate::eigensystem::inspect(&intended, &proof.eigensystem, cancel)?;
        let norm = proof
            .eigensystem
            .values
            .iter()
            .fold(0f64, |s, x| s.max(x.abs()));
        let bound = upper(
            point(norm)
                + point(proof.eigensystem.backward)
                + rounding.sqrt()
                + point(left_error)
                + point(right_error),
        )?;
        if proof.distance != bound {
            return Err(bad("projector distance or numerical bound"));
        }
        Ok(bound)
    })
}

fn basis(
    a: &Matrix,
    gauge: &Matrix,
    block: [usize; 2],
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    let mut out = Matrix {
        n: a.n,
        data: resources::zeros(a.n * a.n)?,
    };
    let mut rounding = point(0.);
    for col in block[0]..block[1] {
        cancel.poll()?;
        for row in 0..a.n {
            let mut exact = point(0.);
            for k in block[0]..block[1] {
                exact += point(a.at(row, k)) * point(gauge.at(k - block[0], col - block[0]));
            }
            let mid = exact.inf() / 2. + exact.sup() / 2.;
            if !mid.is_finite() {
                return Err(Error::Exhausted("initial basis midpoint".into()));
            }
            out.data[row + col * a.n] = mid;
            rounding += square(exact - point(mid));
        }
    }
    Ok((out, upper(rounding.sqrt())?))
}

fn difference(
    left: &Matrix,
    right: &Matrix,
    block: [usize; 2],
    cancel: &Cancellation,
) -> Result<f64> {
    let mut squared = point(0.);
    for col in block[0]..block[1] {
        cancel.poll()?;
        for row in 0..left.n {
            squared += square(point(left.at(row, col)) - point(right.at(row, col)));
        }
    }
    upper(squared.sqrt())
}

fn initialization(
    r: &TransportRequest,
    reference_polar: f64,
    node_polar: f64,
    proof: &InitializationProof,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        let requested_polar = polar(&r.initial_frame, cancel)?;
        let gauge_polar = polar(&r.initial_gauge, cancel)?;
        let (reference, reference_round) =
            basis(&r.reference_nodes[0], &r.initial_gauge, r.block, cancel)?;
        let (reconstructed, node_round) = basis(&r.nodes[0], &r.initial_gauge, r.block, cancel)?;
        let reference_error = upper(
            point(reference_polar)
                + point(gauge_polar)
                + point(reference_polar) * point(gauge_polar)
                + point(reference_round),
        )?;
        let basis_error = upper(
            point(node_polar)
                + point(gauge_polar)
                + point(node_polar) * point(gauge_polar)
                + point(node_round),
        )?;
        let correction = upper(
            point(difference(&reference, &r.initial_frame, r.block, cancel)?)
                + point(reference_error)
                + point(requested_polar),
        )?;
        let mismatch = upper(
            point(difference(&reference, &reconstructed, r.block, cancel)?)
                + point(reference_error)
                + point(basis_error),
        )?;
        if proof.requested_polar != requested_polar
            || proof.gauge_polar != gauge_polar
            || proof.reference_basis != reference
            || proof.basis != reconstructed
            || proof.reference_error != reference_error
            || proof.basis_error != basis_error
            || proof.correction != correction
            || proof.mismatch != mismatch
        {
            return Err(bad(
                "initial fibre representative, gauge, correction or mismatch",
            ));
        }
        Ok(())
    })
}

fn multiply_into(
    left: &Matrix,
    right: &Matrix,
    out: &mut Matrix,
    cancel: &Cancellation,
) -> Result<f64> {
    let n = left.n;
    let mut rounding = point(0.);
    for col in 0..n {
        cancel.poll()?;
        for row in 0..n {
            let mut exact = point(0.);
            for k in 0..n {
                exact += point(left.at(row, k)) * point(right.at(k, col));
            }
            let mid = exact.inf() / 2. + exact.sup() / 2.;
            if !mid.is_finite() {
                return Err(Error::Exhausted("transport product midpoint".into()));
            }
            out.data[row + col * n] = mid;
            rounding += square(exact - point(mid));
        }
    }
    upper(rounding.sqrt())
}

fn local_rotation_error(
    left: &Matrix,
    left_error: f64,
    right: &Matrix,
    right_error: f64,
    distance_bound: f64,
    proposed: &Matrix,
    cancel: &Cancellation,
) -> Result<f64> {
    shape(proposed, left.n)?;
    let correction = polar(proposed, cancel)?;
    let n = left.n;
    let mut residual = point(0.);
    for col in 0..n {
        cancel.poll()?;
        for row in 0..n {
            let mut qp = point(0.);
            for k in 0..n {
                qp += point(right.at(row, k)) * point(left.at(k, col));
            }
            let intended =
                point(f64::from(row == col)) - point(left.at(row, col)) - point(right.at(row, col))
                    + point(2.) * qp;
            residual += square(intended - point(proposed.at(row, col)));
        }
    }
    let source = point(3.) * (point(left_error) + point(right_error))
        + point(2.) * point(left_error) * point(right_error);
    let sigma = (point(1.) - square(point(distance_bound))).sqrt();
    upper(
        point(2.) * (residual.sqrt() + source + point(correction)) / (sigma + point(1.))
            + point(correction),
    )
}

fn no_final(report: &TransportAssessment) -> bool {
    report.rotation.is_empty() && report.frame.is_empty() && report.budget.is_empty()
}

fn unresolved(report: &TransportAssessment, gate: Gate) -> Result<()> {
    if report.outcome != (Outcome::Unresolved { gate }) {
        return Err(bad("incorrect unresolved gate"));
    }
    Ok(())
}

fn geometry(
    r: &TransportRequest,
    report: &TransportAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    let reserved = admission(r)?;
    let n = r.times.len();
    if report.schema != SCHEMA
        || report.request_hash != serialization::digest_with_cancel(r, cancel)?
        || report.scope != SCOPE
        || !report.completed
        || report.managed_reservation != reserved
        || report.resource_qualification != RESOURCE_SCOPE
        || report.reference_bounds.len() != n
        || report.node_bounds.len() != n
        || report.initial.len() != 1
    {
        return Err(bad(
            "schema, binding, scope, completion, bounds or resources",
        ));
    }
    for j in 0..n {
        cancel.poll()?;
        memory::scope(|| {
            let (_, reference) = projector(&r.reference_nodes[j], r.block, cancel)?;
            let (_, node) = projector(&r.nodes[j], r.block, cancel)?;
            if report.reference_bounds[j] != reference || report.node_bounds[j] != node {
                return Err(bad("endpoint polar/projector representation"));
            }
            Ok(())
        })?;
    }
    let initial = &report.initial[0];
    initialization(
        r,
        report.reference_bounds[0].polar,
        report.node_bounds[0].polar,
        initial,
        cancel,
    )?;
    let initial_gate = if initial.correction > r.initial_correction_limit {
        Some(Gate::InitialFibre)
    } else if initial.mismatch > r.initial_mismatch_limit {
        Some(Gate::InitialMismatch)
    } else {
        None
    };
    if let Some(gate) = initial_gate {
        if !report.endpoints.is_empty() || !report.steps.is_empty() || !no_final(report) {
            return Err(bad("claims after a refused initialization"));
        }
        return unresolved(report, gate);
    }
    let mut numerical_error = initial.basis_error;
    if numerical_error > 0.001 {
        return Err(Error::Exhausted("transport numerical frame budget".into()));
    }
    if report.endpoints.len() != n {
        return Err(bad("incomplete endpoint comparison universe"));
    }
    let mut endpoint_error = 0f64;
    for j in 0..n {
        cancel.poll()?;
        let bound = memory::scope(|| {
            let (p, dp) = projector(&r.reference_nodes[j], r.block, cancel)?;
            let (q, dq) = projector(&r.nodes[j], r.block, cancel)?;
            distance(
                &p,
                dp.projector_error,
                &q,
                dq.projector_error,
                &report.endpoints[j],
                cancel,
            )
        })?;
        endpoint_error = endpoint_error.max(bound);
    }
    if endpoint_error > r.endpoint_error_limit {
        if !report.steps.is_empty() || !no_final(report) {
            return Err(bad("claims after failed endpoint accuracy"));
        }
        return unresolved(report, Gate::EndpointError);
    }
    let mut accumulated = identity(r.dimension)?;
    let mut next = Matrix {
        n: r.dimension,
        data: resources::zeros(r.dimension * r.dimension)?,
    };
    let mut product_error = 0.;
    let mut reference_speed = 0f64;
    let mut speed = 0f64;
    let mut mesh = 0f64;
    for j in 0..n - 1 {
        cancel.poll()?;
        let step = report
            .steps
            .get(j)
            .ok_or_else(|| bad("missing transport step"))?;
        let stopped = memory::scope(|| {
            let (pa, dpa) = projector(&r.reference_nodes[j], r.block, cancel)?;
            let (pb, dpb) = projector(&r.reference_nodes[j + 1], r.block, cancel)?;
            let reference_distance = distance(
                &pa,
                dpa.projector_error,
                &pb,
                dpb.projector_error,
                &step.reference,
                cancel,
            )?;
            let (qa, dqa) = projector(&r.nodes[j], r.block, cancel)?;
            let (qb, dqb) = projector(&r.nodes[j + 1], r.block, cancel)?;
            let node_distance = distance(
                &qa,
                dqa.projector_error,
                &qb,
                dqb.projector_error,
                &step.reconstructed,
                cancel,
            )?;
            if reference_distance >= 1. || node_distance >= 1. {
                if !step.rotation.is_empty() {
                    return Err(bad("rotation across unresolved cut locus"));
                }
                return Ok(Some(Gate::CutLocus));
            }
            if step.rotation.len() != 1 {
                return Err(bad("missing or extra numerical rotation"));
            }
            let rotation = &step.rotation[0];
            let reference_angle = upper(asin(point(reference_distance))?)?;
            let angle = upper(asin(point(node_distance))?)?;
            let local_error = local_rotation_error(
                &qa,
                dqa.projector_error,
                &qb,
                dqb.projector_error,
                node_distance,
                &rotation.matrix,
                cancel,
            )?;
            let round = multiply_into(&rotation.matrix, &accumulated, &mut next, cancel)?;
            let next_error = upper(
                point(product_error)
                    + point(local_error) * (point(1.) + point(product_error))
                    + point(round),
            )?;
            let mut frame = Matrix {
                n: r.dimension,
                data: resources::zeros(r.dimension * r.dimension)?,
            };
            let frame_round = multiply_into(&next, &initial.basis, &mut frame, cancel)?;
            let frame_error = upper(
                point(initial.basis_error)
                    + point(next_error) * (point(1.) + point(initial.basis_error))
                    + point(frame_round),
            )?;
            if rotation.reference_angle != reference_angle
                || rotation.angle != angle
                || rotation.local_error != local_error
                || rotation.product_error != next_error
                || rotation.frame != frame
                || rotation.frame_error != frame_error
            {
                return Err(bad(
                    "rotation, product, initialized frame or error components",
                ));
            }
            product_error = next_error;
            numerical_error = numerical_error.max(frame_error);
            if numerical_error > 0.001 {
                return Err(Error::Exhausted("transport numerical frame budget".into()));
            }
            let dt = point(r.times[j + 1]) - point(r.times[j]);
            mesh = mesh.max(upper(dt)?);
            reference_speed = reference_speed.max(upper(point(reference_angle) / dt)?);
            speed = speed.max(upper(point(angle) / dt)?);
            Ok(if reference_speed > r.reference_speed_limit {
                Some(Gate::ReferenceSpeed)
            } else if speed > r.max_speed {
                Some(Gate::Speed)
            } else {
                None
            })
        })?;
        if let Some(gate) = stopped {
            if report.steps.len() != j + 1 || !no_final(report) {
                return Err(bad("extra claims after an unresolved step"));
            }
            return unresolved(report, gate);
        }
        std::mem::swap(&mut accumulated.data, &mut next.data);
    }
    if report.steps.len() != n - 1
        || report.rotation.len() != 1
        || report.frame.len() != 1
        || report.budget.len() != 1
    {
        return Err(bad("missing or extra completed transport fields"));
    }
    let duration = point(r.times[n - 1]) - point(r.times[0]);
    let epsilon =
        point(endpoint_error) + (point(reference_speed) + point(speed)) * point(mesh) / point(2.);
    let comparison_error = upper(
        (point(4.) + point(12.) * point(reference_speed.max(speed)) * duration) * epsilon
            + point(initial.correction)
            + point(initial.mismatch)
            + point(numerical_error),
    )?;
    let budget = TransportBudget {
        reference_speed,
        speed,
        endpoint_error,
        initial_correction: initial.correction,
        initial_mismatch: initial.mismatch,
        product_error,
        numerical_error,
        mesh,
        comparison_error,
    };
    let outcome = if comparison_error >= r.target {
        Outcome::Unresolved {
            gate: Gate::Accuracy,
        }
    } else {
        Outcome::Resolved
    };
    if report.rotation[0] != accumulated
        || report.frame[0] != report.steps[n - 2].rotation[0].frame
        || report.budget[0] != budget
        || report.outcome != outcome
    {
        return Err(bad(
            "final operator/frame, canonical budget or accuracy outcome",
        ));
    }
    cancel.poll()?;
    Ok(())
}

/// Check self-contained geometry; a statistical result requires a bound history.
pub fn check(
    r: &TransportRequest,
    report: &TransportAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[r.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(r.memory_bytes, retained, || {
        cancel.poll()?;
        if !report.covariance.is_empty() {
            return Err(bad("covariance promotion in standalone geometry"));
        }
        geometry(r, report, cancel)
    })
}

pub fn check_history(
    obs: &TraceRequest,
    baseline: &TraceReport,
    r: &TransportRequest,
    report: &TransportAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[
        obs.heap_bytes()?,
        baseline.heap_bytes()?,
        r.heap_bytes()?,
        report.heap_bytes()?,
    ])?;
    memory::ensure_operation(obs.memory_bytes, retained, || {
        cancel.poll()?;
        crate::check_with_cancel(obs, baseline, cancel)?;
        admission(r)?;
        if r.dimension != obs.features.len()
            || r.memory_bytes != obs.memory_bytes
            || r.times.first() != obs.times.first()
            || r.times.last() != obs.times.last()
            || obs
                .times
                .iter()
                .any(|time| r.times.binary_search_by(|v| v.total_cmp(time)).is_err())
        {
            return Err(bad(
                "covariance observation mesh, domain, dimension or budget",
            ));
        }
        geometry(r, report, cancel)?;
        if report.outcome != Outcome::Resolved {
            if !report.covariance.is_empty() {
                return Err(bad("covariance claims after unresolved geometry"));
            }
            return Ok(());
        }
        covariance(obs, baseline, r, report, cancel)
    })
}

fn covariance(
    obs: &TraceRequest,
    baseline: &TraceReport,
    r: &TransportRequest,
    report: &TransportAssessment,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        let n = obs.times.len();
        let p = r.dimension;
        let q = r.block[1] - r.block[0];
        if report.covariance.len() != 1 {
            return Err(bad("missing or extra covariance-family proof"));
        }
        let proof = &report.covariance[0];
        if proof.scope != COVARIANCE_SCOPE
            || proof.observation_indices.len() != n
            || proof.distances.len() != n
        {
            return Err(bad("covariance scope or complete observation universe"));
        }
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
        let mut node_gaps = memory::capacity(n * count)?;
        let mut gaps = memory::capacity((n - 1) * count)?;
        let root_q = point(q as f64).sqrt();
        let mut failed_gap = false;
        let mut endpoint = 0f64;
        let mut initial_distance = 0.;
        for j in 0..n {
            cancel.poll()?;
            let index = r
                .times
                .binary_search_by(|time| time.total_cmp(&obs.times[j]))
                .map_err(|_| bad("missing covariance observation node"))?;
            if proof.observation_indices[j] != index {
                return Err(bad("wrong observation-to-geometry index"));
            }
            let bound = memory::scope(|| {
                let (reference, dr) = projector(&r.reference_nodes[index], r.block, cancel)?;
                let (base, db) = projector(&baseline.eigensystems[j].vectors, r.block, cancel)?;
                distance(
                    &reference,
                    dr.projector_error,
                    &base,
                    db.projector_error,
                    &proof.distances[j],
                    cancel,
                )
            })?;
            let e = point(baseline.radii[j]) + point(baseline.eigensystems[j].backward);
            let mut minimum = f64::MAX;
            for &boundary in &boundaries[..count] {
                let values = &baseline.eigensystems[j].values;
                let gap =
                    (point(values[boundary]) - point(values[boundary + 1]) - point(2.) * e).inf();
                node_gaps.push(gap);
                minimum = minimum.min(gap);
                failed_gap |= gap <= (point(2.) * e).sup();
            }
            let node_error = if count == 0 {
                0.
            } else if minimum <= 0. {
                1.
            } else {
                let d = point(baseline.eigensystems[j].polar);
                upper(point(2.) * root_q * e / point(minimum) + (point(2.) + d) * d)?.min(1.)
            };
            let delta = upper(point(node_error) + point(bound))?;
            endpoint = endpoint.max(delta);
            if j == 0 {
                initial_distance = delta;
            }
        }
        let mut minimum_gap = f64::MAX;
        let mut mesh = 0f64;
        for j in 0..n - 1 {
            cancel.poll()?;
            let dt = point(obs.times[j + 1]) - point(obs.times[j]);
            mesh = mesh.max(upper(dt)?);
            for k in 0..count {
                let left = point(node_gaps[j * count + k]);
                let right = point(node_gaps[(j + 1) * count + k]);
                let drift = point(obs.lipschitz) * dt;
                let gap = (left - point(2.) * drift)
                    .inf()
                    .max((right - point(2.) * drift).inf())
                    .max(((left + right) / point(2.) - drift).inf());
                gaps.push(gap);
                minimum_gap = minimum_gap.min(gap);
                failed_gap |= gap <= 0.;
            }
        }
        if proof.node_gaps != node_gaps || proof.gaps != gaps {
            return Err(bad("incomplete covariance nodal/segment gap universe"));
        }
        if failed_gap || initial_distance >= 1. {
            let gate = if failed_gap {
                Gate::CovarianceGap
            } else {
                Gate::CovarianceInitial
            };
            if !proof.budget.is_empty() || proof.outcome != (Outcome::Unresolved { gate }) {
                return Err(bad(
                    "covariance strict gap/initial gate or unsupported budget",
                ));
            }
            return Ok(());
        }
        let speed = if count == 0 {
            0.
        } else {
            upper(point(2.) * root_q * point(obs.lipschitz) / point(minimum_gap))?
        };
        let reference_speed = report.budget[0].reference_speed;
        let duration = point(obs.times[n - 1]) - point(obs.times[0]);
        let initial = upper(point(2.) * point(initial_distance))?;
        let epsilon =
            point(endpoint) + (point(speed) + point(reference_speed)) * point(mesh) / point(2.);
        let extra = upper(
            (point(4.) + point(12.) * point(speed.max(reference_speed)) * duration) * epsilon
                + point(initial),
        )?;
        let total = upper(point(report.budget[0].comparison_error) + point(extra))?;
        let budget = CovarianceBudget {
            projector_speed: speed,
            endpoint_error: endpoint,
            initial_uncertainty: initial,
            mesh,
            comparison_error: extra,
            total_error: total,
        };
        let outcome = if total >= r.target {
            Outcome::Unresolved {
                gate: Gate::CovarianceAccuracy,
            }
        } else {
            Outcome::Resolved
        };
        if proof.budget.as_slice() != [budget] || proof.outcome != outcome {
            return Err(bad(
                "covariance initial uncertainty, observation mesh, budget or outcome",
            ));
        }
        cancel.poll()?;
        Ok(())
    })
}
