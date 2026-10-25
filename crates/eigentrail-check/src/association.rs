use eigentrail_core::{
    resources::{Cancellation, memory},
    scalar::*,
    *,
};

pub fn reconstruct(
    a: &Eigensystem,
    b: &Eigensystem,
    blocks: &[Tube],
    k: f64,
    dt: f64,
    cancel: &eigentrail_core::resources::Cancellation,
) -> Result<Vec<Vec<usize>>> {
    memory::scope(|| {
        let mut graph = memory::capacity(blocks.len())?;
        for x in blocks {
            cancel.poll()?;
            let mut neighbours = memory::capacity(blocks.len())?;
            for (j, y) in blocks.iter().enumerate() {
                if x.end - x.start != y.end - y.start {
                    continue;
                }
                let p = a.vectors.n;
                if x.start == 0 && x.end == p && y.start == 0 && y.end == p {
                    neighbours.push(j);
                    continue;
                }
                let correction = point(a.polar) + point(b.polar) + point(a.polar) * point(b.polar);
                let q = x.end - x.start;
                let mut maximum_overlap = point(0.);
                for u in x.start..x.end {
                    cancel.poll()?;
                    for v in y.start..y.end {
                        let mut dot = point(0.);
                        for k in 0..p {
                            dot += point(a.vectors.at(k, u)) * point(b.vectors.at(k, v));
                        }
                        maximum_overlap += square(point(magnitude(dot)) + correction);
                    }
                }
                let square_lower = (point(2. * q as f64) - point(2.) * maximum_overlap)
                    .inf()
                    .max(0.);
                let lower = (point(square_lower) / point((2 * q).min(p) as f64))
                    .sqrt()
                    .inf();
                let movement = if x.gap > 0. {
                    (point(2.) * point((x.end - x.start) as f64).sqrt() * point(k) * point(dt)
                        / point(x.gap))
                    .sup()
                    .min(1.)
                } else {
                    1.
                };
                if lower <= (point(movement) + point(x.bound) + point(y.bound)).sup() {
                    neighbours.push(j);
                }
            }
            graph.push(neighbours);
        }
        Ok(graph)
    })
}

// Independent matching algorithm: breadth-first augmenting paths, then alternating-cycle test.
/// Compatibility wrapper; qualified checking uses the fallible variant.
pub fn classify(g: &[Vec<usize>]) -> Association {
    classify_checked(g, &Cancellation::default())
        .expect("fallible classification requires classify_checked")
}
pub fn classify_checked(g: &[Vec<usize>], cancel: &Cancellation) -> Result<Association> {
    memory::scope(|| {
        cancel.poll()?;
        let n = g.len();
        if g.iter().flatten().any(|&j| j >= n) {
            return Err(Error::Invalid(
                "checker: graph index outside candidate universe".into(),
            ));
        }
        let mut left = memory::capacity(n)?;
        left.resize(n, None);
        let mut right = memory::capacity(n)?;
        right.resize(n, None);
        let mut parent = memory::capacity(n)?;
        parent.resize(n, None);
        let mut seen = memory::capacity(n)?;
        seen.resize(n, false);
        let mut queue = memory::capacity(n)?;
        for root in 0..n {
            cancel.poll()?;
            parent.fill(None);
            seen.fill(false);
            queue.clear();
            queue.push(root);
            let mut head = 0;
            let mut free = None;
            'search: while head < queue.len() {
                cancel.poll()?;
                let i = queue[head];
                head += 1;
                for &j in &g[i] {
                    if seen[j] {
                        continue;
                    }
                    seen[j] = true;
                    parent[j] = Some(i);
                    if let Some(next) = right[j] {
                        queue.push(next);
                    } else {
                        free = Some(j);
                        break 'search;
                    }
                }
            }
            let Some(mut j) = free else {
                return Ok(Association::NoPerfectMatching);
            };
            loop {
                let i = parent[j].unwrap();
                let old = left[i];
                left[i] = Some(j);
                right[j] = Some(i);
                if let Some(old) = old {
                    j = old;
                } else {
                    break;
                }
            }
        }
        let square = n
            .checked_mul(n)
            .ok_or_else(|| Error::Resource(String::new()))?;
        let mut reach = memory::capacity(square)?;
        reach.resize(square, false);
        for i in 0..n {
            cancel.poll()?;
            for &j in &g[i] {
                if left[i] != Some(j) {
                    reach[i * n + right[j].unwrap()] = true;
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                cancel.poll()?;
                for j in 0..n {
                    reach[i * n + j] |= reach[i * n + k] && reach[k * n + j];
                }
            }
        }
        if (0..n).any(|i| reach[i * n + i]) {
            Ok(Association::Alternatives)
        } else {
            let mut matching = memory::capacity(n)?;
            for value in left {
                matching.push(value.unwrap());
            }
            Ok(Association::Unique(matching))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_resource_and_cancellation_do_not_become_ambiguity() {
        let graph = vec![vec![0]];
        let cancel = Cancellation::default();
        let baseline = memory::Operation::new(resources::DEFAULT_BYTES)
            .unwrap()
            .stats()
            .live;
        let tight = memory::Operation::new(baseline).unwrap();
        assert!(matches!(
            tight.run(|| classify_checked(&graph, &cancel)),
            Err(Error::Resource(_))
        ));
        cancel.cancel();
        let ordinary = memory::Operation::new(resources::DEFAULT_BYTES).unwrap();
        assert!(matches!(
            ordinary.run(|| classify_checked(&graph, &cancel)),
            Err(Error::Cancelled)
        ));
    }
}
