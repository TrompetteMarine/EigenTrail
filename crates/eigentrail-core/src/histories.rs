use crate::{Error, Result, numerics::*, resources::*, scalar::*, types::*};
#[derive(Debug, PartialEq)]
pub enum Witness {
    Original,
    Widened { additional: f64 },
    Unverified,
}
pub fn witness(
    request: &TraceRequest,
    observed: &[Matrix],
    radii: &[f64],
    nodes: &[Matrix],
    widening: f64,
    cancel: &Cancellation,
) -> Result<Witness> {
    crate::resources::memory::scope(|| {
        nonnegative(widening, "witness widening")?;
        if nodes.len() != observed.len() || radii.len() != nodes.len() {
            return Err(Error::Invalid("whole witness chain required".into()));
        }
        for (j, x) in nodes.iter().enumerate() {
            cancel.poll()?;
            validate_matrix(x)?;
            if x.n != observed[j].n {
                return Err(Error::Invalid("witness dimensions".into()));
            }
            let e = eigensystem(x, cancel)?;
            if !try_verified_psd(x, &e)? {
                return Ok(Witness::Unverified);
            }
            if difference_norm_upper(x, &observed[j], cancel)?
                > (point(radii[j]) + point(widening)).inf()
            {
                return Ok(Witness::Unverified);
            }
            if j > 0
                && difference_norm_upper(x, &nodes[j - 1], cancel)?
                    > (point(request.lipschitz)
                        * (point(request.times[j]) - point(request.times[j - 1])))
                    .inf()
            {
                return Ok(Witness::Unverified);
            }
        }
        Ok(if widening == 0. {
            Witness::Original
        } else {
            Witness::Widened {
                additional: widening,
            }
        })
    })
}
pub fn diagonal_psd(a: &Matrix) -> bool {
    (0..a.n).all(|i| {
        (0..a.n).all(|j| {
            if i == j {
                a.at(i, i) >= 0.
            } else {
                a.at(i, j) == 0.
            }
        })
    })
}
pub fn verified_psd(a: &Matrix, e: &Eigensystem) -> bool {
    try_verified_psd(a, e).expect("fallible PSD check requires try_verified_psd")
}
pub fn try_verified_psd(a: &Matrix, e: &Eigensystem) -> Result<bool> {
    crate::resources::memory::scope(|| {
        if (point(*e.values.last().unwrap()) - point(e.backward)).inf() >= 0. || diagonal_psd(a) {
            return Ok(true);
        }
        let n = a.n;
        let mut schur = memory::capacity(a.data.len())?;
        for &x in &a.data {
            schur.push(point(x));
        }
        for k in 0..n {
            let pivot = schur[k + k * n];
            if pivot.inf() == 0. && pivot.sup() == 0. {
                if (k + 1..n).any(|i| schur[i + k * n].inf() != 0. || schur[i + k * n].sup() != 0.)
                {
                    return Ok(false);
                }
                continue;
            }
            if pivot.inf() <= 0. {
                return Ok(false);
            }
            for j in k + 1..n {
                for i in j..n {
                    let updated = schur[i + j * n] - schur[i + k * n] * schur[j + k * n] / pivot;
                    schur[i + j * n] = updated;
                    schur[j + i * n] = updated;
                }
            }
        }
        Ok(true)
    })
}
