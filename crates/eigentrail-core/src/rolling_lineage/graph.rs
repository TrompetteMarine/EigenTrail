//! Dated min-max paths. Coverage and angular decisions belong to the caller.
//!
//! Vertices are in typed-key order; streamed edges are ordered by target subject
//! and date. A date's incoming edges must finish before a later date is visited.
//! This retains two costs and two predecessors per vertex, never all paths.

use super::Lens;
use crate::{
    Error, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Vertex {
    pub subject: usize,
    pub date: usize,
    pub lens: Lens,
    pub rank: [usize; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub source: usize,
    pub target: usize,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteCost {
    NoRoute,
    Bound(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Costs {
    pub lower: RouteCost,
    pub upper: RouteCost,
}

impl Costs {
    const ABSENT: Self = Self {
        lower: RouteCost::NoRoute,
        upper: RouteCost::NoRoute,
    };
    const TERMINAL: Self = Self {
        lower: RouteCost::Bound(0.0),
        upper: RouteCost::Bound(0.0),
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundSide {
    Lower,
    Upper,
}

#[derive(Clone, Copy, Debug)]
struct Record {
    costs: Costs,
    parents: [Option<usize>; 2],
}

/// An attaining witness for one bound; its gaps count original observed slots.
#[derive(Debug, PartialEq)]
pub struct Witness {
    pub cost: f64,
    pub vertices: Vec<usize>,
    pub slot_gaps: Vec<usize>,
}

pub struct Forward<'a> {
    vertices: &'a [Vertex],
    founder: usize,
    lag_cap: usize,
    records: Vec<Record>,
    last_target: Option<(usize, usize)>,
}

impl HeapBytes for Costs {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for Record {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
impl HeapBytes for Witness {
    fn heap_bytes(&self) -> Result<usize> {
        self.vertices
            .heap_bytes()?
            .checked_add(self.slot_gaps.heap_bytes()?)
            .ok_or_else(|| Error::Resource(String::new()))
    }
}
impl HeapBytes for Forward<'_> {
    fn heap_bytes(&self) -> Result<usize> {
        self.records.heap_bytes()
    }
}

fn invalid(message: &str) -> Error {
    match memory::text(message) {
        Ok(message) => Error::Invalid(message),
        Err(error) => error,
    }
}

fn validate_vertices(vertices: &[Vertex], cancel: &Cancellation) -> Result<()> {
    for (index, vertex) in vertices.iter().enumerate() {
        cancel.poll()?;
        if vertex.rank[0] >= vertex.rank[1] || index > 0 && vertices[index - 1] >= *vertex {
            return Err(invalid(
                "rolling vertices require unique typed-key order and nonempty half-open ranks",
            ));
        }
    }
    Ok(())
}

fn validate_edge(vertices: &[Vertex], edge: Edge, lag_cap: usize) -> Result<(usize, usize)> {
    let source = vertices
        .get(edge.source)
        .ok_or_else(|| invalid("rolling edge source is absent"))?;
    let target = vertices
        .get(edge.target)
        .ok_or_else(|| invalid("rolling edge target is absent"))?;
    let gap = target.date.checked_sub(source.date);
    if source.subject != target.subject
        || !gap.is_some_and(|value| value > 0 && value <= lag_cap)
        || !edge.lower.is_finite()
        || !edge.upper.is_finite()
        || !(0.0..=90.0).contains(&edge.lower)
        || !(edge.lower..=90.0).contains(&edge.upper)
    {
        return Err(invalid(
            "rolling edges require one subject, positive bounded lag and ordered finite angles in [0,90]",
        ));
    }
    Ok((target.subject, target.date))
}

fn candidate(prefix: RouteCost, edge: f64) -> RouteCost {
    match prefix {
        RouteCost::NoRoute => RouteCost::NoRoute,
        RouteCost::Bound(cost) => RouteCost::Bound(cost.max(edge)),
    }
}

fn minimum(first: RouteCost, second: RouteCost) -> RouteCost {
    match (first, second) {
        (RouteCost::NoRoute, other) | (other, RouteCost::NoRoute) => other,
        (RouteCost::Bound(a), RouteCost::Bound(b)) => RouteCost::Bound(a.min(b)),
    }
}

impl<'a> Forward<'a> {
    pub fn new(
        vertices: &'a [Vertex],
        founder: usize,
        lag_cap: usize,
        cancel: &Cancellation,
    ) -> Result<Self> {
        validate_vertices(vertices, cancel)?;
        if founder >= vertices.len() {
            return Err(invalid("rolling founder vertex is absent"));
        }
        let mut records = memory::capacity(vertices.len())?;
        records.resize(
            vertices.len(),
            Record {
                costs: Costs::ABSENT,
                parents: [None; 2],
            },
        );
        records[founder].costs = Costs::TERMINAL;
        Ok(Self {
            vertices,
            founder,
            lag_cap,
            records,
            last_target: None,
        })
    }

    /// Supply every eligible edge, before applying the query's angular cutoff.
    /// Prefix costs do not establish complete candidate coverage or exclusion.
    pub fn consider_edge(&mut self, edge: Edge, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let target_key = validate_edge(self.vertices, edge, self.lag_cap)?;
        if self
            .last_target
            .is_some_and(|previous| previous > target_key)
        {
            return Err(invalid("rolling edge stream reordered a target date"));
        }
        self.last_target = Some(target_key);
        let source = self.records[edge.source].costs;
        self.update(
            edge.target,
            edge.source,
            candidate(source.lower, edge.lower),
            0,
        );
        self.update(
            edge.target,
            edge.source,
            candidate(source.upper, edge.upper),
            1,
        );
        Ok(())
    }

    fn update(&mut self, target: usize, source: usize, value: RouteCost, side: usize) {
        let RouteCost::Bound(new) = value else {
            return;
        };
        let record = &mut self.records[target];
        let cost = if side == 0 {
            &mut record.costs.lower
        } else {
            &mut record.costs.upper
        };
        let better = match *cost {
            RouteCost::NoRoute => true,
            RouteCost::Bound(old) => {
                new < old
                    || new == old
                        && record.parents[side]
                            .is_some_and(|prior| self.vertices[source] < self.vertices[prior])
            }
        };
        if better {
            *cost = value;
            record.parents[side] = Some(source);
        }
    }

    pub fn costs(&self, vertex: usize) -> Result<Costs> {
        self.records
            .get(vertex)
            .map(|record| record.costs)
            .ok_or_else(|| invalid("rolling queried vertex is absent"))
    }

    /// Tie-breaking selects the smallest typed incoming predecessor. This is
    /// an attaining deterministic path, not the lexicographically first path.
    pub fn witness(
        &self,
        target: usize,
        side: BoundSide,
        cancel: &Cancellation,
    ) -> Result<Option<Witness>> {
        cancel.poll()?;
        let costs = self.costs(target)?;
        let (cost, side) = match side {
            BoundSide::Lower => (costs.lower, 0),
            BoundSide::Upper => (costs.upper, 1),
        };
        let RouteCost::Bound(cost) = cost else {
            return Ok(None);
        };
        let mut vertices = memory::capacity(1)?;
        let mut slot_gaps = memory::capacity(0)?;
        let mut current = target;
        vertices.push(current);
        while current != self.founder {
            cancel.poll()?;
            let previous = self.records[current].parents[side]
                .ok_or_else(|| invalid("rolling route lost a predecessor"))?;
            let gap = self.vertices[current]
                .date
                .checked_sub(self.vertices[previous].date)
                .filter(|gap| *gap > 0)
                .ok_or_else(|| invalid("rolling predecessor is not earlier"))?;
            if vertices.len() >= self.vertices.len() {
                return Err(invalid("rolling predecessor cycle"));
            }
            memory::push(&mut vertices, previous)?;
            memory::push(&mut slot_gaps, gap)?;
            current = previous;
        }
        vertices.reverse();
        slot_gaps.reverse();
        Ok(Some(Witness {
            cost,
            vertices,
            slot_gaps,
        }))
    }
}

pub fn forward<'a>(
    vertices: &'a [Vertex],
    edges: &[Edge],
    founder: usize,
    lag_cap: usize,
    cancel: &Cancellation,
) -> Result<Forward<'a>> {
    let mut state = Forward::new(vertices, founder, lag_cap, cancel)?;
    for &edge in edges {
        state.consider_edge(edge, cancel)?;
    }
    Ok(state)
}

/// Reverse induction for a fixed terminal target, using the same ordered edges.
pub fn backward(
    vertices: &[Vertex],
    edges: &[Edge],
    target: usize,
    lag_cap: usize,
    cancel: &Cancellation,
) -> Result<Vec<Costs>> {
    validate_vertices(vertices, cancel)?;
    if target >= vertices.len() {
        return Err(invalid("rolling terminal vertex is absent"));
    }
    let mut last = None;
    for &edge in edges {
        cancel.poll()?;
        let key = validate_edge(vertices, edge, lag_cap)?;
        if last.is_some_and(|previous| previous > key) {
            return Err(invalid("rolling edge stream reordered a target date"));
        }
        last = Some(key);
    }
    let mut costs = memory::capacity(vertices.len())?;
    costs.resize(vertices.len(), Costs::ABSENT);
    costs[target] = Costs::TERMINAL;
    for &edge in edges.iter().rev() {
        cancel.poll()?;
        let suffix = costs[edge.target];
        let source = &mut costs[edge.source];
        source.lower = minimum(source.lower, candidate(suffix.lower, edge.lower));
        source.upper = minimum(source.upper, candidate(suffix.upper, edge.upper));
    }
    Ok(costs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(date: usize, lens: Lens) -> Vertex {
        Vertex {
            subject: 0,
            date,
            lens,
            rank: [0, 1],
        }
    }
    fn edge(source: usize, target: usize, lower: f64, upper: f64) -> Edge {
        Edge {
            source,
            target,
            lower,
            upper,
        }
    }
    fn brute(
        edges: &[Edge],
        current: usize,
        target: usize,
        lower: f64,
        upper: f64,
        result: &mut Vec<(f64, f64)>,
    ) {
        if current == target {
            result.push((lower, upper));
        } else {
            for edge in edges.iter().filter(|edge| edge.source == current) {
                brute(
                    edges,
                    edge.target,
                    target,
                    lower.max(edge.lower),
                    upper.max(edge.upper),
                    result,
                );
            }
        }
    }
    fn attained(state: &Forward<'_>, edges: &[Edge], target: usize, side: BoundSide) {
        let witness = state
            .witness(target, side, &Cancellation::default())
            .unwrap();
        let cost = match side {
            BoundSide::Lower => state.costs(target).unwrap().lower,
            BoundSide::Upper => state.costs(target).unwrap().upper,
        };
        match (cost, witness) {
            (RouteCost::NoRoute, None) => (),
            (RouteCost::Bound(value), Some(witness)) => {
                assert_eq!(witness.vertices.first(), Some(&state.founder));
                assert_eq!(witness.vertices.last(), Some(&target));
                let mut actual: f64 = 0.0;
                for pair in witness.vertices.windows(2) {
                    let edge = edges
                        .iter()
                        .find(|edge| edge.source == pair[0] && edge.target == pair[1])
                        .unwrap();
                    actual = actual.max(match side {
                        BoundSide::Lower => edge.lower,
                        BoundSide::Upper => edge.upper,
                    });
                }
                assert_eq!(actual, value);
                assert_eq!(witness.cost, value);
                assert_eq!(witness.slot_gaps.len() + 1, witness.vertices.len());
            }
            other => panic!("route tag/witness mismatch: {other:?}"),
        }
    }

    #[test]
    fn exhaustive_small_dags_match_both_inductions_and_attaining_paths() {
        let vertices: Vec<_> = (0..5).map(|date| vertex(date, Lens::Covariance)).collect();
        let possible: Vec<_> = (1..5)
            .flat_map(|target| {
                (0..target).map(move |source| {
                    let lower = ((source * 7 + target * 3) % 13) as f64;
                    edge(
                        source,
                        target,
                        lower,
                        lower + ((source + target) % 3) as f64,
                    )
                })
            })
            .collect();
        for mask in 0..(1 << possible.len()) {
            let edges: Vec<_> = possible
                .iter()
                .enumerate()
                .filter_map(|(i, edge)| (mask & (1 << i) != 0).then_some(*edge))
                .collect();
            for founder in 0..vertices.len() {
                let state =
                    forward(&vertices, &edges, founder, 5, &Cancellation::default()).unwrap();
                for target in 0..vertices.len() {
                    let mut paths = Vec::new();
                    brute(&edges, founder, target, 0.0, 0.0, &mut paths);
                    let expected = if paths.is_empty() {
                        Costs::ABSENT
                    } else {
                        Costs {
                            lower: RouteCost::Bound(
                                paths.iter().map(|costs| costs.0).reduce(f64::min).unwrap(),
                            ),
                            upper: RouteCost::Bound(
                                paths.iter().map(|costs| costs.1).reduce(f64::min).unwrap(),
                            ),
                        }
                    };
                    assert_eq!(
                        state.costs(target).unwrap(),
                        expected,
                        "mask={mask} founder={founder} target={target}"
                    );
                    let reverse =
                        backward(&vertices, &edges, target, 5, &Cancellation::default()).unwrap();
                    assert_eq!(reverse[founder], expected);
                    attained(&state, &edges, target, BoundSide::Lower);
                    attained(&state, &edges, target, BoundSide::Upper);
                }
            }
        }
    }

    #[test]
    fn lower_and_upper_routes_differ_and_skip_keeps_original_gap() {
        let vertices = [
            vertex(0, Lens::Covariance),
            vertex(1, Lens::Covariance),
            vertex(2, Lens::Covariance),
        ];
        let adjacent = [edge(0, 1, 0.0, 90.0), edge(1, 2, 0.0, 90.0)];
        let cancel = Cancellation::default();
        let first = forward(&vertices, &adjacent, 0, 1, &cancel).unwrap();
        assert_eq!(
            first.costs(2).unwrap(),
            Costs {
                lower: RouteCost::Bound(0.0),
                upper: RouteCost::Bound(90.0)
            }
        );
        let skipped = [adjacent[0], adjacent[1], edge(0, 2, 10.0, 20.0)];
        let second = forward(&vertices, &skipped, 0, 2, &cancel).unwrap();
        assert_eq!(
            second.costs(2).unwrap(),
            Costs {
                lower: RouteCost::Bound(0.0),
                upper: RouteCost::Bound(20.0)
            }
        );
        assert_eq!(
            second
                .witness(2, BoundSide::Lower, &cancel)
                .unwrap()
                .unwrap()
                .vertices,
            vec![0, 1, 2]
        );
        let witness = second
            .witness(2, BoundSide::Upper, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(witness.vertices, vec![0, 2]);
        assert_eq!(witness.slot_gaps, vec![2]);
        assert!(forward(&vertices, &skipped, 0, 1, &cancel).is_err());
    }

    #[test]
    fn distinct_middle_lenses_do_not_compose() {
        let vertices = [
            vertex(0, Lens::Covariance),
            vertex(1, Lens::Covariance),
            vertex(1, Lens::Correlation),
            vertex(2, Lens::Covariance),
        ];
        let state = forward(
            &vertices,
            &[edge(0, 1, 0.0, 1.0), edge(2, 3, 0.0, 1.0)],
            0,
            1,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(state.costs(3).unwrap(), Costs::ABSENT);
        assert_eq!(
            state
                .witness(3, BoundSide::Upper, &Cancellation::default())
                .unwrap(),
            None
        );
    }

    #[test]
    fn typed_predecessor_ties_are_independent_of_incoming_order() {
        let vertices = [
            vertex(0, Lens::Covariance),
            vertex(1, Lens::Covariance),
            vertex(1, Lens::Precision),
            vertex(2, Lens::Covariance),
        ];
        let edges = [
            edge(0, 2, 2.0, 3.0),
            edge(0, 1, 2.0, 3.0),
            edge(2, 3, 2.0, 3.0),
            edge(1, 3, 2.0, 3.0),
        ];
        let cancel = Cancellation::default();
        let state = forward(&vertices, &edges, 0, 2, &cancel).unwrap();
        assert_eq!(
            state
                .witness(3, BoundSide::Upper, &cancel)
                .unwrap()
                .unwrap()
                .vertices,
            vec![0, 1, 3]
        );
    }

    #[test]
    fn attaining_predecessor_witness_need_not_be_global_lexicographic_minimum() {
        let vertices = [
            vertex(0, Lens::Covariance),
            vertex(1, Lens::Covariance),
            vertex(1, Lens::Precision),
            vertex(2, Lens::Covariance),
            vertex(3, Lens::Covariance),
        ];
        let edges = [
            edge(0, 1, 5.0, 5.0),
            edge(0, 2, 1.0, 1.0),
            edge(1, 3, 5.0, 5.0),
            edge(2, 3, 1.0, 1.0),
            edge(3, 4, 10.0, 10.0),
        ];
        let cancel = Cancellation::default();
        let state = forward(&vertices, &edges, 0, 2, &cancel).unwrap();
        let witness = state
            .witness(4, BoundSide::Upper, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(witness.cost, 10.0);
        assert_eq!(witness.vertices, vec![0, 2, 3, 4]);
        let lexicographically_smaller_attaining_path = vec![0, 1, 3, 4];
        assert!(lexicographically_smaller_attaining_path < witness.vertices);
        assert_eq!(5.0_f64.max(5.0).max(10.0), witness.cost);
        attained(&state, &edges, 4, BoundSide::Upper);
    }

    #[test]
    fn invalid_graph_inputs_cancellation_and_allocation_refuse() {
        let vertices = [
            vertex(0, Lens::Covariance),
            vertex(1, Lens::Covariance),
            vertex(2, Lens::Covariance),
        ];
        let cancel = Cancellation::default();
        let invalid = [
            edge(3, 1, 0.0, 1.0),
            edge(0, 3, 0.0, 1.0),
            edge(1, 0, 0.0, 1.0),
            edge(1, 1, 0.0, 1.0),
            edge(0, 2, 0.0, 1.0),
            edge(0, 1, -1.0, 1.0),
            edge(0, 1, 2.0, 1.0),
            edge(0, 1, 0.0, f64::INFINITY),
            edge(0, 1, f64::NAN, 1.0),
        ];
        for bad in invalid {
            assert!(forward(&vertices, &[bad], 0, 1, &cancel).is_err());
            assert!(backward(&vertices, &[bad], 1, 1, &cancel).is_err());
        }
        assert!(
            forward(
                &vertices,
                &[edge(0, 2, 0.0, 1.0), edge(0, 1, 0.0, 1.0)],
                0,
                2,
                &cancel
            )
            .is_err()
        );
        assert!(
            backward(
                &vertices,
                &[edge(0, 2, 0.0, 1.0), edge(0, 1, 0.0, 1.0)],
                2,
                2,
                &cancel
            )
            .is_err()
        );
        assert!(Forward::new(&[vertices[0], vertices[0]], 0, 1, &cancel).is_err());
        assert!(Forward::new(&[vertices[1], vertices[0]], 0, 1, &cancel).is_err());
        let mut invalid_vertex = vertices[0];
        invalid_vertex.rank = [1, 1];
        assert!(Forward::new(&[invalid_vertex], 0, 1, &cancel).is_err());
        let mut other_subject = vertices[1];
        other_subject.subject = 1;
        assert!(
            forward(
                &[vertices[0], other_subject],
                &[edge(0, 1, 0.0, 1.0)],
                0,
                1,
                &cancel
            )
            .is_err()
        );
        assert!(Forward::new(&vertices, 3, 1, &cancel).is_err());
        assert!(backward(&vertices, &[], 3, 1, &cancel).is_err());
        let mut state = Forward::new(&vertices, 0, 1, &cancel).unwrap();
        assert!(state.costs(3).is_err());
        cancel.cancel();
        assert_eq!(
            Forward::new(&vertices, 0, 1, &cancel).err(),
            Some(Error::Cancelled)
        );
        assert_eq!(
            state.consider_edge(edge(0, 1, 0.0, 1.0), &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(
            state.witness(0, BoundSide::Upper, &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(
            backward(&vertices, &[], 0, 1, &cancel),
            Err(Error::Cancelled)
        );
        let operation = memory::Operation::new(8192).unwrap();
        assert!(matches!(
            operation.run(|| Forward::new(&vertices, 0, 1, &Cancellation::default())),
            Err(Error::Resource(_))
        ));
    }
}
