//! Validated short Grassmann geodesics and checked direct-rotation proposals.
use crate::{Error, Result, resources::Cancellation, scalar::*};
use crate::{numerics, resources::zeros, types::*};

fn projector_with_error(
    e: &Eigensystem,
    start: usize,
    end: usize,
    cancel: &Cancellation,
) -> Result<(Matrix, f64)> {
    crate::resources::memory::scope(|| {
        cancel.poll()?;
        let p = e.vectors.n;
        if p == 0 || p > 256 {
            return Err(Error::Resource("transport dimension ceiling 256".into()));
        }
        if start >= end
            || end > p
            || e.vectors.data.len() != p * p
            || e.polar < 0.
            || !e.polar.is_finite()
            || e.vectors.data.iter().any(|x| !x.is_finite())
        {
            return Err(Error::Invalid("transport block/frame".into()));
        }
        let mut data = zeros(p * p)?;
        let mut rounding = point(0.);
        for j in 0..p {
            cancel.poll()?;
            for i in 0..p {
                let exact = (start..end).fold(point(0.), |s, k| {
                    s + point(e.vectors.at(i, k)) * point(e.vectors.at(j, k))
                });
                let value = exact.inf() / 2. + exact.sup() / 2.;
                if !value.is_finite() {
                    return Err(Error::Exhausted(
                        "endpoint projector evaluation overflow".into(),
                    ));
                }
                data[i + j * p] = value;
                rounding += square(exact - point(value));
            }
        }
        let mut gram = point(0.);
        for i in 0..p {
            cancel.poll()?;
            for j in 0..p {
                let value = (0..p).fold(point(0.), |s, k| {
                    s + point(e.vectors.at(k, i)) * point(e.vectors.at(k, j))
                });
                gram += square(value - point(f64::from(i == j)));
            }
        }
        let tau = gram.sqrt();
        if tau.sup() >= 1. {
            return Err(Error::Exhausted("endpoint frame polar gate".into()));
        }
        let polar = (tau / (point(1.) + (point(1.) - tau).sqrt()))
            .sup()
            .max(e.polar);
        let error = (rounding.sqrt() + (point(2.) + point(polar)) * point(polar)).sup();
        Ok((Matrix { n: p, data }, error))
    })
}

/// Proposal only: outward residual checks below establish its numerical meaning.
pub(crate) fn polar_proposal(mid: &[f64], n: usize, cancel: &Cancellation) -> Result<Matrix> {
    crate::resources::memory::scope(|| {
        cancel.poll()?;
        if !(1..=256).contains(&n) || mid.len() != n * n || mid.iter().any(|x| !x.is_finite()) {
            return Err(Error::Invalid(
                "finite square bounded polar proposal required".into(),
            ));
        }
        let scale = upper(
            mid.iter()
                .fold(point(0.), |sum, &x| sum + square(point(x)))
                .sqrt(),
        )?
        .max(1.);
        let mut x = zeros(n * n)?;
        let mut gram = zeros(n * n)?;
        let mut next = zeros(n * n)?;
        for (i, value) in x.iter_mut().enumerate() {
            *value = mid[i] / scale;
        }
        for _ in 0..64 {
            let mut defect = 0.;
            for j in 0..n {
                cancel.poll()?;
                for i in 0..n {
                    let dot = (0..n).fold(0., |sum, k| sum + x[k + i * n] * x[k + j * n]);
                    gram[i + j * n] = dot;
                    defect += (dot - f64::from(i == j)).powi(2);
                }
            }
            if !defect.is_finite() {
                return Err(Error::Exhausted("polar proposal overflow".into()));
            }
            if defect.sqrt() <= 1e-14 {
                return Ok(Matrix { n, data: x });
            }
            for j in 0..n {
                cancel.poll()?;
                for i in 0..n {
                    let dot = (0..n).fold(0., |sum, k| sum + x[i + k * n] * gram[k + j * n]);
                    next[i + j * n] = (3. * x[i + j * n] - dot) / 2.;
                }
            }
            if next.iter().any(|v| !v.is_finite()) {
                return Err(Error::Exhausted("polar proposal overflow".into()));
            }
            std::mem::swap(&mut x, &mut next);
        }
        Err(Error::Exhausted("polar proposal iteration limit 64".into()))
    })
}

/// Checked proposal for the unique direct rotation between exact polar blocks.
/// Eigensystem orthogonality bounds must already have been independently checked.
pub fn direct_rotation(
    a: &Eigensystem,
    b: &Eigensystem,
    start: usize,
    end: usize,
    cancel: &Cancellation,
) -> Result<(Matrix, f64, f64)> {
    crate::resources::memory::scope(|| {
        cancel.poll()?;
        if a.vectors.n != b.vectors.n {
            return Err(Error::Invalid("equal ambient transport dimensions".into()));
        }
        let (p, dp) = projector_with_error(a, start, end, cancel)?;
        let (q, dq) = projector_with_error(b, start, end, cancel)?;
        let n = p.n;
        let mut difference = Matrix {
            n,
            data: zeros(n * n)?,
        };
        for (i, value) in difference.data.iter_mut().enumerate() {
            *value = p.data[i] - q.data[i];
            if !value.is_finite() {
                return Err(Error::Exhausted("projector difference overflow".into()));
            }
        }
        // The subtraction itself is enclosed before spectral evaluation.
        let mut difference_round = point(0.);
        for j in 0..n * n {
            difference_round +=
                square(point(p.data[j]) - point(q.data[j]) - point(difference.data[j]));
        }
        let ev = numerics::eigensystem(&difference, cancel)?;
        let norm = ev.values.iter().fold(0f64, |x, v| x.max(v.abs()));
        let distance =
            (point(norm) + point(ev.backward) + difference_round.sqrt() + point(dp) + point(dq))
                .sup();
        if distance >= 1. {
            return Err(Error::Exhausted(
                "direct rotation cut-locus gate unresolved".into(),
            ));
        }
        let mut s = crate::resources::memory::capacity(n * n)?;
        s.resize(n * n, point(0.));
        let mut mid = zeros(n * n)?;
        for j in 0..n {
            cancel.poll()?;
            for i in 0..n {
                let product = (0..n).fold(point(0.), |sum, k| {
                    sum + point(q.at(i, k)) * point(p.at(k, j))
                });
                let x = point(f64::from(i == j)) - point(p.at(i, j)) - point(q.at(i, j))
                    + point(2.) * product;
                s[i + j * n] = x;
                mid[i + j * n] = (x.inf() + x.sup()) / 2.;
            }
        }
        let rotation = polar_proposal(&mid, n, cancel)?;
        let mut gram = point(0.);
        let mut residual = point(0.);
        for i in 0..n {
            cancel.poll()?;
            for j in 0..n {
                let dot = (0..n).fold(point(0.), |sum, k| {
                    sum + point(rotation.at(k, i)) * point(rotation.at(k, j))
                });
                gram += square(dot - point(f64::from(i == j)));
                residual += square(s[i + j * n] - point(rotation.at(i, j)));
            }
        }
        let tau = gram.sqrt();
        if tau.sup() >= 1. {
            return Err(Error::Exhausted("rotation polar gate".into()));
        }
        let db = tau / (point(1.) + (point(1.) - tau).sqrt());
        let source_error = point(3.) * (point(dp) + point(dq)) + point(2.) * point(dp) * point(dq);
        let sigma = (point(1.) - square(point(distance))).sqrt();
        let local =
            (point(2.) * (residual.sqrt() + source_error + db) / (sigma + point(1.)) + db).sup();
        Ok((rotation, local, asin(point(distance))?.sup()))
    })
}

pub struct BlockTransport {
    pub rotation: Matrix,
    pub speed_upper: f64,
    pub product_error: f64,
    pub comparison_error: f64,
}
#[derive(Clone, Copy)]
pub struct TransportContract {
    pub reference_speed: f64,
    pub max_speed: f64,
    pub endpoint_error: f64,
    pub initial_mismatch: f64,
    pub target: f64,
}
pub fn transport_blocks(
    times: &[f64],
    nodes: &[Eigensystem],
    block: [usize; 2],
    contract: &TransportContract,
    cancel: &Cancellation,
) -> Result<BlockTransport> {
    use crate::resources::memory::{self, HeapBytes};
    cancel.poll()?;
    if times.len() > 65537 {
        return Err(Error::Exhausted(
            "transport refinement limit 65536 cells".into(),
        ));
    }
    if times.len() != nodes.len() || times.len() < 2 {
        return Err(Error::Invalid("bounded ordered transport mesh".into()));
    }
    let borrowed = std::mem::size_of_val(times)
        .checked_add(std::mem::size_of_val(nodes))
        .ok_or_else(|| Error::Resource("transport allocation overflow".into()))?;
    let borrowed = nodes.iter().try_fold(borrowed, |total, node| {
        cancel.poll()?;
        total
            .checked_add(node.heap_bytes()?)
            .ok_or_else(|| Error::Resource("transport allocation overflow".into()))
    })?;
    let limit = memory::active_limit().unwrap_or(crate::resources::DEFAULT_BYTES);
    crate::resources::memory::ensure_operation(limit, borrowed, || {
        cancel.poll()?;
        let [start, end] = block;
        let TransportContract {
            reference_speed,
            max_speed,
            endpoint_error,
            initial_mismatch,
            target,
        } = *contract;
        if times.iter().any(|x| !x.is_finite()) || times.windows(2).any(|v| v[0] >= v[1]) {
            return Err(Error::Invalid("bounded ordered transport mesh".into()));
        }
        for (x, n) in [
            (reference_speed, "reference speed"),
            (max_speed, "Q speed"),
            (endpoint_error, "node error"),
            (initial_mismatch, "initial mismatch"),
            (target, "target"),
        ] {
            nonnegative(x, n)?;
        }
        let n = nodes[0].vectors.n;
        if n == 0 || n > 256 {
            return Err(Error::Resource("transport dimension ceiling 256".into()));
        }
        let estimate = nodes
            .len()
            .checked_mul(n * n * 8 + n * 8 + 128)
            .and_then(|v| v.checked_add(n * n * 8 * 64 + 4 * 1024 * 1024))
            .ok_or_else(|| Error::Resource("transport allocation overflow".into()))?;
        if estimate > limit {
            return Err(Error::Resource(
                "transport nodes and scratch exceed managed limit before evaluation".into(),
            ));
        }
        let mut product = Matrix {
            n,
            data: zeros(n * n)?,
        };
        let mut next = zeros(n * n)?;
        for i in 0..n {
            product.data[i + i * n] = 1.;
        }
        let mut error = point(0.);
        let mut speed = 0f64;
        let mut mesh = 0f64;
        for j in 0..nodes.len() - 1 {
            cancel.poll()?;
            let dt = point(times[j + 1]) - point(times[j]);
            mesh = mesh.max(dt.sup());
            let (local, angle, round) = memory::scope(|| {
                let (rotation, local, angle) =
                    direct_rotation(&nodes[j], &nodes[j + 1], start, end, cancel)?;
                let mut round = point(0.);
                for c in 0..n {
                    cancel.poll()?;
                    for r in 0..n {
                        let exact = (0..n).fold(point(0.), |sum, k| {
                            sum + point(rotation.at(r, k)) * point(product.at(k, c))
                        });
                        let value = (exact.inf() + exact.sup()) / 2.;
                        next[r + c * n] = value;
                        round += square(exact - point(value));
                    }
                }
                Ok((local, angle, round))
            })?;
            speed = speed.max((point(angle) / dt).sup());
            if speed > max_speed {
                return Err(Error::Exhausted("verified Q speed exceeds contract".into()));
            }
            error = error + point(local) * (point(1.) + error) + round.sqrt();
            if error.sup() > 0.001 {
                return Err(Error::Exhausted("accumulated product budget".into()));
            }
            std::mem::swap(&mut product.data, &mut next);
        }
        let duration = point(times[times.len() - 1]) - point(times[0]);
        let epsilon = point(endpoint_error)
            + (point(reference_speed) + point(speed)) * point(mesh) / point(2.);
        let total = (point(4.) + point(12.) * point(reference_speed.max(speed)) * duration)
            * epsilon
            + point(initial_mismatch)
            + error;
        if total.sup() >= target {
            return Err(Error::Exhausted("transport target unresolved".into()));
        }
        Ok(BlockTransport {
            rotation: product,
            speed_upper: speed,
            product_error: error.sup(),
            comparison_error: total.sup(),
        })
    })
}

#[derive(Debug)]
pub struct LineTransport {
    pub rotation: [[f64; 2]; 2],
    pub speed_upper: f64,
    pub product_error: f64,
    pub comparison_error: f64,
}

pub struct InitializedLine {
    pub compatible_vector: [f64; 2],
    pub mismatch_upper: f64,
}
pub fn initialize_line(
    node: [f64; 2],
    initial: [f64; 2],
    allowed_mismatch: f64,
) -> Result<InitializedLine> {
    nonnegative(allowed_mismatch, "initial mismatch allowance")?;
    if node.iter().chain(initial.iter()).any(|x| !x.is_finite()) {
        return Err(Error::Invalid("finite initialization".into()));
    }
    let norm = (square(point(node[0])) + square(point(node[1]))).sqrt();
    if norm.inf() <= 0. {
        return Err(Error::Invalid("nonzero line frame".into()));
    }
    let dot = point(node[0]) * point(initial[0]) + point(node[1]) * point(initial[1]);
    let sign = if dot.inf() > 0. {
        1.
    } else if dot.sup() < 0. {
        -1.
    } else {
        return Err(Error::Exhausted(
            "incompatible or unresolved initial fibre".into(),
        ));
    };
    let compatible = [
        point(sign) * point(node[0]) / norm,
        point(sign) * point(node[1]) / norm,
    ];
    let error = (square(compatible[0] - point(initial[0]))
        + square(compatible[1] - point(initial[1])))
    .sqrt()
    .sup();
    if error > allowed_mismatch {
        return Err(Error::Exhausted("initial mismatch exceeds contract".into()));
    }
    Ok(InitializedLine {
        compatible_vector: compatible.map(|x| (x.inf() + x.sup()) / 2.),
        mismatch_upper: error,
    })
}

/// Nodes represent exact polar-normalized lines. The caller must independently
/// justify the reference-path speed, endpoint error and compatible initialization.
pub fn transport_lines(
    times: &[f64],
    nodes: &[[f64; 2]],
    contract: &TransportContract,
    cancel: &Cancellation,
) -> Result<LineTransport> {
    cancel.poll()?;
    if times.len() > 65537 {
        return Err(Error::Exhausted(
            "transport refinement limit 65536 cells".into(),
        ));
    }
    let TransportContract {
        reference_speed,
        max_speed,
        endpoint_error,
        initial_mismatch,
        target,
    } = *contract;
    if times.len() != nodes.len()
        || times.len() < 2
        || times.len() > 65537
        || times.iter().any(|x| !x.is_finite())
        || times.windows(2).any(|x| x[0] >= x[1])
        || nodes.iter().flatten().any(|x| !x.is_finite())
    {
        return Err(Error::Invalid(
            "finite transport nodes on an ordered mesh, at most 65536 cells".into(),
        ));
    }
    for (v, n) in [
        (reference_speed, "reference speed"),
        (max_speed, "reconstructed speed"),
        (endpoint_error, "endpoint projector error"),
        (initial_mismatch, "initial mismatch"),
        (target, "target"),
    ] {
        nonnegative(v, n)?;
    }
    let mut product = [[1., 0.], [0., 1.]];
    let mut error = point(0.);
    let mut speed = 0f64;
    let mut h = 0f64;
    for j in 0..nodes.len() - 1 {
        cancel.poll()?;
        let dt = point(times[j + 1]) - point(times[j]);
        h = h.max(dt.sup());
        let [x, y] = nodes[j].map(point);
        let [u, v] = nodes[j + 1].map(point);
        let norm = (square(x) + square(y)).sqrt() * (square(u) + square(v)).sqrt();
        if norm.inf() <= 0. {
            return Err(Error::Invalid("zero frame cannot represent a line".into()));
        }
        let dot = (x * u + y * v) / norm;
        let cross = (x * v - y * u) / norm;
        let sign = if dot.inf() > 0. {
            1.
        } else if dot.sup() < 0. {
            -1.
        } else {
            return Err(Error::Exhausted(
                "principal-angle/cut-locus gate unresolved".into(),
            ));
        };
        let c = dot * point(sign);
        let s = cross * point(sign);
        let cosine = interval(c.inf().max(0.), c.sup().min(1.))?;
        let angle = acos(cosine)?;
        if angle.sup() >= (Interval::PI / point(2.)).inf() {
            return Err(Error::Exhausted("strict principal angle failed".into()));
        }
        speed = speed.max((angle / dt).sup());
        if speed > max_speed {
            return Err(Error::Exhausted(
                "actual reconstructed geodesic speed exceeds bound".into(),
            ));
        }
        let rot = [[c, -s], [s, c]];
        let mut next = [[0.; 2]; 2];
        let mut local = point(0.);
        for r in 0..2 {
            for col in 0..2 {
                let exact = rot[r][0] * point(product[0][col]) + rot[r][1] * point(product[1][col]);
                let approx = (exact.inf() + exact.sup()) / 2.;
                next[r][col] = approx;
                local += square(exact - point(approx));
            }
        }
        error += local.sqrt();
        product = next;
        if error.sup() > 0.001 {
            return Err(Error::Exhausted(
                "rotation/product error budget exhausted".into(),
            ));
        }
    }
    let common = reference_speed.max(speed);
    let duration = point(times[times.len() - 1]) - point(times[0]);
    let epsilon =
        point(endpoint_error) + (point(reference_speed) + point(speed)) * point(h) / point(2.);
    let total = (point(4.) + point(12.) * point(common) * duration) * epsilon
        + point(initial_mismatch)
        + error;
    if total.sup() >= target {
        return Err(Error::Exhausted(
            "transport accuracy target unresolved".into(),
        ));
    }
    Ok(LineTransport {
        rotation: product,
        speed_upper: speed,
        product_error: error.sup(),
        comparison_error: total.sup(),
    })
}

#[cfg(test)]
mod bounded_polar_tests {
    use super::*;
    #[test]
    fn proposal_exhaustion_invalid_input_and_cancellation() {
        let cancel = Cancellation::default();
        assert!(matches!(
            polar_proposal(&[0.; 4], 2, &cancel),
            Err(Error::Exhausted(_))
        ));
        assert!(matches!(
            polar_proposal(&[f64::INFINITY; 4], 2, &cancel),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            polar_proposal(&[f64::MAX; 4], 2, &cancel),
            Err(Error::Exhausted(_))
        ));
        assert!(matches!(
            polar_proposal(&[1.; 3], 2, &cancel),
            Err(Error::Invalid(_))
        ));
        cancel.cancel();
        assert_eq!(
            polar_proposal(&[1., 0., 0., 1.], 2, &cancel),
            Err(Error::Cancelled)
        );
    }
}
