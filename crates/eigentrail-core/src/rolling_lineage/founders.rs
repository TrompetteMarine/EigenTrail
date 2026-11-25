//! Query-specific min-max state. One predecessor per bound; internal lines stay outer information.
use super::{FounderPolicy, Request, graph::Vertex, report::*};
use crate::{
    Error, Result,
    resources::{Cancellation, memory},
};

#[derive(Clone, Copy)]
struct Node {
    key: Vertex,
    line: bool,
    eligible: bool,
}
#[derive(Clone, Copy)]
struct Route {
    costs: [Option<f64>; 2],
    parents: [Option<usize>; 2],
}
const ABSENT: Route = Route {
    costs: [None; 2],
    parents: [None; 2],
};
struct Target {
    routes: [Route; 3],
    direct: DirectResult,
}
#[derive(Clone, Copy)]
enum Origin {
    Pending,
    Node(usize),
    Internal(usize),
    Unavailable(Reason),
}
struct Query {
    origin: Origin,
    offset: Option<usize>,
    targets: Vec<Target>,
}
pub struct State<'a> {
    request: &'a Request,
    nodes: Vec<Node>,
    queries: Vec<Query>,
}
fn bad(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|e| e)
}
fn overflow() -> Error {
    Error::Resource(String::new())
}
fn families(policy: &[FounderPolicy]) -> [bool; 3] {
    [
        policy.iter().any(|p| {
            matches!(
                p,
                FounderPolicy::ResolvedLine | FounderPolicy::OuterResolved
            )
        }),
        policy.contains(&FounderPolicy::TypedBlockGraph),
        policy.contains(&FounderPolicy::OuterRelaxed),
    ]
}
fn range(bound: &Enclosure) -> Option<[f64; 2]> {
    if let Enclosure::Bounds { range } = bound {
        Some(*range)
    } else {
        None
    }
}
fn bottleneck(cost: Option<f64>) -> Bottleneck {
    cost.map_or(Bottleneck::NoRoute, |degrees| Bottleneck::Bound { degrees })
}
fn parent(route: Route, side: usize, origin: bool) -> Parent {
    if route.costs[side].is_none() {
        Parent::NoRoute
    } else if origin {
        Parent::Origin
    } else {
        route.parents[side].map_or(Parent::NoRoute, |node| Parent::Node { node })
    }
}
impl<'a> State<'a> {
    pub fn new(request: &'a Request, cancel: &Cancellation) -> Result<Self> {
        cancel.poll()?;
        let mut queries = memory::capacity(request.founders.len())?;
        for _ in &request.founders {
            queries.push(Query {
                origin: Origin::Pending,
                offset: None,
                targets: memory::capacity(0)?,
            });
        }
        Ok(Self {
            request,
            nodes: memory::capacity(0)?,
            queries,
        })
    }
    pub fn register_view(&mut self, view: &ViewGroups, cancel: &Cancellation) -> Result<()> {
        for group in &view.groups {
            cancel.poll()?;
            if group.node != self.nodes.len() {
                return Err(bad("rolling founder node order"));
            }
            memory::push(
                &mut self.nodes,
                Node {
                    key: Vertex {
                        subject: view.subject,
                        date: view.date,
                        lens: view.lens,
                        rank: group.rank,
                    },
                    line: group.rank[1] - group.rank[0] == 1,
                    eligible: group.eligible,
                },
            )?;
            for (index, query) in self.queries.iter_mut().enumerate() {
                let founder = &self.request.founders[index];
                if founder.subject != view.subject {
                    continue;
                }
                if query.offset.is_none() {
                    query.offset = Some(group.node);
                }
                if founder.subject == view.subject
                    && founder.date == view.date
                    && founder.lens == view.lens
                    && group.rank[0] <= founder.rank[0]
                    && group.rank[1] >= founder.rank[1]
                {
                    query.origin = if !group.eligible {
                        Origin::Unavailable(Reason::ExcludedRank)
                    } else if group.rank == founder.rank {
                        Origin::Node(group.node)
                    } else {
                        Origin::Internal(group.node)
                    };
                }
                let mut target = Target {
                    routes: [ABSENT; 3],
                    direct: DirectResult::Unavailable {
                        reason: Reason::OutOfScope,
                    },
                };
                let active = families(&founder.policies);
                match query.origin {
                    Origin::Node(node) if node == group.node => {
                        for (family, enabled) in active.iter().enumerate() {
                            if *enabled && (family != 0 || group.rank[1] - group.rank[0] == 1) {
                                target.routes[family].costs = [Some(0.), Some(0.)];
                            }
                        }
                    }
                    Origin::Internal(node) if node == group.node && active[2] => {
                        target.routes[2].costs = [Some(0.), None]
                    }
                    _ => {}
                }
                memory::push(&mut query.targets, target)?;
            }
        }
        Ok(())
    }
    pub fn edge(&mut self, edge: &EdgeRecord, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let source = *self
            .nodes
            .get(edge.source)
            .ok_or_else(|| bad("rolling founder source missing"))?;
        let target = *self
            .nodes
            .get(edge.target)
            .ok_or_else(|| bad("rolling founder target missing"))?;
        if source.key.subject != target.key.subject
            || source.key.date >= target.key.date
            || target.key.date - source.key.date > self.request.scope.max_lag
        {
            return Err(bad("rolling founder invalid edge date"));
        }
        if !source.eligible || !target.eligible {
            return Err(bad("rolling founder masked edge"));
        }
        let method = range(&edge.method_bracket);
        let geometry = range(&edge.geometry);
        for (index, query) in self.queries.iter_mut().enumerate() {
            let enabled = families(&self.request.founders[index].policies);
            if self.request.founders[index].subject != source.key.subject {
                continue;
            }
            for (family, is_enabled) in enabled.iter().enumerate() {
                if !is_enabled || family == 0 && (!source.line || !target.line) {
                    continue;
                }
                let costs = if family == 2 && (!source.line || !target.line) {
                    // A path through a block need not select the same internal line at both edges.
                    [Some(geometry.map_or(0., |x| x[0])), None]
                } else {
                    method.map_or([Some(0.), None], |x| [Some(x[0]), Some(x[1])])
                };
                let offset = query
                    .offset
                    .ok_or_else(|| bad("rolling founder offset missing"))?;
                let prefix = query.targets[edge.source - offset].routes[family];
                let record = &mut query.targets[edge.target - offset].routes[family];
                for (side, cost) in costs.iter().enumerate() {
                    if let (Some(prior), Some(cost)) = (prefix.costs[side], *cost) {
                        let candidate = prior.max(cost);
                        let better = record.costs[side].is_none_or(|old| {
                            candidate < old
                                || candidate == old
                                    && record.parents[side].is_some_and(|p| edge.source < p)
                        });
                        if better {
                            record.costs[side] = Some(candidate);
                            record.parents[side] = Some(edge.source);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn direct(&mut self, query: usize, edge: &EdgeRecord, cancel: &Cancellation) -> Result<()> {
        cancel.poll()?;
        let state = self
            .queries
            .get_mut(query)
            .ok_or_else(|| bad("rolling direct query missing"))?;
        let offset = state
            .offset
            .ok_or_else(|| bad("rolling direct offset missing"))?;
        let target = state
            .targets
            .get_mut(
                edge.target
                    .checked_sub(offset)
                    .ok_or_else(|| bad("rolling direct target subject"))?,
            )
            .ok_or_else(|| bad("rolling direct target missing"))?;
        match state.origin {
            Origin::Node(node) if node == edge.source => {
                target.direct = DirectResult::Available {
                    ordinal: edge.ordinal,
                    point: edge.point.clone(),
                    geometry: edge.geometry.clone(),
                    method_bracket: edge.method_bracket.clone(),
                    verdict: edge.verdict,
                }
            }
            Origin::Internal(_) => {
                target.direct = DirectResult::Unavailable {
                    reason: Reason::UnresolvedInternalLine,
                }
            }
            _ => return Err(bad("rolling direct founder identity")),
        }
        Ok(())
    }
    pub fn finish(
        self,
        request: &Request,
        cancel: &Cancellation,
    ) -> Result<(Vec<FounderRecord>, Vec<SelectedPath>)> {
        if !std::ptr::eq(self.request, request) {
            return Err(bad("rolling founder request mismatch"));
        }
        let mut records = memory::capacity(0)?;
        let mut paths = memory::capacity(0)?;
        for (query_index, query) in self.queries.iter().enumerate() {
            cancel.poll()?;
            let founder = &request.founders[query_index];
            let origin = match query.origin {
                Origin::Node(node) => FounderOrigin::Node { node },
                Origin::Internal(group) => FounderOrigin::InternalLine { group },
                Origin::Unavailable(reason) => FounderOrigin::Unavailable { reason },
                Origin::Pending => FounderOrigin::Unavailable {
                    reason: if matches!(
                        request.subjects[founder.subject].slots[founder.date],
                        super::Slot::Missing { .. }
                    ) {
                        Reason::MissingDate
                    } else {
                        Reason::OutOfScope
                    },
                },
            };
            memory::push(
                &mut records,
                FounderRecord::Origin {
                    query: query_index,
                    origin,
                },
            )?;
            let enabled = families(&founder.policies);
            let last_date = self
                .nodes
                .iter()
                .rev()
                .find(|n| {
                    n.key.subject == founder.subject && n.key.date > founder.date && n.eligible
                })
                .map(|n| n.key.date);
            let mut cursor = 0;
            for date in founder.date + 1..request.dates.len() {
                for &lens in &request.lenses {
                    let mut any = false;
                    while cursor < self.nodes.len()
                        && (
                            self.nodes[cursor].key.subject,
                            self.nodes[cursor].key.date,
                            self.nodes[cursor].key.lens,
                        ) < (founder.subject, date, lens)
                    {
                        cursor += 1;
                    }
                    let mut at = cursor;
                    while at < self.nodes.len()
                        && (
                            self.nodes[at].key.subject,
                            self.nodes[at].key.date,
                            self.nodes[at].key.lens,
                        ) == (founder.subject, date, lens)
                    {
                        cancel.poll()?;
                        let node = self.nodes[at];
                        if node.eligible {
                            any = true;
                            let offset = query
                                .offset
                                .ok_or_else(|| bad("rolling founder target offset"))?;
                            let target = &query.targets[at - offset];
                            let mut states =
                                memory::capacity(enabled.iter().filter(|x| **x).count())?;
                            for (family, is_enabled) in enabled.iter().enumerate() {
                                if !is_enabled {
                                    continue;
                                }
                                let route = target.routes[family];
                                let family_name = [
                                    BottleneckFamily::LineOnly,
                                    BottleneckFamily::TypedBlockGraph,
                                    BottleneckFamily::RelaxedInternalLines,
                                ][family];
                                let reach = if route.costs[1]
                                    .is_some_and(|u| u <= request.tolerance_deg)
                                {
                                    ReachState::Agreed
                                } else if route.costs[0].is_some_and(|l| l > request.tolerance_deg)
                                {
                                    ReachState::Excluded
                                } else if route.costs[0].is_some() {
                                    ReachState::Possible
                                } else {
                                    ReachState::Unavailable {
                                        reason: if !node.line && family == 0 {
                                            Reason::UnresolvedInternalLine
                                        } else {
                                            Reason::NoCandidates
                                        },
                                    }
                                };
                                states.push(FamilyState {
                                    family: family_name,
                                    lower: bottleneck(route.costs[0]),
                                    upper: bottleneck(route.costs[1]),
                                    lower_parent: parent(route, 0, false),
                                    upper_parent: parent(route, 1, false),
                                    reach,
                                });
                                if Some(date) == last_date {
                                    for side in 0..2 {
                                        let Some(cost) = route.costs[side] else {
                                            continue;
                                        };
                                        let mut nodes = memory::capacity(0)?;
                                        let mut gaps = memory::capacity(0)?;
                                        let mut current = at;
                                        loop {
                                            cancel.poll()?;
                                            memory::push(&mut nodes, current)?;
                                            let Some(previous) = query.targets[current - offset]
                                                .routes[family]
                                                .parents[side]
                                            else {
                                                break;
                                            };
                                            let gap = self.nodes[current]
                                                .key
                                                .date
                                                .checked_sub(self.nodes[previous].key.date)
                                                .filter(|v| *v > 0)
                                                .ok_or_else(|| {
                                                    bad("rolling founder predecessor date")
                                                })?;
                                            memory::push(&mut gaps, gap)?;
                                            if nodes.len() > self.nodes.len() {
                                                return Err(overflow());
                                            }
                                            current = previous;
                                        }
                                        nodes.reverse();
                                        gaps.reverse();
                                        memory::push(
                                            &mut paths,
                                            SelectedPath {
                                                query: query_index,
                                                target: at,
                                                family: family_name,
                                                side: if side == 0 {
                                                    PathSide::Lower
                                                } else {
                                                    PathSide::Upper
                                                },
                                                nodes,
                                                slot_gaps: gaps,
                                                cost,
                                            },
                                        )?;
                                    }
                                }
                            }
                            let direct = match query.origin {
                                Origin::Internal(_) => DirectResult::Unavailable {
                                    reason: Reason::UnresolvedInternalLine,
                                },
                                Origin::Unavailable(_) | Origin::Pending => {
                                    DirectResult::Unavailable {
                                        reason: Reason::SourceUnavailable,
                                    }
                                }
                                Origin::Node(_) => target.direct.clone(),
                            };
                            memory::push(
                                &mut records,
                                FounderRecord::Target {
                                    query: query_index,
                                    node: at,
                                    direct,
                                    families: states,
                                },
                            )?;
                        }
                        at += 1;
                    }
                    if !any {
                        memory::push(
                            &mut records,
                            FounderRecord::Unavailable {
                                query: query_index,
                                date,
                                lens,
                                reason: if matches!(
                                    request.subjects[founder.subject].slots[date],
                                    super::Slot::Missing { .. }
                                ) {
                                    Reason::MissingDate
                                } else {
                                    Reason::ExcludedRank
                                },
                            },
                        )?;
                    }
                }
            }
        }
        Ok((records, paths))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rolling_lineage::{self as rolling, Lens};
    fn request() -> Request {
        super::super::test_request()
    }
    fn view(node: usize, date: usize, rank: [usize; 2]) -> ViewGroups {
        ViewGroups {
            subject: 0,
            date,
            lens: Lens::Covariance,
            reference: Reference::Root,
            spectrum: SpectrumState::Unavailable {
                reason: Reason::NumericalUnresolved,
            },
            region: RegionState::Point { numerical_rho: 0. },
            groups: vec![GroupRecord {
                node,
                rank,
                columns: rank,
                grouping: if rank[1] - rank[0] > 1 {
                    GroupingCause::Region
                } else {
                    GroupingCause::Simple
                },
                multiplicity: Multiplicity::Unknown,
                information: if rank[1] - rank[0] > 1 {
                    InformationLevel::WholeSpace
                } else {
                    InformationLevel::Line
                },
                angular_radius_deg: 0.,
                spectral_sum: Enclosure::Undefined {
                    reason: Reason::NumericalUnresolved,
                },
                covariance_share: ShareState::NotApplicable,
                eligible: true,
            }],
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
    fn target(records: &[FounderRecord], node: usize) -> (&DirectResult, &[FamilyState]) {
        records
            .iter()
            .find_map(|r| {
                if let FounderRecord::Target {
                    node: n,
                    direct,
                    families,
                    ..
                } = r
                {
                    (*n == node).then_some((direct, families.as_slice()))
                } else {
                    None
                }
            })
            .unwrap()
    }
    #[test]
    fn dated_skip_changes_chain_not_direct_and_internal_line_stays_outer() {
        let mut request = request();
        request.dates.push(rolling::Date {
            id: "third".into(),
            time: 2.,
        });
        let slot = request.subjects[0].slots[0].clone();
        request.subjects[0].slots.push(slot);
        request.scope.max_lag = 2;
        request.tolerance_deg = 20.;
        request.founders[0].policies = vec![
            FounderPolicy::ResolvedLine,
            FounderPolicy::TypedBlockGraph,
            FounderPolicy::OuterRelaxed,
        ];
        let cancel = Cancellation::default();
        let mut state = State::new(&request, &cancel).unwrap();
        for v in [view(0, 0, [0, 1]), view(1, 1, [0, 2]), view(2, 2, [0, 1])] {
            state.register_view(&v, &cancel).unwrap();
        }
        state.edge(&edge(0, 0, 1, [0., 90.]), &cancel).unwrap();
        state.edge(&edge(1, 1, 2, [0., 90.]), &cancel).unwrap();
        state.edge(&edge(2, 0, 2, [10., 19.]), &cancel).unwrap();
        state
            .direct(0, &edge(0, 0, 2, [40., 41.]), &cancel)
            .unwrap();
        let (records, paths) = state.finish(&request, &cancel).unwrap();
        let (direct, families) = target(&records, 2);
        assert!(matches!(
            direct,
            DirectResult::Available {
                verdict: Verdict::Exclusion,
                ..
            }
        ));
        let typed = families
            .iter()
            .find(|f| f.family == BottleneckFamily::TypedBlockGraph)
            .unwrap();
        assert_eq!(typed.lower, Bottleneck::Bound { degrees: 0. });
        assert_eq!(typed.upper, Bottleneck::Bound { degrees: 19. });
        assert_eq!(typed.reach, ReachState::Agreed);
        let path = paths
            .iter()
            .find(|p| {
                p.target == 2
                    && p.family == BottleneckFamily::TypedBlockGraph
                    && p.side == PathSide::Upper
            })
            .unwrap();
        assert_eq!(path.nodes, [0, 2]);
        assert_eq!(path.slot_gaps, [2]);
        let (_, middle) = target(&records, 1);
        let relaxed = middle
            .iter()
            .find(|f| f.family == BottleneckFamily::RelaxedInternalLines)
            .unwrap();
        assert_eq!(relaxed.reach, ReachState::Possible);
        assert_eq!(relaxed.upper, Bottleneck::NoRoute);
    }
    #[test]
    fn unresolved_founder_does_not_become_an_identified_line_at_ninety_degrees() {
        let mut request = request();
        request.tolerance_deg = 90.;
        request.founders[0].policies = vec![
            FounderPolicy::ResolvedLine,
            FounderPolicy::TypedBlockGraph,
            FounderPolicy::OuterRelaxed,
        ];
        let cancel = Cancellation::default();
        let mut state = State::new(&request, &cancel).unwrap();
        state.register_view(&view(0, 0, [0, 2]), &cancel).unwrap();
        state.register_view(&view(1, 1, [0, 1]), &cancel).unwrap();
        state.edge(&edge(0, 0, 1, [0., 0.]), &cancel).unwrap();
        let (records, _) = state.finish(&request, &cancel).unwrap();
        assert!(matches!(
            records[0],
            FounderRecord::Origin {
                origin: FounderOrigin::InternalLine { group: 0 },
                ..
            }
        ));
        let (_, families) = target(&records, 1);
        assert!(families.iter().all(|f| f.reach != ReachState::Agreed));
        assert_eq!(families[2].reach, ReachState::Possible);
    }
}
