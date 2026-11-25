//! Observed-date rolling input contract. Structural admission does not certify SPD.
use crate::{
    Error, Matrix, Result, numerics,
    resources::{self, Cancellation, memory},
};
use serde::{Deserialize, Serialize};

pub mod angles;
pub mod founders;
pub mod graph;
pub mod integrity;
pub mod numerical;
pub mod produce;
pub mod regional;
pub mod report;
pub mod views;
pub use produce::analyze;
pub use report::Report;

pub const SCHEMA: &str = "eigentrail.rolling-lineage.v1";
pub const MAX_SUBJECTS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lens {
    Covariance,
    Precision,
    Correlation,
    Partial,
}
impl Lens {
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FounderPolicy {
    ResolvedLine,
    OuterResolved,
    OuterRelaxed,
    TypedBlockGraph,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    pub frame: String,
    pub coordinates: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub unit: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Date {
    pub id: String,
    pub time: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tag", rename_all = "snake_case", deny_unknown_fields)]
pub enum Slot {
    Observed { covariance: Matrix },
    Missing { reason: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub id: String,
    pub slots: Vec<Slot>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Correction {
    None,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Representation {
    pub symmetry: String,
    pub correction: Correction,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grouping {
    pub rtol: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Uncertainty {
    Point,
    Conditional {
        joint_event: String,
        provider: String,
        rho: f64,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankMask {
    pub lens: Lens,
    pub ranks: Vec<[usize; 2]>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub lens_travel: Vec<[Lens; 2]>,
    pub rank_masks: Vec<RankMask>,
    pub max_lag: usize,
    pub missing: String,
    pub policy: String,
    pub certificate: String,
    pub direct_scope: String,
}
impl Scope {
    /// Adjacent mask intervals may jointly cover a group; a partial group never does.
    pub fn covers_group(&self, lens: Lens, rank: [usize; 2]) -> bool {
        if rank[0] >= rank[1] {
            return false;
        }
        let Some(mask) = self.rank_masks.iter().find(|mask| mask.lens == lens) else {
            return false;
        };
        let mut covered = rank[0];
        for &[start, end] in &mask.ranks {
            if end <= covered {
                continue;
            }
            if start > covered {
                return false;
            }
            covered = end;
            if covered >= rank[1] {
                return true;
            }
        }
        false
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Founder {
    pub subject: usize,
    pub date: usize,
    pub lens: Lens,
    pub rank: [usize; 2],
    pub policies: Vec<FounderPolicy>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeBlock {
    pub subject: usize,
    pub from_date: usize,
    pub to_date: usize,
    pub source_lens: Lens,
    pub target_lens: Lens,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Projection {
    Counts,
    SelectedEdges { blocks: Vec<EdgeBlock> },
    Full,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_candidates: usize,
    pub max_edge_records: usize,
    pub max_bytes: usize,
    pub chunk_records: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Precision {
    pub format: String,
    pub angle_unit: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub chart: Chart,
    pub features: Vec<Feature>,
    pub dates: Vec<Date>,
    pub subjects: Vec<Subject>,
    pub lenses: Vec<Lens>,
    pub representation: Representation,
    pub grouping: Grouping,
    pub tolerance_deg: f64,
    pub uncertainty: Uncertainty,
    pub scope: Scope,
    pub founders: Vec<Founder>,
    pub projection: Projection,
    pub limits: Limits,
    pub precision: Precision,
}

/// Heap-free shape inspected before copying input; not a scientific certificate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shape {
    pub features: usize,
    pub dates: usize,
    pub subjects: usize,
    pub lenses: usize,
    pub max_lag: usize,
    pub lens_pairs: usize,
    pub rank_pairs: usize,
    pub founders: usize,
    pub founder_policies: usize,
    pub direct_founders: [usize; 4],
    pub first_founder_date: [usize; 4],
    pub direct_targets: [usize; 4],
    pub selected_blocks: usize,
    pub full_edges: bool,
    pub wire_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preflight {
    pub lag: usize,
    pub date_pairs: usize,
    pub all_simple_bound: usize,
    pub eligible_ceiling: usize,
    pub direct_ceiling: usize,
    pub direct_extra_ceiling: usize,
    pub work_ceiling: usize,
    pub edge_record_ceiling: usize,
    pub managed_reservation: usize,
}

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn resource(message: &str) -> Error {
    memory::text(message)
        .map(Error::Resource)
        .unwrap_or_else(|error| error)
}
fn overflow() -> Error {
    resource("rolling count or reservation overflow; no exclusion certified")
}
fn product(values: &[usize]) -> Result<usize> {
    values.iter().try_fold(1usize, |total, value| {
        total.checked_mul(*value).ok_or_else(overflow)
    })
}
fn sum(values: &[usize]) -> Result<usize> {
    values.iter().try_fold(0usize, |total, value| {
        total.checked_add(*value).ok_or_else(overflow)
    })
}
fn date_pairs(dates: usize, lag: usize) -> Result<usize> {
    let lag = lag.min(dates.saturating_sub(1));
    // H*T-H*(H+1)/2, dividing before multiplication to avoid spurious overflow.
    let next = lag.checked_add(1).ok_or_else(overflow)?;
    let triangular = if lag % 2 == 0 {
        product(&[lag / 2, next])?
    } else {
        product(&[lag, next / 2])?
    };
    product(&[lag, dates])?
        .checked_sub(triangular)
        .ok_or_else(overflow)
}
pub fn all_simple_bound(
    subjects: usize,
    features: usize,
    dates: usize,
    lag: usize,
) -> Result<usize> {
    product(&[16, subjects, features, features, date_pairs(dates, lag)?])
}

/// Conservative streaming reservation; the shared allocator still enforces actual live bytes.
pub fn shape_preflight(shape: &Shape, limits: &Limits) -> Result<Preflight> {
    let Shape {
        features: n,
        dates: t,
        subjects: k,
        ..
    } = *shape;
    if !(1..=256).contains(&n)
        || !(1..=10000).contains(&t)
        || !(1..=MAX_SUBJECTS).contains(&k)
        || shape.lenses > 4
        || shape.lens_pairs > 16
        || limits.max_bytes > resources::DEFAULT_BYTES
        || limits.chunk_records == 0
        || limits.chunk_records > 65536
        || shape.rank_pairs > product(&[16, n, n])?
    {
        return Err(invalid(
            "rolling dimensions, chunk size or managed budget outside supported bounds",
        ));
    }
    let lag = shape.max_lag.min(t - 1);
    let pairs = date_pairs(t, lag)?;
    let universal = all_simple_bound(k, n, t, lag)?;
    let eligible = product(&[k, pairs, shape.rank_pairs])?;
    let (mut direct, mut extra) = (0usize, 0usize);
    for (lens, &count) in shape.direct_founders.iter().enumerate() {
        if count == 0 {
            continue;
        }
        if shape.first_founder_date[lens] >= t || shape.direct_targets[lens] > 4 * n {
            return Err(invalid(
                "direct founder dates or target ranks outside the declared shape",
            ));
        }
        let later = t - 1 - shape.first_founder_date[lens];
        direct = sum(&[
            direct,
            product(&[count, later, shape.direct_targets[lens]])?,
        ])?;
        extra = sum(&[
            extra,
            product(&[count, later.saturating_sub(lag), shape.direct_targets[lens]])?,
        ])?;
    }
    let work = sum(&[eligible, extra])?;
    if work > limits.max_candidates {
        return Err(resource(
            "rolling graph and extra-direct work ceiling exceeds cap; no exclusion certified",
        ));
    }
    let edges = if shape.full_edges {
        eligible
    } else {
        eligible.min(product(&[shape.selected_blocks, n, n])?)
    };
    if edges > limits.max_edge_records {
        return Err(resource(
            "rolling projected-edge ceiling exceeds cap; no truncation or exclusion certified",
        ));
    }
    let matrix = product(&[n, n, 8])?;
    let input = product(&[k, t, sum(&[matrix, 128])?])?;
    let metadata = product(&[
        sum(&[n, t, k, shape.founders, shape.selected_blocks])?,
        8192,
    ])?;
    let scratch = product(&[matrix, 64, shape.lenses.max(1), (lag + 1).min(t)])?;
    let windows = product(&[k, pairs, shape.lens_pairs + 1, 256])?;
    // Each queried subject retains three bound families and direct fields, including Vec growth.
    let predecessors = product(&[shape.founders, t, n, shape.lenses.max(1), 768])?;
    let node_index = product(&[k, t, n, shape.lenses.max(1), 128])?;
    let reservation = sum(&[
        input,
        metadata,
        scratch,
        windows,
        predecessors,
        node_index,
        product(&[edges, 6144])?,
        product(&[direct, 512])?,
        product(&[shape.wire_bytes, 2])?,
        65536,
    ])?;
    if reservation > limits.max_bytes {
        return Err(resource(
            "rolling streaming reservation exceeds managed budget; no exclusion certified",
        ));
    }
    Ok(Preflight {
        lag,
        date_pairs: pairs,
        all_simple_bound: universal,
        eligible_ceiling: eligible,
        direct_ceiling: direct,
        direct_extra_ceiling: extra,
        work_ceiling: work,
        edge_record_ceiling: edges,
        managed_reservation: reservation,
    })
}

fn text(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(invalid(
            "nonempty rolling IDs, units and descriptions of at most 4096 bytes required",
        ));
    }
    Ok(())
}
fn rank_valid(rank: [usize; 2], n: usize) -> bool {
    rank[0] < rank[1] && rank[1] <= n
}

/// Validate representation, IDs, indexing and resource scope; SPD remains a producer/checker gate.
pub fn validate_structure(request: &Request, cancel: &Cancellation) -> Result<Preflight> {
    cancel.poll()?;
    if request.schema != SCHEMA
        || request.chart.coordinates != "frame_coefficients"
        || request.representation.symmetry != "exact"
        || request.precision.format != "binary64"
        || request.precision.angle_unit != "degrees"
        || request.scope.missing != "skip_recorded"
        || request.scope.policy != "draft_f_conservative"
        || request.scope.certificate != "observed_dated_subspaces"
        || request.scope.direct_scope != "all_later_slots"
    {
        return Err(invalid(
            "unsupported rolling schema, chart, precision or scope",
        ));
    }
    text(&request.chart.frame)?;
    if !request.tolerance_deg.is_finite()
        || !(0. ..=90.).contains(&request.tolerance_deg)
        || !request.grouping.rtol.is_finite()
        || request.grouping.rtol < 0.
    {
        return Err(invalid(
            "finite rolling angular tolerance in [0,90] and nonnegative grouping rtol required",
        ));
    }
    if let Uncertainty::Conditional {
        joint_event,
        provider,
        rho,
    } = &request.uncertainty
    {
        text(joint_event)?;
        text(provider)?;
        if !rho.is_finite() || *rho < 0. {
            return Err(invalid(
                "finite nonnegative conditional Thompson radius required",
            ));
        }
    }
    let (n, t, k) = (
        request.features.len(),
        request.dates.len(),
        request.subjects.len(),
    );
    if !(1..=256).contains(&n) || !(1..=10000).contains(&t) || !(1..=MAX_SUBJECTS).contains(&k) {
        return Err(invalid(
            "rolling features 1..256, dates 1..10000 and subjects 1..1024 required",
        ));
    }
    for (index, feature) in request.features.iter().enumerate() {
        cancel.poll()?;
        text(&feature.id)?;
        text(&feature.unit)?;
        if request.features[..index]
            .iter()
            .any(|old| old.id == feature.id)
        {
            return Err(invalid("duplicate feature ID"));
        }
    }
    for (index, date) in request.dates.iter().enumerate() {
        cancel.poll()?;
        text(&date.id)?;
        if !date.time.is_finite()
            || (index > 0 && date.time <= request.dates[index - 1].time)
            || request.dates[..index].iter().any(|old| old.id == date.id)
        {
            return Err(invalid(
                "unique dated IDs and strictly increasing finite times required",
            ));
        }
    }
    // Matrix validation precedes mask/projection handling, including empty queries.
    for (index, subject) in request.subjects.iter().enumerate() {
        cancel.poll()?;
        text(&subject.id)?;
        if subject.slots.len() != t
            || request.subjects[..index]
                .iter()
                .any(|old| old.id == subject.id)
        {
            return Err(invalid(
                "unique subject IDs and one explicit slot per date required",
            ));
        }
        for slot in &subject.slots {
            cancel.poll()?;
            match slot {
                Slot::Observed { covariance } => {
                    if covariance.n != n {
                        return Err(invalid("rolling covariance dimension mismatch"));
                    }
                    numerics::validate_matrix(covariance)?;
                }
                Slot::Missing { reason } => text(reason)?,
            }
        }
    }
    if request.lenses.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(
            "unique lenses in declared covariance/precision/correlation/partial order required",
        ));
    }
    let mut covered = [0usize; 4];
    for (index, mask) in request.scope.rank_masks.iter().enumerate() {
        cancel.poll()?;
        if !request.lenses.contains(&mask.lens)
            || request.scope.rank_masks[..index]
                .iter()
                .any(|old| old.lens == mask.lens)
        {
            return Err(invalid("unique rank masks for declared lenses required"));
        }
        let mut end = 0;
        for &rank in &mask.ranks {
            cancel.poll()?;
            if !rank_valid(rank, n) || rank[0] < end {
                return Err(invalid(
                    "ordered disjoint zero-based half-open rank masks required",
                ));
            }
            covered[mask.lens.index()] = sum(&[covered[mask.lens.index()], rank[1] - rank[0]])?;
            end = rank[1];
        }
    }
    let mut rank_pairs = 0usize;
    let mut direct_targets = [0usize; 4];
    for (index, pair) in request.scope.lens_travel.iter().enumerate() {
        cancel.poll()?;
        if pair.iter().any(|lens| !request.lenses.contains(lens))
            || request.scope.lens_travel[..index].contains(pair)
        {
            return Err(invalid(
                "unique ordered travel pairs over declared lenses required",
            ));
        }
        rank_pairs = sum(&[
            rank_pairs,
            product(&[covered[pair[0].index()], covered[pair[1].index()]])?,
        ])?;
        if covered[pair[0].index()] > 0 {
            direct_targets[pair[0].index()] =
                sum(&[direct_targets[pair[0].index()], covered[pair[1].index()]])?;
        }
    }
    let mut policies = 0usize;
    let (mut direct_founders, mut first_founder_date) = ([0usize; 4], [0usize; 4]);
    for (index, founder) in request.founders.iter().enumerate() {
        cancel.poll()?;
        if founder.subject >= k
            || founder.date >= t
            || !rank_valid(founder.rank, n)
            || founder.policies.is_empty()
            || founder
                .policies
                .iter()
                .enumerate()
                .any(|(i, policy)| founder.policies[..i].contains(policy))
            || request.founders[..index].iter().any(|old| {
                (old.subject, old.date, old.lens, old.rank)
                    == (founder.subject, founder.date, founder.lens, founder.rank)
            })
        {
            return Err(invalid(
                "valid unique founder coordinates/ranks and unique nonempty policies required",
            ));
        }
        policies = sum(&[policies, founder.policies.len()])?;
        let lens = founder.lens.index();
        if direct_founders[lens] == 0 {
            first_founder_date[lens] = founder.date;
        } else {
            first_founder_date[lens] = first_founder_date[lens].min(founder.date);
        }
        direct_founders[lens] += 1;
    }
    let blocks = if let Projection::SelectedEdges { blocks } = &request.projection {
        for (index, block) in blocks.iter().enumerate() {
            cancel.poll()?;
            if block.subject >= k
                || block.from_date >= block.to_date
                || block.to_date >= t
                || blocks[..index].contains(block)
            {
                return Err(invalid(
                    "valid unique selected subject/date/lens blocks required",
                ));
            }
        }
        blocks.len()
    } else {
        0
    };
    shape_preflight(
        &Shape {
            features: n,
            dates: t,
            subjects: k,
            lenses: request.lenses.len(),
            max_lag: request.scope.max_lag,
            lens_pairs: request.scope.lens_travel.len(),
            rank_pairs,
            founders: request.founders.len(),
            founder_policies: policies,
            direct_founders,
            first_founder_date,
            direct_targets,
            selected_blocks: blocks,
            full_edges: matches!(request.projection, Projection::Full),
            wire_bytes: 0,
        },
        &request.limits,
    )
}

#[cfg(test)]
pub(crate) fn test_request() -> Request {
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
}
