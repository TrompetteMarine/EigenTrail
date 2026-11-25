//! Independently accumulate founder min-max evidence from checked candidate edges.
//! Candidate coverage and numerical certificates are reconstructed by the caller.
use eigentrail_core::{
    Error, Result,
    resources::{Cancellation, memory},
    rolling_lineage::{FounderPolicy, Lens, Request, Slot, report::*},
};

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn require(test: bool, message: &str) -> Result<()> {
    if test { Ok(()) } else { Err(invalid(message)) }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key(usize, usize, Lens, [usize; 2]);
#[derive(Clone, Copy)]
struct Node {
    key: Key,
    eligible: bool,
}
impl Node {
    fn line(self) -> bool {
        self.key.3[1] - self.key.3[0] == 1
    }
}
#[derive(Clone, Copy, Default)]
struct Choice {
    cost: Option<f64>,
    predecessor: Option<usize>,
}
struct Cell {
    choices: [[Choice; 2]; 3],
    direct: Option<DirectResult>,
}
#[derive(Clone, Copy)]
enum Start {
    Pending,
    Exact(usize),
    Internal(usize),
    Masked,
}
struct Query {
    enabled: [bool; 3],
    start: Start,
    offset: Option<usize>,
    cells: Vec<Cell>,
}

pub struct Reach<'a> {
    request: &'a Request,
    nodes: Vec<Node>,
    queries: Vec<Query>,
    last_view: Option<(usize, usize, Lens)>,
    last_edge_date: Option<(usize, usize)>,
    graph_ordinal: usize,
    direct_ordinal: usize,
    travel: [[bool; 4]; 4],
}

fn policies(policies: &[FounderPolicy]) -> [bool; 3] {
    [
        policies.iter().any(|p| {
            matches!(
                p,
                FounderPolicy::ResolvedLine | FounderPolicy::OuterResolved
            )
        }),
        policies.contains(&FounderPolicy::TypedBlockGraph),
        policies.contains(&FounderPolicy::OuterRelaxed),
    ]
}
fn family(index: usize) -> BottleneckFamily {
    [
        BottleneckFamily::LineOnly,
        BottleneckFamily::TypedBlockGraph,
        BottleneckFamily::RelaxedInternalLines,
    ][index]
}
fn bounds(enclosure: &Enclosure) -> Result<Option<[f64; 2]>> {
    match enclosure {
        Enclosure::Bounds { range } => {
            require(
                range.iter().all(|x| x.is_finite())
                    && 0. <= range[0]
                    && range[0] <= range[1]
                    && range[1] <= 90.,
                "rolling reach: invalid checked angle",
            )?;
            Ok(Some(*range))
        }
        Enclosure::Undefined { .. } | Enclosure::Unbounded { .. } => Ok(None),
    }
}
fn bottleneck(value: Option<f64>) -> Bottleneck {
    match value {
        Some(degrees) => Bottleneck::Bound { degrees },
        None => Bottleneck::NoRoute,
    }
}
fn same_bound(actual: &Bottleneck, expected: &Bottleneck) -> bool {
    actual == expected
        && match (actual, expected) {
            (Bottleneck::Bound { degrees: a }, Bottleneck::Bound { degrees: b }) => {
                a.to_bits() == b.to_bits()
            }
            _ => true,
        }
}
fn same_enclosure(actual: &Enclosure, expected: &Enclosure) -> bool {
    actual == expected
        && match (actual, expected) {
            (Enclosure::Bounds { range: a }, Enclosure::Bounds { range: b }) => {
                a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
            }
            _ => true,
        }
}
fn same_direct(actual: &DirectResult, expected: &DirectResult) -> bool {
    actual == expected
        && match (actual, expected) {
            (
                DirectResult::Available {
                    point: a,
                    geometry: b,
                    method_bracket: c,
                    ..
                },
                DirectResult::Available {
                    point: x,
                    geometry: y,
                    method_bracket: z,
                    ..
                },
            ) => same_enclosure(a, x) && same_enclosure(b, y) && same_enclosure(c, z),
            _ => true,
        }
}
fn parent(choice: Choice) -> Result<Parent> {
    match (choice.cost, choice.predecessor) {
        (None, None) => Ok(Parent::NoRoute),
        (Some(_), Some(node)) => Ok(Parent::Node { node }),
        _ => Err(invalid("rolling reach: inconsistent target predecessor")),
    }
}

impl<'a> Reach<'a> {
    pub fn new(request: &'a Request, cancel: &Cancellation) -> Result<Self> {
        cancel.poll()?;
        let mut queries = memory::capacity(request.founders.len())?;
        for founder in &request.founders {
            memory::push(
                &mut queries,
                Query {
                    enabled: policies(&founder.policies),
                    start: Start::Pending,
                    offset: None,
                    cells: memory::capacity(0)?,
                },
            )?;
        }
        let mut travel = [[false; 4]; 4];
        for &[source, target] in &request.scope.lens_travel {
            travel[source.index()][target.index()] = true;
        }
        Ok(Self {
            request,
            nodes: memory::capacity(0)?,
            queries,
            last_view: None,
            last_edge_date: None,
            graph_ordinal: 0,
            direct_ordinal: 0,
            travel,
        })
    }

    pub fn register_view(&mut self, view: &ViewGroups, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let key = (view.subject, view.date, view.lens);
        require(
            view.subject < self.request.subjects.len()
                && view.date < self.request.dates.len()
                && self.request.lenses.contains(&view.lens)
                && self.last_view.is_none_or(|old| old < key),
            "rolling reach: view scope or order",
        )?;
        self.last_view = Some(key);
        let mut previous_end = 0;
        for group in &view.groups {
            cancel.poll()?;
            require(
                group.node == self.nodes.len()
                    && group.rank[0] < group.rank[1]
                    && group.rank[1] <= self.request.features.len()
                    && group.rank[0] >= previous_end,
                "rolling reach: node order or ranks",
            )?;
            previous_end = group.rank[1];
            let node = Node {
                key: Key(view.subject, view.date, view.lens, group.rank),
                eligible: group.eligible,
            };
            require(
                self.nodes.last().is_none_or(|old| old.key < node.key),
                "rolling reach: typed node order",
            )?;
            memory::push(&mut self.nodes, node)?;
            for (index, query) in self.queries.iter_mut().enumerate() {
                let founder = &self.request.founders[index];
                if founder.subject != view.subject {
                    continue;
                }
                let offset = *query.offset.get_or_insert(group.node);
                require(
                    group.node - offset == query.cells.len(),
                    "rolling reach: noncontiguous subject nodes",
                )?;
                let mut cell = Cell {
                    choices: [[Choice::default(); 2]; 3],
                    direct: None,
                };
                if founder.date == view.date
                    && founder.lens == view.lens
                    && group.rank[0] <= founder.rank[0]
                    && founder.rank[1] <= group.rank[1]
                {
                    require(
                        matches!(query.start, Start::Pending),
                        "rolling reach: duplicate founder origin",
                    )?;
                    query.start = if !group.eligible {
                        Start::Masked
                    } else if group.rank == founder.rank {
                        Start::Exact(group.node)
                    } else {
                        Start::Internal(group.node)
                    };
                    if group.eligible {
                        for (f, enabled) in query.enabled.iter().enumerate() {
                            if !enabled {
                                continue;
                            }
                            let exact = group.rank == founder.rank && (f != 0 || node.line());
                            let relaxed = f == 2 && group.rank != founder.rank;
                            if exact || relaxed {
                                cell.choices[f][0].cost = Some(0.);
                            }
                            if exact {
                                cell.choices[f][1].cost = Some(0.);
                            }
                        }
                    }
                }
                memory::push(&mut query.cells, cell)?;
            }
        }
        Ok(())
    }

    fn pair(&self, edge: &EdgeRecord) -> Result<(Node, Node)> {
        let source = *self
            .nodes
            .get(edge.source)
            .ok_or_else(|| invalid("rolling reach: absent source"))?;
        let target = *self
            .nodes
            .get(edge.target)
            .ok_or_else(|| invalid("rolling reach: absent target"))?;
        require(
            source.key.0 == target.key.0
                && source.key.1 < target.key.1
                && source.eligible
                && target.eligible
                && self.travel[source.key.2.index()][target.key.2.index()],
            "rolling reach: ineligible candidate",
        )?;
        Ok((source, target))
    }

    pub fn edge(&mut self, edge: &EdgeRecord, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let (source, target) = self.pair(edge)?;
        let key = (target.key.0, target.key.1);
        require(
            edge.ordinal == self.graph_ordinal
                && target.key.1 - source.key.1 <= self.request.scope.max_lag
                && self.last_edge_date.is_none_or(|old| old <= key),
            "rolling reach: graph ordinal, lag or topological order",
        )?;
        self.graph_ordinal = self
            .graph_ordinal
            .checked_add(1)
            .ok_or_else(|| Error::Resource(String::new()))?;
        self.last_edge_date = Some(key);
        let method = bounds(&edge.method_bracket)?;
        let geometry = bounds(&edge.geometry)?;
        for (index, query) in self.queries.iter_mut().enumerate() {
            if self.request.founders[index].subject != source.key.0 {
                continue;
            }
            let offset = query
                .offset
                .ok_or_else(|| invalid("rolling reach: absent subject offset"))?;
            for f in 0..3 {
                if !query.enabled[f] || f == 0 && (!source.line() || !target.line()) {
                    continue;
                }
                // A block does not identify the same internal line on its two incident edges.
                let costs = if f == 2 && (!source.line() || !target.line()) {
                    [Some(geometry.map_or(0., |x| x[0])), None]
                } else {
                    method.map_or([Some(0.), None], |x| [Some(x[0]), Some(x[1])])
                };
                let prior = query.cells[edge.source - offset].choices[f];
                for side in 0..2 {
                    let (Some(prefix), Some(cost)) = (prior[side].cost, costs[side]) else {
                        continue;
                    };
                    let candidate = prefix.max(cost);
                    let best = &mut query.cells[edge.target - offset].choices[f][side];
                    let improves = best.cost.is_none_or(|old| {
                        candidate < old
                            || candidate == old
                                && best
                                    .predecessor
                                    .is_some_and(|p| source.key < self.nodes[p].key)
                    });
                    if improves {
                        *best = Choice {
                            cost: Some(candidate),
                            predecessor: Some(edge.source),
                        };
                    }
                }
            }
        }
        Ok(())
    }

    pub fn direct(&mut self, query: usize, edge: &EdgeRecord, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let (source, target) = self.pair(edge)?;
        let founder = self
            .request
            .founders
            .get(query)
            .ok_or_else(|| invalid("rolling reach: direct query"))?;
        let state = self
            .queries
            .get_mut(query)
            .ok_or_else(|| invalid("rolling reach: direct query state"))?;
        require(
            matches!(state.start, Start::Exact(node) if node == edge.source)
                && source.key.0 == founder.subject
                && target.key.1 > founder.date
                && edge.ordinal == self.direct_ordinal,
            "rolling reach: direct origin or ordinal",
        )?;
        bounds(&edge.point)?;
        bounds(&edge.geometry)?;
        bounds(&edge.method_bracket)?;
        let offset = state
            .offset
            .ok_or_else(|| invalid("rolling reach: direct subject offset"))?;
        let cell = state
            .cells
            .get_mut(
                edge.target
                    .checked_sub(offset)
                    .ok_or_else(|| invalid("rolling reach: direct subject"))?,
            )
            .ok_or_else(|| invalid("rolling reach: direct target"))?;
        require(
            cell.direct.is_none(),
            "rolling reach: duplicate direct target",
        )?;
        cell.direct = Some(DirectResult::Available {
            ordinal: edge.ordinal,
            point: edge.point.clone(),
            geometry: edge.geometry.clone(),
            method_bracket: edge.method_bracket.clone(),
            verdict: edge.verdict,
        });
        self.direct_ordinal = self
            .direct_ordinal
            .checked_add(1)
            .ok_or_else(|| Error::Resource(String::new()))?;
        Ok(())
    }

    fn origin(&self, query: usize) -> FounderOrigin {
        match self.queries[query].start {
            Start::Exact(node) => FounderOrigin::Node { node },
            Start::Internal(group) => FounderOrigin::InternalLine { group },
            Start::Masked => FounderOrigin::Unavailable {
                reason: Reason::ExcludedRank,
            },
            Start::Pending => {
                let founder = &self.request.founders[query];
                FounderOrigin::Unavailable {
                    reason: if matches!(
                        self.request.subjects[founder.subject].slots[founder.date],
                        Slot::Missing { .. }
                    ) {
                        Reason::MissingDate
                    } else {
                        Reason::OutOfScope
                    },
                }
            }
        }
    }

    fn expected_direct(&self, query: usize, cell: &Cell) -> DirectResult {
        match self.queries[query].start {
            Start::Exact(_) => cell.direct.clone().unwrap_or(DirectResult::Unavailable {
                reason: Reason::OutOfScope,
            }),
            Start::Internal(_) => DirectResult::Unavailable {
                reason: Reason::UnresolvedInternalLine,
            },
            Start::Pending | Start::Masked => DirectResult::Unavailable {
                reason: Reason::SourceUnavailable,
            },
        }
    }

    fn state(&self, node: usize, f: usize, choices: [Choice; 2]) -> Result<FamilyState> {
        let reach = match (choices[0].cost, choices[1].cost) {
            (_, Some(upper)) if upper <= self.request.tolerance_deg => ReachState::Agreed,
            (Some(lower), _) if lower > self.request.tolerance_deg => ReachState::Excluded,
            (Some(_), _) => ReachState::Possible,
            _ => ReachState::Unavailable {
                reason: if f == 0 && !self.nodes[node].line() {
                    Reason::UnresolvedInternalLine
                } else {
                    Reason::NoCandidates
                },
            },
        };
        Ok(FamilyState {
            family: family(f),
            lower: bottleneck(choices[0].cost),
            upper: bottleneck(choices[1].cost),
            lower_parent: parent(choices[0])?,
            upper_parent: parent(choices[1])?,
            reach,
        })
    }

    fn path(
        &self,
        query: usize,
        target: usize,
        f: usize,
        side: usize,
        reported: &SelectedPath,
        cancel: &Cancellation,
    ) -> Result<()> {
        let state = &self.queries[query];
        let offset = state
            .offset
            .ok_or_else(|| invalid("rolling reach: path subject offset"))?;
        let cost = state.cells[target - offset].choices[f][side]
            .cost
            .ok_or_else(|| invalid("rolling reach: absent selected route"))?;
        require(
            reported.query == query
                && reported.target == target
                && reported.family == family(f)
                && reported.side
                    == if side == 0 {
                        PathSide::Lower
                    } else {
                        PathSide::Upper
                    }
                && reported.cost.to_bits() == cost.to_bits()
                && !reported.nodes.is_empty()
                && reported.slot_gaps.len() == reported.nodes.len() - 1,
            "rolling reach: selected path header",
        )?;
        let mut current = target;
        for position in (0..reported.nodes.len()).rev() {
            cancel.poll()?;
            require(
                reported.nodes[position] == current,
                "rolling reach: altered selected path vertex",
            )?;
            let choice = state.cells[current - offset].choices[f][side];
            if position == 0 {
                let origin = match state.start {
                    Start::Exact(node) | Start::Internal(node) => node,
                    _ => return Err(invalid("rolling reach: path has no origin")),
                };
                require(
                    current == origin && choice.predecessor.is_none() && choice.cost == Some(0.),
                    "rolling reach: truncated selected path",
                )?;
            } else {
                let previous = choice
                    .predecessor
                    .ok_or_else(|| invalid("rolling reach: extended selected path"))?;
                let gap = self.nodes[current]
                    .key
                    .1
                    .checked_sub(self.nodes[previous].key.1)
                    .filter(|gap| *gap > 0)
                    .ok_or_else(|| invalid("rolling reach: invalid predecessor date"))?;
                require(
                    reported.slot_gaps[position - 1] == gap && gap <= self.request.scope.max_lag,
                    "rolling reach: altered original date gap",
                )?;
                current = previous;
            }
        }
        Ok(())
    }

    pub fn finish(
        &self,
        records: &[&FounderRecord],
        paths: &[&SelectedPath],
        cancel: &Cancellation,
    ) -> Result<()> {
        cancel.poll()?;
        let (mut row, mut path) = (0, 0);
        for (query, state) in self.queries.iter().enumerate() {
            cancel.poll()?;
            let founder = &self.request.founders[query];
            let expected = FounderRecord::Origin {
                query,
                origin: self.origin(query),
            };
            require(
                records.get(row).is_some_and(|actual| **actual == expected),
                "rolling reach: origin record or ordering",
            )?;
            row += 1;
            let last_date = self
                .nodes
                .iter()
                .rev()
                .find(|node| {
                    node.key.0 == founder.subject && node.key.1 > founder.date && node.eligible
                })
                .map(|node| node.key.1);
            let mut cursor = self.nodes.partition_point(|node| {
                (node.key.0, node.key.1) <= (founder.subject, founder.date)
            });
            for date in founder.date + 1..self.request.dates.len() {
                for &lens in &self.request.lenses {
                    cancel.poll()?;
                    let mut any = false;
                    while cursor < self.nodes.len()
                        && (
                            self.nodes[cursor].key.0,
                            self.nodes[cursor].key.1,
                            self.nodes[cursor].key.2,
                        ) < (founder.subject, date, lens)
                    {
                        cursor += 1;
                    }
                    while cursor < self.nodes.len()
                        && (
                            self.nodes[cursor].key.0,
                            self.nodes[cursor].key.1,
                            self.nodes[cursor].key.2,
                        ) == (founder.subject, date, lens)
                    {
                        let node = cursor;
                        cursor += 1;
                        if !self.nodes[node].eligible {
                            continue;
                        }
                        any = true;
                        let offset = state
                            .offset
                            .ok_or_else(|| invalid("rolling reach: target offset"))?;
                        let cell = &state.cells[node - offset];
                        let Some(FounderRecord::Target {
                            query: actual_query,
                            node: actual_node,
                            direct,
                            families,
                        }) = records.get(row).copied()
                        else {
                            return Err(invalid("rolling reach: missing target record"));
                        };
                        require(
                            *actual_query == query
                                && *actual_node == node
                                && same_direct(direct, &self.expected_direct(query, cell))
                                && families.len()
                                    == state.enabled.iter().filter(|enabled| **enabled).count(),
                            "rolling reach: target identity, direct result or family scope",
                        )?;
                        let mut at_family = 0;
                        for (f, enabled) in state.enabled.iter().enumerate() {
                            if !enabled {
                                continue;
                            }
                            let expected = self.state(node, f, cell.choices[f])?;
                            require(
                                families[at_family] == expected
                                    && same_bound(&families[at_family].lower, &expected.lower)
                                    && same_bound(&families[at_family].upper, &expected.upper),
                                "rolling reach: altered bottleneck, predecessor or reach",
                            )?;
                            at_family += 1;
                            if Some(date) == last_date {
                                for side in 0..2 {
                                    if cell.choices[f][side].cost.is_none() {
                                        continue;
                                    }
                                    self.path(
                                        query,
                                        node,
                                        f,
                                        side,
                                        paths.get(path).copied().ok_or_else(|| {
                                            invalid("rolling reach: missing selected path")
                                        })?,
                                        cancel,
                                    )?;
                                    path += 1;
                                }
                            }
                        }
                        row += 1;
                    }
                    if !any {
                        let reason = if matches!(
                            self.request.subjects[founder.subject].slots[date],
                            Slot::Missing { .. }
                        ) {
                            Reason::MissingDate
                        } else {
                            Reason::ExcludedRank
                        };
                        require(
                            records.get(row).is_some_and(|actual| {
                                **actual
                                    == FounderRecord::Unavailable {
                                        query,
                                        date,
                                        lens,
                                        reason,
                                    }
                            }),
                            "rolling reach: unavailable target scope or reason",
                        )?;
                        row += 1;
                    }
                }
            }
        }
        require(
            row == records.len() && path == paths.len(),
            "rolling reach: extra founder or path records",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(dates: usize) -> Request {
        use eigentrail_core::{Matrix, rolling_lineage::*};
        let mut request = {
            Request {
                schema: SCHEMA.into(),
                chart: Chart {
                    frame: "identity".into(),
                    coordinates: "frame_coefficients".into(),
                },
                features: vec![
                    Feature {
                        id: "x".into(),
                        unit: "u".into(),
                    },
                    Feature {
                        id: "y".into(),
                        unit: "u".into(),
                    },
                ],
                dates: vec![
                    Date {
                        id: "d0".into(),
                        time: 0.,
                    },
                    Date {
                        id: "d1".into(),
                        time: 1.,
                    },
                ],
                subjects: vec![Subject {
                    id: "s0".into(),
                    slots: vec![
                        Slot::Observed {
                            covariance: Matrix {
                                n: 2,
                                data: vec![2., 0., 0., 1.],
                            },
                        },
                        Slot::Observed {
                            covariance: Matrix {
                                n: 2,
                                data: vec![3., 0., 0., 1.],
                            },
                        },
                    ],
                }],
                lenses: vec![Lens::Covariance],
                representation: Representation {
                    symmetry: "exact".into(),
                    correction: Correction::None,
                },
                grouping: Grouping { rtol: 1e-12 },
                tolerance_deg: 20.,
                uncertainty: Uncertainty::Point,
                scope: Scope {
                    lens_travel: vec![[Lens::Covariance, Lens::Covariance]],
                    rank_masks: vec![RankMask {
                        lens: Lens::Covariance,
                        ranks: vec![[0, 2]],
                    }],
                    max_lag: 1,
                    missing: "skip_recorded".into(),
                    policy: "draft_f_conservative".into(),
                    certificate: "observed_dated_subspaces".into(),
                    direct_scope: "all_later_slots".into(),
                },
                founders: vec![Founder {
                    subject: 0,
                    date: 0,
                    lens: Lens::Covariance,
                    rank: [0, 1],
                    policies: vec![
                        FounderPolicy::ResolvedLine,
                        FounderPolicy::OuterResolved,
                        FounderPolicy::OuterRelaxed,
                        FounderPolicy::TypedBlockGraph,
                    ],
                }],
                projection: Projection::SelectedEdges {
                    blocks: vec![EdgeBlock {
                        subject: 0,
                        from_date: 0,
                        to_date: 1,
                        source_lens: Lens::Covariance,
                        target_lens: Lens::Covariance,
                    }],
                },
                limits: Limits {
                    max_candidates: 64,
                    max_edge_records: 64,
                    max_bytes: 1024 * 1024,
                    chunk_records: 64,
                },
                precision: Precision {
                    format: "binary64".into(),
                    angle_unit: "degrees".into(),
                },
            }
        };
        while request.dates.len() < dates {
            let date = request.dates.len();
            request.dates.push(Date {
                id: format!("d{date}"),
                time: date as f64,
            });
            let slot = request.subjects[0].slots[0].clone();
            request.subjects[0].slots.push(slot);
        }
        request.founders[0].policies = vec![FounderPolicy::TypedBlockGraph];
        request
    }
    fn view(first: usize, date: usize, ranks: &[[usize; 2]]) -> ViewGroups {
        ViewGroups {
            subject: 0,
            date,
            lens: Lens::Covariance,
            reference: Reference::Root,
            spectrum: SpectrumState::Unavailable {
                reason: Reason::NumericalUnresolved,
            },
            region: RegionState::Point { numerical_rho: 0. },
            groups: ranks
                .iter()
                .enumerate()
                .map(|(offset, &rank)| GroupRecord {
                    node: first + offset,
                    rank,
                    columns: rank,
                    grouping: GroupingCause::Simple,
                    multiplicity: Multiplicity::Unknown,
                    information: if rank[1] - rank[0] == 1 {
                        InformationLevel::Line
                    } else {
                        InformationLevel::WholeSpace
                    },
                    angular_radius_deg: 0.,
                    spectral_sum: Enclosure::Undefined {
                        reason: Reason::NumericalUnresolved,
                    },
                    covariance_share: ShareState::NotApplicable,
                    eligible: true,
                })
                .collect(),
        }
    }
    fn edge(ordinal: usize, source: usize, target: usize, range: [f64; 2]) -> EdgeRecord {
        EdgeRecord {
            ordinal,
            source,
            target,
            edge_type: EdgeType::LineLine,
            smaller_side: SmallerSide::None,
            point: Enclosure::Bounds { range },
            geometry: Enclosure::Bounds { range },
            method_bracket: Enclosure::Bounds { range },
            verdict: if range[1] <= 20. {
                Verdict::Agreement
            } else if range[0] > 20. {
                Verdict::Exclusion
            } else {
                Verdict::Open
            },
        }
    }
    fn direct(edge: &EdgeRecord) -> DirectResult {
        DirectResult::Available {
            ordinal: edge.ordinal,
            point: edge.point.clone(),
            geometry: edge.geometry.clone(),
            method_bracket: edge.method_bracket.clone(),
            verdict: edge.verdict,
        }
    }
    fn typed(lower: f64, upper: f64, parents: [usize; 2]) -> FamilyState {
        FamilyState {
            family: BottleneckFamily::TypedBlockGraph,
            lower: Bottleneck::Bound { degrees: lower },
            upper: Bottleneck::Bound { degrees: upper },
            lower_parent: Parent::Node { node: parents[0] },
            upper_parent: Parent::Node { node: parents[1] },
            reach: if upper <= 20. {
                ReachState::Agreed
            } else if lower > 20. {
                ReachState::Excluded
            } else {
                ReachState::Possible
            },
        }
    }
    fn verify(reach: &Reach<'_>, rows: &[FounderRecord], paths: &[SelectedPath]) -> Result<()> {
        reach.finish(
            &rows.iter().collect::<Vec<_>>(),
            &paths.iter().collect::<Vec<_>>(),
            &Cancellation::default(),
        )
    }

    #[test]
    fn independent_bellman_uses_different_lower_and_upper_paths_and_binds_all_outputs() {
        let request = request(3);
        let cancel = Cancellation::default();
        let mut reach = Reach::new(&request, &cancel).unwrap();
        for view in [
            view(0, 0, &[[0, 1]]),
            view(1, 1, &[[0, 1], [1, 2]]),
            view(3, 2, &[[0, 1]]),
        ] {
            reach.register_view(&view, &cancel).unwrap();
        }
        for edge in [
            edge(0, 0, 1, [1., 60.]),
            edge(1, 0, 2, [5., 10.]),
            edge(2, 1, 3, [2., 3.]),
            edge(3, 2, 3, [15., 16.]),
        ] {
            reach.edge(&edge, &cancel).unwrap();
        }
        let direct_edges = [
            edge(0, 0, 1, [1., 60.]),
            edge(1, 0, 2, [5., 10.]),
            edge(2, 0, 3, [40., 41.]),
        ];
        for edge in &direct_edges {
            reach.direct(0, edge, &cancel).unwrap();
        }
        let rows = vec![
            FounderRecord::Origin {
                query: 0,
                origin: FounderOrigin::Node { node: 0 },
            },
            FounderRecord::Target {
                query: 0,
                node: 1,
                direct: direct(&direct_edges[0]),
                families: vec![typed(1., 60., [0, 0])],
            },
            FounderRecord::Target {
                query: 0,
                node: 2,
                direct: direct(&direct_edges[1]),
                families: vec![typed(5., 10., [0, 0])],
            },
            FounderRecord::Target {
                query: 0,
                node: 3,
                direct: direct(&direct_edges[2]),
                families: vec![typed(2., 16., [1, 2])],
            },
        ];
        let paths = vec![
            SelectedPath {
                query: 0,
                target: 3,
                family: BottleneckFamily::TypedBlockGraph,
                side: PathSide::Lower,
                nodes: vec![0, 1, 3],
                slot_gaps: vec![1, 1],
                cost: 2.,
            },
            SelectedPath {
                query: 0,
                target: 3,
                family: BottleneckFamily::TypedBlockGraph,
                side: PathSide::Upper,
                nodes: vec![0, 2, 3],
                slot_gaps: vec![1, 1],
                cost: 16.,
            },
        ];
        verify(&reach, &rows, &paths).unwrap();
        for change in 0..7 {
            let mut altered = rows.clone();
            if let FounderRecord::Target {
                direct, families, ..
            } = &mut altered[3]
            {
                match change {
                    0 => families[0].lower = Bottleneck::Bound { degrees: 1. },
                    1 => families[0].upper_parent = Parent::Node { node: 1 },
                    2 => families[0].reach = ReachState::Excluded,
                    3 => families.push(families[0].clone()),
                    4 => {
                        *direct = DirectResult::Unavailable {
                            reason: Reason::OutOfScope,
                        }
                    }
                    5 => {
                        if let DirectResult::Available { ordinal, .. } = direct {
                            *ordinal += 1;
                        }
                    }
                    _ => {
                        if let DirectResult::Available { method_bracket, .. } = direct {
                            *method_bracket = Enclosure::Bounds { range: [0., 0.] };
                        }
                    }
                }
            }
            assert!(verify(&reach, &altered, &paths).is_err());
        }
        for change in 0..6 {
            let mut altered = paths.clone();
            match change {
                0 => altered[0].nodes = vec![0, 2, 3],
                1 => altered[0].slot_gaps[0] = 2,
                2 => altered[0].cost = 1.,
                3 => altered.swap(0, 1),
                4 => {
                    altered.pop();
                }
                _ => altered.push(altered[0].clone()),
            }
            assert!(verify(&reach, &rows, &altered).is_err());
        }
        assert!(verify(&reach, &rows[1..], &paths).is_err());
        let mut duplicated = rows.clone();
        duplicated.push(rows[0].clone());
        assert!(verify(&reach, &duplicated, &paths).is_err());
    }

    #[test]
    fn immediate_typed_predecessor_tie_is_distinct_from_complete_path_lexicographic_order() {
        let request = request(4);
        let cancel = Cancellation::default();
        let mut reach = Reach::new(&request, &cancel).unwrap();
        for view in [
            view(0, 0, &[[0, 1]]),
            view(1, 1, &[[0, 1], [1, 2]]),
            view(3, 2, &[[0, 1], [1, 2]]),
            view(5, 3, &[[0, 1]]),
        ] {
            reach.register_view(&view, &cancel).unwrap();
        }
        for (ordinal, (source, target)) in [(0, 1), (0, 2), (2, 3), (1, 4), (3, 5), (4, 5)]
            .into_iter()
            .enumerate()
        {
            reach
                .edge(&edge(ordinal, source, target, [5., 5.]), &cancel)
                .unwrap();
        }
        assert_eq!(reach.queries[0].cells[5].choices[1][1].predecessor, Some(3));
        let path = SelectedPath {
            query: 0,
            target: 5,
            family: BottleneckFamily::TypedBlockGraph,
            side: PathSide::Upper,
            nodes: vec![0, 2, 3, 5],
            slot_gaps: vec![1, 1, 1],
            cost: 5.,
        };
        reach.path(0, 5, 1, 1, &path, &cancel).unwrap();
        let mut full_lex = path;
        full_lex.nodes = vec![0, 1, 4, 5];
        assert!(reach.path(0, 5, 1, 1, &full_lex, &cancel).is_err());
    }

    #[test]
    fn distinct_middle_vertices_do_not_compose_and_undefined_edges_remain_outer_only() {
        let request = request(3);
        let cancel = Cancellation::default();
        let mut reach = Reach::new(&request, &cancel).unwrap();
        for view in [
            view(0, 0, &[[0, 1]]),
            view(1, 1, &[[0, 1], [1, 2]]),
            view(3, 2, &[[0, 1]]),
        ] {
            reach.register_view(&view, &cancel).unwrap();
        }
        reach.edge(&edge(0, 0, 1, [0., 0.]), &cancel).unwrap();
        reach.edge(&edge(1, 2, 3, [0., 0.]), &cancel).unwrap();
        assert!(
            reach.queries[0].cells[3].choices[1]
                .iter()
                .all(|choice| choice.cost.is_none())
        );
        let mut unknown = edge(2, 1, 3, [0., 90.]);
        unknown.method_bracket = Enclosure::Undefined {
            reason: Reason::UnresolvedSeparation,
        };
        reach.edge(&unknown, &cancel).unwrap();
        assert_eq!(reach.queries[0].cells[3].choices[1][0].cost, Some(0.));
        assert_eq!(reach.queries[0].cells[3].choices[1][1].cost, None);
        assert_eq!(
            reach
                .state(3, 1, reach.queries[0].cells[3].choices[1])
                .unwrap()
                .reach,
            ReachState::Possible
        );
    }

    #[test]
    fn internal_founder_keeps_original_missing_slot_gap_and_never_certifies_at_ninety() {
        let mut request = request(3);
        request.subjects[0].slots[1] = Slot::Missing {
            reason: "not observed".into(),
        };
        request.scope.max_lag = 2;
        request.tolerance_deg = 90.;
        request.founders[0].policies = vec![
            FounderPolicy::ResolvedLine,
            FounderPolicy::TypedBlockGraph,
            FounderPolicy::OuterRelaxed,
        ];
        let cancel = Cancellation::default();
        let mut reach = Reach::new(&request, &cancel).unwrap();
        reach
            .register_view(&view(0, 0, &[[0, 2]]), &cancel)
            .unwrap();
        reach
            .register_view(&view(1, 2, &[[0, 1]]), &cancel)
            .unwrap();
        reach.edge(&edge(0, 0, 1, [0., 0.]), &cancel).unwrap();
        let no_route = |family| FamilyState {
            family,
            lower: Bottleneck::NoRoute,
            upper: Bottleneck::NoRoute,
            lower_parent: Parent::NoRoute,
            upper_parent: Parent::NoRoute,
            reach: ReachState::Unavailable {
                reason: Reason::NoCandidates,
            },
        };
        let rows = vec![
            FounderRecord::Origin {
                query: 0,
                origin: FounderOrigin::InternalLine { group: 0 },
            },
            FounderRecord::Unavailable {
                query: 0,
                date: 1,
                lens: Lens::Covariance,
                reason: Reason::MissingDate,
            },
            FounderRecord::Target {
                query: 0,
                node: 1,
                direct: DirectResult::Unavailable {
                    reason: Reason::UnresolvedInternalLine,
                },
                families: vec![
                    no_route(BottleneckFamily::LineOnly),
                    no_route(BottleneckFamily::TypedBlockGraph),
                    FamilyState {
                        family: BottleneckFamily::RelaxedInternalLines,
                        lower: Bottleneck::Bound { degrees: 0. },
                        upper: Bottleneck::NoRoute,
                        lower_parent: Parent::Node { node: 0 },
                        upper_parent: Parent::NoRoute,
                        reach: ReachState::Possible,
                    },
                ],
            },
        ];
        let paths = [SelectedPath {
            query: 0,
            target: 1,
            family: BottleneckFamily::RelaxedInternalLines,
            side: PathSide::Lower,
            nodes: vec![0, 1],
            slot_gaps: vec![2],
            cost: 0.,
        }];
        verify(&reach, &rows, &paths).unwrap();
        assert!(reach.direct(0, &edge(0, 0, 1, [0., 0.]), &cancel).is_err());
        let mut altered = rows;
        if let FounderRecord::Target { families, .. } = &mut altered[2] {
            families[2].reach = ReachState::Agreed;
        }
        assert!(verify(&reach, &altered, &paths).is_err());
    }

    #[test]
    fn unavailable_origin_empty_mask_and_callback_domains_are_explicit() {
        let mut request = request(2);
        request.founders[0].rank = [0, 2];
        let cancel = Cancellation::default();
        let mut reach = Reach::new(&request, &cancel).unwrap();
        reach
            .register_view(&view(0, 0, &[[0, 1], [1, 2]]), &cancel)
            .unwrap();
        let mut masked = view(2, 1, &[[0, 2]]);
        masked.groups[0].eligible = false;
        reach.register_view(&masked, &cancel).unwrap();
        let rows = [
            FounderRecord::Origin {
                query: 0,
                origin: FounderOrigin::Unavailable {
                    reason: Reason::OutOfScope,
                },
            },
            FounderRecord::Unavailable {
                query: 0,
                date: 1,
                lens: Lens::Covariance,
                reason: Reason::ExcludedRank,
            },
        ];
        verify(&reach, &rows, &[]).unwrap();
        assert!(reach.edge(&edge(0, 0, 2, [0., 0.]), &cancel).is_err());
        assert!(reach.register_view(&masked, &cancel).is_err());
        let mut missing_request = request.clone();
        missing_request.subjects[0].slots[0] = Slot::Missing {
            reason: "missing".into(),
        };
        let empty = Reach::new(&missing_request, &cancel).unwrap();
        assert_eq!(
            empty.origin(0),
            FounderOrigin::Unavailable {
                reason: Reason::MissingDate
            }
        );
        let cancelled = Cancellation::default();
        cancelled.cancel();
        assert!(Reach::new(&request, &cancelled).is_err());
        assert!(reach.finish(&[], &[], &cancelled).is_err());
    }
}
