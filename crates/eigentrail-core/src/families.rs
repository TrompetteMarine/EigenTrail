//! Exact symbolic whole-path families; cell outputs are conditional outer information.
use crate::{
    histories, jumps,
    resources::{Cancellation, memory},
    *,
};
use serde::{Deserialize, Serialize};
pub const SCHEMA: &str = "eigentrail.histories.v1";
pub const SCHEMA_V2: &str = "eigentrail.histories.v2";
pub const SCOPE: &str = "exact configuration-indexed PSD whole paths on one common nodal event; query states share a path; marginal cells, block overlaps and local matchings are outer information, not realizability; coverage requires truth in at least one declared configuration";
pub const RESOURCE_SCOPE: &str = "pending audit of native allocations and interruption latency";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // At most 16 configurations; inline models avoid another allocation.
pub enum PathClass {
    Continuous,
    Jumps {
        model: jumps::JumpModel,
    },
    CommonFrame {
        model: common_frame::CommonFrameModel,
    },
    TransverseCrossings {
        windows: Vec<crossings::CrossingWindow>,
    },
    InitializedTransport {
        model: transport_workflow::TransportRequest,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffineWitness {
    pub nodes: Vec<Matrix>,
    pub widening: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub id: String,
    pub lipschitz: f64,
    pub class: PathClass,
    pub witnesses: Vec<AffineWitness>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub schema: String,
    pub observations: TraceRequest,
    pub configurations: Vec<Configuration>,
    pub query_times: Vec<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridNode {
    pub time: f64,
    pub observations: Vec<usize>,
    pub queries: Vec<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CellScope {
    Smooth { segment: usize },
    JumpPossible { cell: usize, mandatory: bool },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionOverlap {
    pub date: usize,
    pub left_block: usize,
    pub right_block: usize,
    pub rank: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Feasibility {
    OriginalAffineWitness,
    OriginalStepWitness,
    WidenedAffineWitness { additional: f64 },
    Unresolved,
    UnsupportedModel,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationReport {
    pub id: String,
    pub class: PathClass,
    pub lipschitz: f64,
    pub grid: Vec<GridNode>,
    pub continuous_conditional: TraceReport,
    pub jumps: Vec<jumps::JumpReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub common_frame: Vec<common_frame::CommonFrameAssessment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub common_frame_queries: Vec<common_frame_queries::Workflow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continuous_workflow: Vec<continuous_queries::Workflow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jump_workflow: Vec<jump_queries::Workflow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crossings: Vec<crossings::CrossingAssessment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transports: Vec<transport_workflow::TransportAssessment>,
    pub cells: Vec<CellScope>,
    pub partition_overlaps: Vec<PartitionOverlap>,
    pub feasibility: Feasibility,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryReport {
    pub schema: String,
    pub request_hash: String,
    pub configurations: Vec<ConfigurationReport>,
    pub scope: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}

pub fn validate_request(request: &HistoryRequest) -> Result<usize> {
    let base = validate(&request.observations)?;
    if ![SCHEMA, SCHEMA_V2].contains(&request.schema.as_str())
        || !(1..=16).contains(&request.configurations.len())
        || request.query_times.len() > 10000
    {
        return Err(Error::Invalid(
            "history schema, 1..16 configurations and <=10000 queries required".into(),
        ));
    }
    let input = &request.observations;
    for &t in &request.query_times {
        if !t.is_finite() || t < input.times[0] || t > *input.times.last().unwrap() {
            return Err(Error::Invalid(
                "finite query inside observation domain required".into(),
            ));
        }
    }
    let mut crossing_reservation = 0usize;
    for (index, cfg) in request.configurations.iter().enumerate() {
        if cfg.id.is_empty()
            || cfg.id.len() > 256
            || request.configurations[..index]
                .iter()
                .any(|old| old.id == cfg.id)
            || cfg.witnesses.len() > 1
        {
            return Err(Error::Invalid(
                "unique bounded configuration IDs and at most one complete witness required".into(),
            ));
        }
        scalar::nonnegative(cfg.lipschitz, "configuration K")?;
        scalar::upper(
            (scalar::point(*input.times.last().unwrap()) - scalar::point(input.times[0]))
                * scalar::point(cfg.lipschitz),
        )?;
        if let PathClass::Jumps { model } = &cfg.class {
            if !model.at_most_one_per_cell {
                return Err(Error::Invalid("unsupported jump occupancy model".into()));
            }
            if let Some(x) = model.minimum_jump {
                scalar::nonnegative(x, "minimum jump")?;
            }
        }
        if let PathClass::CommonFrame { model } = &cfg.class {
            common_frame::validate_model(model, input.times.len())?;
        }
        if let PathClass::TransverseCrossings { windows } = &cfg.class {
            if request.schema != SCHEMA_V2 || !(1..=16).contains(&windows.len()) {
                return Err(Error::Invalid(
                    "history v2 and 1..16 crossing windows required".into(),
                ));
            }
            for window in windows {
                crossing_reservation = crossing_reservation
                    .checked_add(crossings::validate_window(input, window)?)
                    .ok_or_else(|| {
                        Error::Resource("history crossing reservation overflow".into())
                    })?;
            }
        }
        if let PathClass::InitializedTransport { model } = &cfg.class {
            if request.schema != SCHEMA_V2 {
                return Err(Error::Invalid(
                    "initialized transport requires history v2".into(),
                ));
            }
            crossing_reservation = crossing_reservation
                .checked_add(transport_workflow::validate_history(input, model)?)
                .ok_or_else(|| Error::Resource("history transport reservation overflow".into()))?;
        }
        for witness in &cfg.witnesses {
            scalar::nonnegative(witness.widening, "witness widening")?;
            if witness.nodes.len() != input.times.len() {
                return Err(Error::Invalid(
                    "whole observation witness chain required".into(),
                ));
            }
            for node in &witness.nodes {
                numerics::validate_matrix(node)?;
                if node.n != input.features.len() {
                    return Err(Error::Invalid("witness dimension".into()));
                }
            }
        }
    }
    let common_frame_count = request
        .configurations
        .iter()
        .filter(|cfg| {
            matches!(cfg.class, PathClass::CommonFrame { .. })
                || (request.schema == SCHEMA_V2
                    && (cfg.class == PathClass::Continuous
                        || matches!(cfg.class, PathClass::Jumps { .. })))
        })
        .count();
    let reserved = reservation(
        base,
        input.features.len(),
        input.times.len(),
        request.query_times.len(),
        request.configurations.len(),
        common_frame_count,
    )?
    .checked_add(crossing_reservation)
    .ok_or_else(|| Error::Resource("history crossing reservation overflow".into()))?;
    if reserved > input.memory_bytes {
        return Err(Error::Resource(format!(
            "history configurations require reservation {reserved}; limit {}",
            input.memory_bytes
        )));
    }
    Ok(reserved)
}

fn reservation(
    base: usize,
    features: usize,
    dates: usize,
    queries: usize,
    configurations: usize,
    common_frames: usize,
) -> Result<usize> {
    let overhead = dates
        .checked_add(queries)
        .and_then(|v| v.checked_mul(1024))
        .ok_or_else(|| Error::Resource("history grid size overflow".into()))?;
    let query_records = queries
        .checked_mul(features)
        .and_then(|v| v.checked_mul(8192))
        .ok_or_else(|| Error::Resource("history query record overflow".into()))?;
    base.checked_mul(3)
        .and_then(|v| v.checked_add(overhead))
        .and_then(|v| v.checked_mul(configurations))
        .and_then(|v| {
            base.checked_add(query_records)
                .and_then(|extra| extra.checked_mul(common_frames))
                .and_then(|extra| v.checked_add(extra))
        })
        .ok_or_else(|| Error::Resource("history workspace overflow".into()))
}

/// Conservative shape refusal for native common-frame inputs before matrix copies.
pub fn preflight_common_frame(
    features: usize,
    dates: usize,
    queries: usize,
    limit: usize,
) -> Result<usize> {
    if queries > 10000 {
        return Err(Error::Invalid(
            "at most 10000 history queries required".into(),
        ));
    }
    let base = resources::preflight(features, dates, limit)?;
    let reserved = reservation(base, features, dates, queries, 1, 1)?;
    if reserved > limit {
        return Err(Error::Resource(format!(
            "history configurations require reservation {reserved}; limit {limit}"
        )));
    }
    Ok(reserved)
}

fn grid(request: &HistoryRequest, cancel: &Cancellation) -> Result<Vec<GridNode>> {
    let mut entries =
        memory::capacity(request.observations.times.len() + request.query_times.len())?;
    for (i, &time) in request.observations.times.iter().enumerate() {
        entries.push((time, true, i));
    }
    for (i, &time) in request.query_times.iter().enumerate() {
        entries.push((time, false, i));
    }
    entries.sort_unstable_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let mut out: Vec<GridNode> = memory::capacity(entries.len())?;
    for (time, observed, index) in entries {
        cancel.poll()?;
        if out.last().is_none_or(|node| node.time != time) {
            out.push(GridNode {
                time,
                observations: Vec::new(),
                queries: Vec::new(),
            });
        }
        let node = out.last_mut().unwrap();
        if observed {
            memory::push(&mut node.observations, index)?;
        } else {
            memory::push(&mut node.queries, index)?;
        }
    }
    Ok(out)
}

pub fn analyze(request: &HistoryRequest, cancel: &Cancellation) -> Result<HistoryReport> {
    cancel.poll()?;
    validate_request(request)?;
    memory::ensure_operation(
        request.observations.memory_bytes,
        resources::memory::HeapBytes::heap_bytes(request)?,
        || {
            cancel.poll()?;
            let reservation = validate_request(request)?;
            let grid = grid(request, cancel)?;
            let mut configurations = memory::capacity(request.configurations.len())?;
            for cfg in &request.configurations {
                cancel.poll()?;
                let member = memory::scope(|| {
                    let mut scoped = request.observations.try_clone()?;
                    scoped.lipschitz = cfg.lipschitz;
                    let continuous_conditional = track_with_cancel(&scoped, cancel)?;
                    let jumps = match &cfg.class {
                        PathClass::Continuous
                        | PathClass::CommonFrame { .. }
                        | PathClass::InitializedTransport { .. }
                        | PathClass::TransverseCrossings { .. } => Vec::new(),
                        PathClass::Jumps { model } => {
                            let mut out = memory::capacity(1)?;
                            out.push(jumps::analyze_with_cancel(&scoped, model, cancel)?);
                            out
                        }
                    };
                    let common_frame = match &cfg.class {
                        PathClass::CommonFrame { model } => {
                            let mut out = memory::capacity(1)?;
                            out.push(common_frame::assess(&scoped, model, cancel)?);
                            out
                        }
                        _ => Vec::new(),
                    };
                    let common_frame_queries = match &cfg.class {
                        PathClass::CommonFrame { .. } => {
                            let mut out = memory::capacity(1)?;
                            out.push(common_frame_queries::analyze(
                                &scoped,
                                &common_frame[0],
                                &request.query_times,
                                cfg.witnesses.is_empty(),
                                cancel,
                            )?);
                            out
                        }
                        _ => Vec::new(),
                    };
                    let mut crossings = Vec::new();
                    if let PathClass::TransverseCrossings { windows } = &cfg.class {
                        crossings = memory::capacity(windows.len())?;
                        for window in windows {
                            cancel.poll()?;
                            crossings.push(crossings::analyze(&scoped, window, cancel)?);
                        }
                    }
                    let mut cells = memory::capacity(scoped.times.len() - 1)?;
                    let mut transports = Vec::new();
                    if let PathClass::InitializedTransport { model } = &cfg.class {
                        transports = memory::capacity(1)?;
                        transports.push(transport_workflow::analyze_history(
                            &scoped,
                            &continuous_conditional,
                            model,
                            cancel,
                        )?);
                    }
                    for j in 0..scoped.times.len() - 1 {
                        let possible = jumps
                            .first()
                            .is_some_and(|r| r.family.mandatory[j] || r.family.optional[j]);
                        cells.push(if possible {
                            CellScope::JumpPossible {
                                cell: j,
                                mandatory: jumps[0].family.mandatory[j],
                            }
                        } else {
                            CellScope::Smooth { segment: j }
                        });
                    }
                    let mut overlaps = Vec::new();
                    for date in 1..scoped.times.len() - 1 {
                        cancel.poll()?;
                        if !matches!(cells[date - 1], CellScope::Smooth { .. })
                            || !matches!(cells[date], CellScope::Smooth { .. })
                        {
                            continue;
                        }
                        for (a, left) in continuous_conditional.segments[date - 1]
                            .tubes
                            .iter()
                            .enumerate()
                        {
                            for (b, right) in continuous_conditional.segments[date]
                                .tubes
                                .iter()
                                .enumerate()
                            {
                                let rank = left
                                    .end
                                    .min(right.end)
                                    .saturating_sub(left.start.max(right.start));
                                if rank > 0 {
                                    memory::push(
                                        &mut overlaps,
                                        PartitionOverlap {
                                            date,
                                            left_block: a,
                                            right_block: b,
                                            rank,
                                        },
                                    )?;
                                }
                            }
                        }
                    }
                    let mut feasibility = if matches!(cfg.class, PathClass::CommonFrame { .. }) {
                        match cfg.witnesses.first() {
                            Some(witness) => common_frame_queries::explicit_feasibility(
                                &scoped,
                                &continuous_conditional.radii,
                                witness,
                                cancel,
                            )?,
                            None => common_frame_queries[0].witnesses[0].feasibility.clone(),
                        }
                    } else if cfg.class == PathClass::Continuous {
                        let observed = calibration::normalized(&scoped)?;
                        let (nodes, widening) = cfg
                            .witnesses
                            .first()
                            .map_or((observed.as_slice(), 0.), |w| {
                                (w.nodes.as_slice(), w.widening)
                            });
                        match histories::witness(
                            &scoped,
                            &observed,
                            &continuous_conditional.radii,
                            nodes,
                            widening,
                            cancel,
                        )? {
                            histories::Witness::Original => Feasibility::OriginalAffineWitness,
                            histories::Witness::Widened { additional } => {
                                Feasibility::WidenedAffineWitness { additional }
                            }
                            histories::Witness::Unverified => Feasibility::Unresolved,
                        }
                    } else if cfg.witnesses.is_empty() {
                        Feasibility::Unresolved
                    } else {
                        Feasibility::UnsupportedModel
                    };
                    let mut continuous_workflow = Vec::new();
                    let mut jump_workflow = Vec::new();
                    if request.schema == SCHEMA_V2 && matches!(cfg.class, PathClass::Jumps { .. }) {
                        let (workflow, status) = jump_queries::analyze(
                            &scoped,
                            cfg,
                            &continuous_conditional,
                            &jumps[0],
                            &request.query_times,
                            cancel,
                        )?;
                        memory::push(&mut jump_workflow, workflow)?;
                        feasibility = status;
                    }
                    if request.schema == SCHEMA_V2 && cfg.class == PathClass::Continuous {
                        memory::push(
                            &mut continuous_workflow,
                            continuous_queries::analyze(
                                &scoped,
                                cfg,
                                &continuous_conditional,
                                &request.query_times,
                                &feasibility,
                                cancel,
                            )?,
                        )?;
                    }
                    Ok(ConfigurationReport {
                        id: memory::text(&cfg.id)?,
                        class: clone_class(&cfg.class)?,
                        lipschitz: cfg.lipschitz,
                        grid: clone_grid(&grid)?,
                        continuous_conditional,
                        jumps,
                        common_frame,
                        common_frame_queries,
                        continuous_workflow,
                        jump_workflow,
                        crossings,
                        transports,
                        cells,
                        partition_overlaps: overlaps,
                        feasibility,
                    })
                })?;
                configurations.push(member);
            }
            cancel.poll()?;
            Ok(HistoryReport {
                schema: memory::text(&request.schema)?,
                request_hash: serialization::digest_with_cancel(request, cancel)?,
                configurations,
                scope: memory::text(SCOPE)?,
                completed: true,
                managed_reservation: reservation,
                resource_qualification: memory::text(RESOURCE_SCOPE)?,
            })
        },
    )
}

pub(crate) fn clone_class(class: &PathClass) -> Result<PathClass> {
    Ok(match class {
        PathClass::Continuous => PathClass::Continuous,
        PathClass::Jumps { model } => PathClass::Jumps {
            model: model.clone(),
        },
        PathClass::CommonFrame { model } => PathClass::CommonFrame {
            model: common_frame::CommonFrameModel {
                weights: memory::copy(&model.weights)?,
                common_frame_assumed: model.common_frame_assumed,
            },
        },
        PathClass::TransverseCrossings { windows } => PathClass::TransverseCrossings {
            windows: memory::copy(windows)?,
        },
        PathClass::InitializedTransport { model } => PathClass::InitializedTransport {
            model: transport_workflow::try_clone(model)?,
        },
    })
}
fn clone_grid(grid: &[GridNode]) -> Result<Vec<GridNode>> {
    let mut out = memory::capacity(grid.len())?;
    for node in grid {
        out.push(GridNode {
            time: node.time,
            observations: memory::copy(&node.observations)?,
            queries: memory::copy(&node.queries)?,
        });
    }
    Ok(out)
}
