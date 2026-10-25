use crate::{Result, numerics, resources::memory, scalar::*, types::*};

fn augment(
    i: usize,
    edges: &[Vec<usize>],
    seen: &mut [bool],
    owner: &mut [Option<usize>],
    omit: Option<(usize, usize)>,
) -> bool {
    for &j in &edges[i] {
        if j >= owner.len() || seen[j] || omit == Some((i, j)) {
            continue;
        }
        seen[j] = true;
        if owner[j].is_none() || augment(owner[j].unwrap(), edges, seen, owner, omit) {
            owner[j] = Some(i);
            return true;
        }
    }
    false
}
fn matching(edges: &[Vec<usize>], omit: Option<(usize, usize)>) -> Result<Option<Vec<usize>>> {
    memory::scope(|| {
        let n = edges.len();
        let mut owner = memory::capacity(n)?;
        owner.resize(n, None);
        let mut seen = memory::capacity(n)?;
        seen.resize(n, false);
        for i in 0..n {
            seen.fill(false);
            if !augment(i, edges, &mut seen, &mut owner, omit) {
                return Ok(None);
            }
        }
        let mut m = memory::capacity(n)?;
        m.resize(n, 0);
        for (j, i) in owner.into_iter().enumerate() {
            let Some(i) = i else { return Ok(None) };
            m[i] = j;
        }
        Ok(Some(m))
    })
}
pub fn classify(edges: &[Vec<usize>]) -> Association {
    classify_checked(edges).expect("fallible classification requires classify_checked")
}
pub fn classify_checked(edges: &[Vec<usize>]) -> Result<Association> {
    memory::scope(|| {
        let Some(m) = matching(edges, None)? else {
            return Ok(Association::NoPerfectMatching);
        };
        for (i, &j) in m.iter().enumerate() {
            if matching(edges, Some((i, j)))?.is_some() {
                return Ok(Association::Alternatives);
            }
        }
        Ok(Association::Unique(m))
    })
}
pub fn graph(
    a: &Eigensystem,
    b: &Eigensystem,
    tubes: &[Tube],
    k: f64,
    dt: f64,
    cancel: &crate::resources::Cancellation,
) -> Result<Vec<Vec<usize>>> {
    crate::resources::memory::scope(|| {
        let mut out = memory::capacity(tubes.len())?;
        for _ in tubes {
            out.push(memory::capacity(tubes.len())?);
        }
        for (i, x) in tubes.iter().enumerate() {
            cancel.poll()?;
            for (j, y) in tubes.iter().enumerate() {
                if x.end - x.start != y.end - y.start {
                    continue;
                }
                let n = a.vectors.n;
                let rank = x.end - x.start;
                if x.start == 0 && x.end == n && y.start == 0 && y.end == n {
                    out[i].push(j);
                    continue;
                }
                let correction = point(a.polar) + point(b.polar) + point(a.polar) * point(b.polar);
                let mut overlap_upper = point(0.);
                for u in x.start..x.end {
                    cancel.poll()?;
                    for v in y.start..y.end {
                        let dot = (0..n).fold(point(0.), |s, k| {
                            s + point(a.vectors.at(k, u)) * point(b.vectors.at(k, v))
                        });
                        overlap_upper += square(point(magnitude(dot)) + correction);
                    }
                }
                // ||P-Q||_F²=2q-2||U^T V||_F² and rank(P-Q)<=min(2q,n).
                let squared_lower = (point(2. * rank as f64) - point(2.) * overlap_upper)
                    .inf()
                    .max(0.);
                let lower = (point(squared_lower) / point((2 * rank).min(n) as f64))
                    .sqrt()
                    .inf();
                let motion = if x.gap > 0. {
                    upper(
                        point(2.) * point((x.end - x.start) as f64).sqrt() * point(k) * point(dt)
                            / point(x.gap),
                    )?
                    .min(1.)
                } else {
                    1.
                };
                let threshold = upper(point(motion) + point(x.bound) + point(y.bound))?;
                if lower <= threshold {
                    out[i].push(j);
                }
            }
        }
        Ok(out)
    })
}

pub fn displacement(
    a: &Eigensystem,
    b: &Eigensystem,
    first: usize,
    end: usize,
    ea: f64,
    eb: f64,
) -> Result<Range> {
    let (lo, hi) = numerics::distance_bounds(a, b, first, end)?;
    Ok(Range::closed(
        (point(lo) - point(ea) - point(eb)).inf().max(0.),
        (point(hi) + point(ea) + point(eb)).sup().min(1.),
    ))
}
