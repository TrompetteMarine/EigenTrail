//! Request-bound rolling numerical proposals. The independent checker certifies reports.
use super::{
    Lens, Preflight, Projection, Request, SCHEMA, Uncertainty,
    angles::{self, CheckedFrame, ComparisonKind, NormWitness},
    founders, integrity,
    report::*,
    validate_structure,
    views::{self, PreparedDate},
};
use crate::{
    Error, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    scalar::point,
    serialization,
};

pub const METHOD: &str = "rolling-lineage-i001-r3-draft-f-conservative";
pub const ARITHMETIC_RECIPE: &str = "rolling-scalar-outward-v1";
pub const BACKEND: &str = "inari-2.0.0/faer-0.24.4";
pub const RESOURCE_SCOPE: &str =
    "shared managed allocations; bounded subject/lag frames; no process-RSS guarantee";

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
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| Error::Resource(String::new()))
}
fn bump(counts: &mut Counts, verdict: Verdict) -> Result<()> {
    counts.candidates = add(counts.candidates, 1)?;
    let value = match verdict {
        Verdict::Agreement => &mut counts.agreement,
        Verdict::Exclusion => &mut counts.exclusion,
        Verdict::Open => &mut counts.open,
        Verdict::Unavailable => &mut counts.unavailable,
    };
    *value = add(*value, 1)?;
    Ok(())
}
fn bump_kind(counts: &mut CountsByKind, kind: EdgeType, verdict: Verdict) -> Result<()> {
    bump(
        match kind {
            EdgeType::LineLine => &mut counts.line,
            EdgeType::EqualBlock => &mut counts.equal_block,
            EdgeType::Containment => &mut counts.containment,
        },
        verdict,
    )
}
fn outcome(counts: Counts) -> WindowOutcome {
    if counts.agreement > 0 {
        WindowOutcome::Agreement
    } else if counts.open > 0 {
        WindowOutcome::Open
    } else if counts.candidates > 0 && counts.exclusion == counts.candidates {
        WindowOutcome::CompleteBreak
    } else {
        WindowOutcome::Unavailable
    }
}
fn classify(bound: &Enclosure, tolerance: f64) -> Verdict {
    match bound {
        Enclosure::Bounds { range } if range[1] <= tolerance => Verdict::Agreement,
        Enclosure::Bounds { range } if range[0] > tolerance => Verdict::Exclusion,
        Enclosure::Bounds { .. } => Verdict::Open,
        _ => Verdict::Unavailable,
    }
}
fn edge_type(a: &GroupRecord, b: &GroupRecord) -> (EdgeType, SmallerSide) {
    let (a, b) = (a.rank[1] - a.rank[0], b.rank[1] - b.rank[0]);
    if a == 1 && b == 1 {
        (EdgeType::LineLine, SmallerSide::None)
    } else if a == b {
        (EdgeType::EqualBlock, SmallerSide::None)
    } else if a < b {
        (EdgeType::Containment, SmallerSide::Source)
    } else {
        (EdgeType::Containment, SmallerSide::Target)
    }
}

struct Frames<'a> {
    root: CheckedFrame<'a>,
    correlation: Option<CheckedFrame<'a>>,
    precision: Option<CheckedFrame<'a>>,
}
impl<'a> Frames<'a> {
    fn new(evidence: &'a DateEvidence, cancel: &Cancellation) -> Result<Self> {
        let frame = |state: &'a FrameState| match state {
            FrameState::Available { proof } => {
                CheckedFrame::new(&proof.eigensystem, cancel).map(Some)
            }
            _ => Ok(None),
        };
        Ok(Self {
            root: CheckedFrame::new(&evidence.root, cancel)?,
            correlation: frame(&evidence.correlation)?,
            precision: frame(&evidence.normalised_precision)?,
        })
    }
    fn get(&self, reference: Reference) -> Option<&CheckedFrame<'a>> {
        match reference {
            Reference::Root => Some(&self.root),
            Reference::Correlation => self.correlation.as_ref(),
            Reference::NormalisedPrecision => self.precision.as_ref(),
        }
    }
}

struct Comparison {
    edge: EdgeRecord,
    norm: NormWitness,
}
impl HeapBytes for Comparison {
    fn heap_bytes(&self) -> Result<usize> {
        self.norm.heap_bytes()
    }
}

fn unsupported(view: &ViewGroups, group: &GroupRecord) -> Option<Reason> {
    match view.region {
        RegionState::Unsupported { reason } => Some(reason),
        _ if group.grouping == GroupingCause::Unsupported => Some(Reason::UnsupportedTransfer),
        _ => None,
    }
}

fn comparison(
    request: &Request,
    source: (&ViewGroups, &GroupRecord, &Frames<'_>),
    target: (&ViewGroups, &GroupRecord, &Frames<'_>),
    ordinal: usize,
    cancel: &Cancellation,
) -> Result<Comparison> {
    memory::scope(|| {
        cancel.poll()?;
        let (a_view, a, a_frames) = source;
        let (b_view, b, b_frames) = target;
        let (kind, side) = edge_type(a, b);
        let ambient = a.rank[1] - a.rank[0] == request.features.len()
            || b.rank[1] - b.rank[0] == request.features.len();
        let unsupported = unsupported(a_view, a).or_else(|| unsupported(b_view, b));
        let (point_bound, norm) = if ambient {
            ([0., 0.], NormWitness::None)
        } else if unsupported.is_some() {
            ([0., 90.], NormWitness::None)
        } else {
            let first = a_frames
                .get(a_view.reference)
                .ok_or_else(|| invalid("rolling source reference unavailable"))?;
            let second = b_frames
                .get(b_view.reference)
                .ok_or_else(|| invalid("rolling target reference unavailable"))?;
            let evidence = angles::compare(first, a.columns, second, b.columns, cancel)?;
            let reported_kind = match evidence.kind {
                ComparisonKind::LineLine => EdgeType::LineLine,
                ComparisonKind::EqualBlock => EdgeType::EqualBlock,
                ComparisonKind::Containment => EdgeType::Containment,
            };
            if reported_kind != kind {
                return Err(invalid("rolling angle and group comparison kinds differ"));
            }
            (evidence.point_deg, evidence.norm)
        };
        let point_enclosure = Enclosure::Bounds { range: point_bound };
        // Point/polar angle and numerical/population projector radii describe
        // different terms. Each group's full radius enters exactly once here.
        let geometry = if ambient {
            Enclosure::Bounds { range: [0., 0.] }
        } else if let Some(reason) = unsupported {
            Enclosure::Undefined { reason }
        } else {
            let radius = point(a.angular_radius_deg) + point(b.angular_radius_deg);
            let lower = (point(point_bound[0]) - radius).inf().max(0.);
            let upper = (point(point_bound[1]) + radius).sup().min(90.);
            if !lower.is_finite() || !upper.is_finite() || lower > upper {
                return Err(invalid(
                    "rolling angular widening is not finite and ordered",
                ));
            }
            Enclosure::Bounds {
                range: [lower + 0., upper + 0.],
            }
        };
        let method_bracket = if let Some(reason) = unsupported {
            // Even an analytically trivial ambient projector does not turn a
            // failed transfer into a supported regional certificate.
            Enclosure::Undefined { reason }
        } else if matches!(request.uncertainty, Uncertainty::Conditional { .. })
            && (a.rank[1] - a.rank[0] == 1) != (b.rank[1] - b.rank[0] == 1)
        {
            // Frozen paper bracket: distinguish it from true ambient containment.
            Enclosure::Bounds { range: [0., 90.] }
        } else {
            geometry.clone()
        };
        let verdict = classify(&method_bracket, request.tolerance_deg);
        Ok(Comparison {
            edge: EdgeRecord {
                ordinal,
                source: a.node,
                target: b.node,
                edge_type: kind,
                smaller_side: side,
                point: point_enclosure,
                geometry,
                method_bracket,
                verdict,
            },
            norm,
        })
    })
}

fn sparse(norm: NormWitness, stream: CandidateStream, ordinal: usize) -> Option<SparseNormWitness> {
    let (kind, matrix, eigensystem, entry_error) = match norm {
        NormWitness::None => return None,
        NormWitness::Symmetric {
            midpoint,
            eigensystem,
            entry_error,
        } => (NormKind::Difference, midpoint, eigensystem, entry_error),
        NormWitness::Gram {
            midpoint,
            eigensystem,
            entry_error,
        } => (NormKind::Gram, midpoint, eigensystem, entry_error),
    };
    Some(SparseNormWitness {
        stream,
        ordinal,
        kind,
        matrix,
        entry_error,
        eigensystem,
    })
}

struct Output {
    dates: Vec<DateEvidence>,
    partitions: Vec<GroupPartition>,
    views: Vec<ViewGroups>,
    norms: Vec<SparseNormWitness>,
    edges: Vec<EdgeRecord>,
    windows: Vec<WindowSummary>,
    candidates: Vec<CandidateBatch>,
}
impl Output {
    fn new() -> Self {
        Self {
            dates: Vec::new(),
            partitions: Vec::new(),
            views: Vec::new(),
            norms: Vec::new(),
            edges: Vec::new(),
            windows: Vec::new(),
            candidates: Vec::new(),
        }
    }
    fn selected(
        request: &Request,
        subject: usize,
        from: usize,
        to: usize,
        pair: [Lens; 2],
    ) -> bool {
        match &request.projection {
            Projection::Full => true,
            Projection::Counts => false,
            Projection::SelectedEdges { blocks } => blocks.iter().any(|block| {
                block.subject == subject
                    && block.from_date == from
                    && block.to_date == to
                    && block.source_lens == pair[0]
                    && block.target_lens == pair[1]
            }),
        }
    }
    fn keep_view(request: &Request, view: &ViewGroups) -> bool {
        match &request.projection {
            Projection::Full => true,
            Projection::Counts => !request.founders.is_empty(),
            Projection::SelectedEdges { blocks } => {
                !request.founders.is_empty()
                    || blocks.iter().any(|b| {
                        b.subject == view.subject
                            && (b.from_date == view.date && b.source_lens == view.lens
                                || b.to_date == view.date && b.target_lens == view.lens)
                    })
            }
        }
    }
    fn date(
        &mut self,
        request: &Request,
        prepared: PreparedDate,
        cancel: &Cancellation,
    ) -> Result<()> {
        for view in prepared.views {
            cancel.poll()?;
            let mut ranks = memory::capacity(view.groups.len())?;
            for group in &view.groups {
                ranks.push(group.rank);
            }
            let first_node = view.groups.first().map_or(prepared.next_node, |g| g.node);
            memory::push(
                &mut self.partitions,
                GroupPartition {
                    subject: view.subject,
                    date: view.date,
                    lens: view.lens,
                    first_node,
                    state: PartitionState::Groups { ranks },
                },
            )?;
            if Self::keep_view(request, &view) {
                memory::push(&mut self.views, view)?;
            }
        }
        memory::push(&mut self.dates, prepared.evidence)
    }
}

struct ActiveDate {
    prepared: PreparedDate,
    founder: bool,
}
struct DirectObservation {
    query: usize,
    edge: EdgeRecord,
    norm: NormWitness,
}

struct Run<'a> {
    request: &'a Request,
    cancel: &'a Cancellation,
    request_hash: String,
    output: Output,
    founders: founders::State<'a>,
    graph_hash: integrity::CandidateHasher,
    direct_hash: integrity::CandidateHasher,
    graph_counts: Counts,
    direct_counts: Counts,
    graph_kinds: CountsByKind,
    direct_kinds: CountsByKind,
    extra_work: usize,
}
impl Run<'_> {
    fn direct_origins(
        &self,
        subject: usize,
        from: usize,
        lens: Lens,
        rank: [usize; 2],
    ) -> impl Iterator<Item = usize> + '_ {
        self.request
            .founders
            .iter()
            .enumerate()
            .filter_map(move |(index, founder)| {
                (founder.subject == subject
                    && founder.date == from
                    && founder.lens == lens
                    && founder.rank == rank)
                    .then_some(index)
            })
    }
    fn graph_window(
        &mut self,
        source: &PreparedDate,
        target: &PreparedDate,
        direct: &mut Vec<DirectObservation>,
    ) -> Result<()> {
        let cancel = self.cancel;
        let subject = source.evidence.subject;
        let from = source.evidence.date;
        let to = target.evidence.date;
        let a_frames = Frames::new(&source.evidence, cancel)?;
        let b_frames = Frames::new(&target.evidence, cancel)?;
        let first = self.graph_counts.candidates;
        let mut window_hash = integrity::CandidateHasher::new(
            &self.request_hash,
            CandidateStream::Graph,
            Some([subject, from, to]),
        )?;
        let mut window_counts = Counts::default();
        let mut decisions = Vec::new();
        let mut lens_pairs = Vec::new();
        for a in &source.views {
            for b in &target.views {
                cancel.poll()?;
                if !self.request.scope.lens_travel.contains(&[a.lens, b.lens]) {
                    continue;
                }
                let mut counts = Counts::default();
                for a_group in a.groups.iter().filter(|g| g.eligible) {
                    for b_group in b.groups.iter().filter(|g| g.eligible) {
                        let ordinal = self.graph_counts.candidates;
                        if add(ordinal, self.extra_work)? >= self.request.limits.max_candidates {
                            return Err(resource(
                                "rolling candidate cap exceeded; no exclusion certified",
                            ));
                        }
                        let computed = comparison(
                            self.request,
                            (a, a_group, &a_frames),
                            (b, b_group, &b_frames),
                            ordinal,
                            cancel,
                        )?;
                        let edge = computed.edge;
                        self.graph_hash
                            .push(ordinal, edge.source, edge.target, edge.verdict)?;
                        window_hash.push(ordinal, edge.source, edge.target, edge.verdict)?;
                        bump(&mut self.graph_counts, edge.verdict)?;
                        bump_kind(&mut self.graph_kinds, edge.edge_type, edge.verdict)?;
                        bump(&mut window_counts, edge.verdict)?;
                        bump(&mut counts, edge.verdict)?;
                        memory::push(&mut decisions, edge.verdict)?;
                        self.founders.edge(&edge, cancel)?;
                        // A graph comparison is numerically reused by matching direct queries.
                        for query in self.direct_origins(subject, from, a.lens, a_group.rank) {
                            memory::push(
                                direct,
                                DirectObservation {
                                    query,
                                    edge: edge.clone(),
                                    norm: NormWitness::None,
                                },
                            )?;
                        }
                        if let Some(norm) = sparse(computed.norm, CandidateStream::Graph, ordinal) {
                            memory::push(&mut self.output.norms, norm)?;
                        }
                        if Output::selected(self.request, subject, from, to, [a.lens, b.lens]) {
                            if self.output.edges.len() >= self.request.limits.max_edge_records {
                                return Err(resource(
                                    "rolling projected edge cap exceeded; no exclusion certified",
                                ));
                            }
                            memory::push(&mut self.output.edges, edge)?;
                        }
                    }
                }
                memory::push(
                    &mut lens_pairs,
                    LensWindow {
                        pair: [a.lens, b.lens],
                        counts,
                        outcome: outcome(counts),
                    },
                )?;
            }
        }
        let packed = PackedDecisions::pack(&decisions, cancel)?;
        let mut batch = CandidateBatch {
            stream: CandidateStream::Graph,
            first,
            decisions: packed,
            digest: String::new(),
        };
        batch.digest = integrity::candidate_batch_digest(&self.request_hash, &batch, cancel)?;
        memory::push(&mut self.output.candidates, batch)?;
        memory::push(
            &mut self.output.windows,
            WindowSummary {
                subject,
                from_date: from,
                to_date: to,
                graph_range: [first, self.graph_counts.candidates],
                global_counts: window_counts,
                global_outcome: outcome(window_counts),
                lens_pairs,
                complete: true,
                candidate_digest: window_hash.finish()?,
            },
        )
    }
    fn missing_window(&mut self, subject: usize, from: usize, to: usize) -> Result<()> {
        let mut lens_pairs = Vec::new();
        for &a in &self.request.lenses {
            for &b in &self.request.lenses {
                if self.request.scope.lens_travel.contains(&[a, b]) {
                    memory::push(
                        &mut lens_pairs,
                        LensWindow {
                            pair: [a, b],
                            counts: Counts::default(),
                            outcome: WindowOutcome::Unavailable,
                        },
                    )?;
                }
            }
        }
        let ordinal = self.graph_counts.candidates;
        let mut batch = CandidateBatch {
            stream: CandidateStream::Graph,
            first: ordinal,
            decisions: PackedDecisions {
                count: 0,
                hex: String::new(),
            },
            digest: String::new(),
        };
        batch.digest = integrity::candidate_batch_digest(&self.request_hash, &batch, self.cancel)?;
        memory::push(&mut self.output.candidates, batch)?;
        memory::push(
            &mut self.output.windows,
            WindowSummary {
                subject,
                from_date: from,
                to_date: to,
                graph_range: [ordinal, ordinal],
                global_counts: Counts::default(),
                global_outcome: WindowOutcome::Unavailable,
                lens_pairs,
                complete: true,
                candidate_digest: integrity::CandidateHasher::new(
                    &self.request_hash,
                    CandidateStream::Graph,
                    Some([subject, from, to]),
                )?
                .finish()?,
            },
        )
    }
    fn direct_date(
        &mut self,
        target: &PreparedDate,
        active: &[ActiveDate],
        mut observations: Vec<DirectObservation>,
        lag: usize,
    ) -> Result<()> {
        let cancel = self.cancel;
        let to = target.evidence.date;
        let subject = target.evidence.subject;
        let target_frames = Frames::new(&target.evidence, cancel)?;
        // Only extra, beyond-lag direct comparisons require new numerical work.
        for (query, founder) in self.request.founders.iter().enumerate() {
            cancel.poll()?;
            if founder.subject != subject || founder.date >= to || to - founder.date <= lag {
                continue;
            }
            let Some(source) = active
                .iter()
                .find(|a| a.prepared.evidence.date == founder.date)
            else {
                continue;
            };
            let Some(a) = source
                .prepared
                .views
                .iter()
                .find(|v| v.lens == founder.lens)
            else {
                continue;
            };
            let Some(a_group) = a
                .groups
                .iter()
                .find(|g| g.eligible && g.rank == founder.rank)
            else {
                continue;
            };
            let source_frames = Frames::new(&source.prepared.evidence, cancel)?;
            for b in &target.views {
                if !self.request.scope.lens_travel.contains(&[a.lens, b.lens]) {
                    continue;
                }
                for b_group in b.groups.iter().filter(|g| g.eligible) {
                    if add(self.graph_counts.candidates, self.extra_work)?
                        >= self.request.limits.max_candidates
                    {
                        return Err(resource(
                            "rolling direct work cap exceeded; no exclusion certified",
                        ));
                    }
                    let computed = comparison(
                        self.request,
                        (a, a_group, &source_frames),
                        (b, b_group, &target_frames),
                        0,
                        cancel,
                    )?;
                    self.extra_work = add(self.extra_work, 1)?;
                    memory::push(
                        &mut observations,
                        DirectObservation {
                            query,
                            edge: computed.edge,
                            norm: computed.norm,
                        },
                    )?;
                }
            }
        }
        observations.sort_unstable_by_key(|o| (o.query, o.edge.target));
        let first = self.direct_counts.candidates;
        let mut decisions = Vec::new();
        for mut observation in observations {
            let ordinal = self.direct_counts.candidates;
            if let Some(norm) = sparse(observation.norm, CandidateStream::Direct, ordinal) {
                memory::push(&mut self.output.norms, norm)?;
            }
            observation.edge.ordinal = ordinal;
            let edge = &observation.edge;
            self.direct_hash
                .push(ordinal, edge.source, edge.target, edge.verdict)?;
            self.founders.direct(observation.query, edge, cancel)?;
            bump(&mut self.direct_counts, edge.verdict)?;
            bump_kind(&mut self.direct_kinds, edge.edge_type, edge.verdict)?;
            memory::push(&mut decisions, edge.verdict)?;
        }
        if !decisions.is_empty() {
            let mut batch = CandidateBatch {
                stream: CandidateStream::Direct,
                first,
                decisions: PackedDecisions::pack(&decisions, cancel)?,
                digest: String::new(),
            };
            batch.digest = integrity::candidate_batch_digest(&self.request_hash, &batch, cancel)?;
            memory::push(&mut self.output.candidates, batch)?;
        }
        Ok(())
    }
}

struct ChunkWriter<'a> {
    chunks: Vec<Chunk>,
    first: usize,
    previous: String,
    request_hash: &'a str,
    record_limit: usize,
    cancel: &'a Cancellation,
}
impl ChunkWriter<'_> {
    fn push<T>(&mut self, rows: Vec<T>, kind: fn(Vec<T>) -> RecordBatch) -> Result<()> {
        let mut rows = rows.into_iter();
        loop {
            self.cancel.poll()?;
            let remaining = rows.len();
            if remaining == 0 {
                break;
            }
            let mut batch = memory::capacity(remaining.min(self.record_limit))?;
            for row in rows.by_ref().take(self.record_limit) {
                batch.push(row);
            }
            let length = batch.len();
            let mut chunk = Chunk {
                index: self.chunks.len(),
                first_record: self.first,
                prev_digest: memory::text(&self.previous)?,
                records: kind(batch),
                digest: String::new(),
            };
            chunk.digest = integrity::chunk_digest(self.request_hash, &chunk, self.cancel)?;
            self.previous = memory::text(&chunk.digest)?;
            self.first = add(self.first, length)?;
            memory::push(&mut self.chunks, chunk)?;
        }
        Ok(())
    }
}

fn finish(mut run: Run<'_>, preflight: Preflight) -> Result<Report> {
    let cancel = run.cancel;
    let (founders, paths) = run.founders.finish(run.request, cancel)?;
    // Evidence may leave its lag window after the comparisons using it. Final
    // stable typed ordering is independent of that lifetime and allocation order.
    run.output
        .dates
        .sort_unstable_by_key(|d| (d.subject, d.date));
    run.output
        .partitions
        .sort_unstable_by_key(|v| (v.subject, v.date, v.lens));
    run.output
        .views
        .sort_unstable_by_key(|v| (v.subject, v.date, v.lens));
    run.output.norms.sort_unstable_by_key(|n| {
        (
            match n.stream {
                CandidateStream::Graph => 0,
                CandidateStream::Direct => 1,
            },
            n.ordinal,
        )
    });
    run.output.edges.sort_unstable_by_key(|e| e.ordinal);
    let mut writer = ChunkWriter {
        chunks: Vec::new(),
        first: 0,
        previous: integrity::chunk_seed(&run.request_hash, cancel)?,
        request_hash: &run.request_hash,
        record_limit: run.request.limits.chunk_records,
        cancel,
    };
    macro_rules! batch {
        ($rows:expr,$kind:ident) => {
            writer.push($rows, RecordBatch::$kind)?
        };
    }
    batch!(run.output.dates, DateEvidence);
    batch!(run.output.partitions, Partitions);
    batch!(run.output.views, Views);
    batch!(run.output.norms, Norms);
    batch!(run.output.edges, Edges);
    batch!(run.output.windows, Windows);
    batch!(run.output.candidates, Candidates);
    batch!(founders, Founders);
    batch!(paths, Paths);
    let ChunkWriter {
        chunks: output,
        first,
        previous,
        ..
    } = writer;
    let completion = Completion {
        status: CompletionStatus::Complete,
        chunks: output.len(),
        records: first,
        terminal_digest: previous,
        graph_digest: run.graph_hash.finish()?,
        direct_digest: run.direct_hash.finish()?,
        graph_counts: run.graph_counts,
        direct_counts: run.direct_counts,
        graph_kind_counts: run.graph_kinds,
        direct_kind_counts: run.direct_kinds,
    };
    Ok(Report {
        schema: memory::text(SCHEMA)?,
        request_hash: run.request_hash,
        protocol: Protocol {
            method: memory::text(METHOD)?,
            arithmetic_recipe: memory::text(ARITHMETIC_RECIPE)?,
            backend: memory::text(BACKEND)?,
            format: memory::text("binary64")?,
            angle_unit: memory::text("degrees")?,
        },
        scope: ReportScope {
            horizon: [0, run.request.dates.len() - 1],
            graph_lag: preflight.lag,
            direct_scope: memory::text(&run.request.scope.direct_scope)?,
            all_simple_bound: preflight.all_simple_bound,
            graph_ceiling: preflight.eligible_ceiling,
            direct_ceiling: preflight.direct_ceiling,
            direct_extra_ceiling: preflight.direct_extra_ceiling,
            work_ceiling: preflight.work_ceiling,
        },
        chunks: output,
        completion,
        managed_reservation: preflight.managed_reservation,
        resource_qualification: memory::text(RESOURCE_SCOPE)?,
    })
}

/// Propose a complete observed-date graph. Success is not a substitute for the
/// independent checker, and refusals never return a partial certificate.
pub fn analyze(request: &Request, cancel: &Cancellation) -> Result<Report> {
    cancel.poll()?;
    let preflight = validate_structure(request, cancel)?;
    memory::ensure_operation(request.limits.max_bytes, request.heap_bytes()?, || {
        let request_hash = serialization::digest_with_cancel(request, cancel)?;
        let mut run = Run {
            request,
            cancel,
            graph_hash: integrity::CandidateHasher::new(
                &request_hash,
                CandidateStream::Graph,
                None,
            )?,
            direct_hash: integrity::CandidateHasher::new(
                &request_hash,
                CandidateStream::Direct,
                None,
            )?,
            request_hash,
            output: Output::new(),
            founders: founders::State::new(request, cancel)?,
            graph_counts: Counts::default(),
            direct_counts: Counts::default(),
            graph_kinds: CountsByKind::default(),
            direct_kinds: CountsByKind::default(),
            extra_work: 0,
        };
        let mut next_node = 0;
        for subject in 0..request.subjects.len() {
            let mut active: Vec<ActiveDate> = Vec::new();
            for date in 0..request.dates.len() {
                cancel.poll()?;
                let prepared = views::prepare_date(request, subject, date, next_node, cancel)?;
                if let Some(prepared) = prepared {
                    next_node = prepared.next_node;
                    for view in &prepared.views {
                        run.founders.register_view(view, cancel)?;
                    }
                    let founder = request
                        .founders
                        .iter()
                        .any(|f| f.subject == subject && f.date == date);
                    memory::push(&mut active, ActiveDate { prepared, founder })?;
                }
                let target = active.iter().find(|a| a.prepared.evidence.date == date);
                let mut direct = Vec::new();
                for lag in 1..=preflight.lag.min(date) {
                    let from = date - lag;
                    let source = active.iter().find(|a| a.prepared.evidence.date == from);
                    match (source, target) {
                        (Some(source), Some(target)) => {
                            run.graph_window(&source.prepared, &target.prepared, &mut direct)?
                        }
                        _ => run.missing_window(subject, from, date)?,
                    }
                }
                if let Some(target) = target {
                    run.direct_date(&target.prepared, &active, direct, preflight.lag)?;
                }
                let mut index = 0;
                while index < active.len() {
                    let at = &active[index];
                    if !at.founder && date - at.prepared.evidence.date >= preflight.lag {
                        let expired = active.swap_remove(index);
                        run.output.date(request, expired.prepared, cancel)?;
                    } else {
                        index += 1;
                    }
                }
            }
            for remaining in active {
                run.output.date(request, remaining.prepared, cancel)?;
            }
        }
        finish(run, preflight)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Matrix,
        resources::memory::Operation,
        rolling_lineage::{Date, Slot, Uncertainty},
    };

    fn request() -> Request {
        super::super::test_request()
    }
    fn edges(report: &Report) -> impl Iterator<Item = &EdgeRecord> {
        report.chunks.iter().flat_map(|chunk| match &chunk.records {
            RecordBatch::Edges(rows) => rows.as_slice(),
            _ => &[],
        })
    }
    fn diagonal(a: f64, b: f64) -> Slot {
        Slot::Observed {
            covariance: Matrix {
                n: 2,
                data: vec![a, 0., 0., b],
            },
        }
    }
    #[test]
    fn diagonal_graph_direct_reach_and_canonical_output_are_deterministic() {
        let request = request();
        let cancel = Cancellation::default();
        let report = analyze(&request, &cancel).unwrap();
        assert_eq!(
            report.request_hash,
            serialization::digest(&request).unwrap()
        );
        assert_eq!(
            report.completion.graph_counts,
            Counts {
                candidates: 4,
                agreement: 2,
                exclusion: 2,
                open: 0,
                unavailable: 0
            }
        );
        assert_eq!(
            report.completion.direct_counts,
            Counts {
                candidates: 2,
                agreement: 1,
                exclusion: 1,
                open: 0,
                unavailable: 0
            }
        );
        assert_eq!(edges(&report).count(), 4);
        assert!(
            report
                .chunks
                .iter()
                .all(|c| c.records.len() <= request.limits.chunk_records)
        );
        let mut previous = integrity::chunk_seed(&report.request_hash, &cancel).unwrap();
        let mut first = 0;
        for chunk in &report.chunks {
            assert_eq!(chunk.prev_digest, previous);
            assert_eq!(chunk.first_record, first);
            assert_eq!(
                chunk.digest,
                integrity::chunk_digest(&report.request_hash, chunk, &cancel).unwrap()
            );
            first += chunk.records.len();
            previous = chunk.digest.clone();
        }
        assert_eq!(previous, report.completion.terminal_digest);
        assert_eq!(first, report.completion.records);
        let bytes = serialization::encode_rolling_report(&report).unwrap();
        let again = analyze(&request, &cancel).unwrap();
        assert_eq!(bytes, serialization::encode_rolling_report(&again).unwrap());
        let decoded = Operation::new(request.limits.max_bytes)
            .unwrap()
            .run(|| serialization::decode_rolling_report_in(&bytes, &cancel))
            .unwrap();
        assert_eq!(report, decoded);
        let targets = report.chunks.iter().flat_map(|c| match &c.records {
            RecordBatch::Founders(r) => r.as_slice(),
            _ => &[],
        });
        assert!(targets.into_iter().any(|row|matches!(row,FounderRecord::Target{node:2,families,..} if families.iter().any(|f|f.family==BottleneckFamily::TypedBlockGraph && f.upper==Bottleneck::Bound{degrees:0.}))));
    }
    #[test]
    fn counts_projection_preserves_coverage_without_edge_payloads_and_cutoff_includes_equality() {
        let mut request = request();
        request.projection = Projection::Counts;
        request.founders.clear();
        request.tolerance_deg = 0.;
        let report = analyze(&request, &Cancellation::default()).unwrap();
        assert_eq!(edges(&report).count(), 0);
        assert_eq!(report.completion.graph_counts.agreement, 2);
        assert_eq!(report.completion.graph_counts.exclusion, 2);
        let labels: usize = report
            .chunks
            .iter()
            .map(|c| match &c.records {
                RecordBatch::Candidates(rows) => rows
                    .iter()
                    .filter(|b| b.stream == CandidateStream::Graph)
                    .map(|b| b.decisions.count)
                    .sum(),
                _ => 0,
            })
            .sum();
        assert_eq!(labels, 4);
        request.tolerance_deg = 90.;
        let report = analyze(&request, &Cancellation::default()).unwrap();
        assert_eq!(report.completion.graph_counts.agreement, 4);
        assert_eq!(report.completion.graph_counts.exclusion, 0);
    }
    #[test]
    fn regional_mixed_bracket_is_distinct_from_ambient_containment() {
        let mut request = request();
        request.subjects[0].slots[1] = diagonal(1., 1.);
        request.uncertainty = Uncertainty::Conditional {
            joint_event: "caller-event".into(),
            provider: "external-thompson-radius".into(),
            rho: 0.1,
        };
        let report = analyze(&request, &Cancellation::default()).unwrap();
        assert_eq!(report.completion.graph_counts.candidates, 2);
        assert_eq!(report.completion.graph_counts.open, 2);
        for edge in edges(&report) {
            assert_eq!(edge.edge_type, EdgeType::Containment);
            assert_eq!(edge.geometry, Enclosure::Bounds { range: [0., 0.] });
            assert_eq!(edge.method_bracket, Enclosure::Bounds { range: [0., 90.] });
        }
    }
    #[test]
    fn four_views_keep_reciprocal_ranks_and_full_ties_as_blocks() {
        let mut request = request();
        request.lenses = vec![
            Lens::Covariance,
            Lens::Precision,
            Lens::Correlation,
            Lens::Partial,
        ];
        request.scope.rank_masks = request
            .lenses
            .iter()
            .map(|&lens| super::super::RankMask {
                lens,
                ranks: vec![[0, 2]],
            })
            .collect();
        request.scope.lens_travel = request
            .lenses
            .iter()
            .flat_map(|&a| request.lenses.iter().map(move |&b| [a, b]))
            .collect();
        request.projection = Projection::Full;
        let operation = Operation::new(request.limits.max_bytes).unwrap();
        let report = operation
            .run(|| analyze(&request, &Cancellation::default()))
            .unwrap_or_else(|error| panic!("{error}; {:?}", operation.stats()));
        assert_eq!(
            report.completion.graph_counts,
            Counts {
                candidates: 36,
                agreement: 28,
                exclusion: 8,
                open: 0,
                unavailable: 0
            }
        );
        assert_eq!(report.completion.direct_counts.candidates, 6);
        let views = report
            .chunks
            .iter()
            .flat_map(|c| match &c.records {
                RecordBatch::Views(rows) => rows.as_slice(),
                _ => &[],
            })
            .collect::<Vec<_>>();
        let covariance = views
            .iter()
            .find(|v| v.date == 0 && v.lens == Lens::Covariance)
            .unwrap();
        let precision = views
            .iter()
            .find(|v| v.date == 1 && v.lens == Lens::Precision)
            .unwrap();
        assert_eq!(precision.groups[1].columns, covariance.groups[0].columns);
        let source = covariance.groups[0].node;
        let target = precision.groups[1].node;
        assert!(
            edges(&report).any(|e| e.source == source
                && e.target == target
                && e.verdict == Verdict::Agreement)
        );
        for view in views
            .iter()
            .filter(|v| matches!(v.lens, Lens::Correlation | Lens::Partial))
        {
            assert_eq!(view.groups.len(), 1);
            assert_eq!(view.groups[0].rank, [0, 2]);
            assert_eq!(view.groups[0].information, InformationLevel::WholeSpace);
        }
    }
    #[test]
    fn failed_region_transfer_keeps_trivial_geometry_separate_from_unavailable_certification() {
        let mut request = request();
        request.founders.clear();
        request.features.push(super::super::Feature {
            id: "third".into(),
            unit: "one".into(),
        });
        request.lenses = vec![Lens::Correlation];
        request.scope.rank_masks = vec![super::super::RankMask {
            lens: Lens::Correlation,
            ranks: vec![[0, 3]],
        }];
        request.scope.lens_travel = vec![[Lens::Correlation, Lens::Correlation]];
        request.subjects[0].slots = vec![
            Slot::Observed {
                covariance: Matrix {
                    n: 3,
                    data: vec![4., 0., 0., 0., 2., 0., 0., 0., 1.]
                }
            };
            2
        ];
        request.uncertainty = Uncertainty::Conditional {
            joint_event: "caller-event".into(),
            provider: "external-thompson-radius".into(),
            rho: 1.,
        };
        request.projection = Projection::Full;
        let operation = Operation::new(request.limits.max_bytes).unwrap();
        let report = operation
            .run(|| analyze(&request, &Cancellation::default()))
            .unwrap_or_else(|error| panic!("{error}; {:?}", operation.stats()));
        assert_eq!(report.completion.graph_counts.candidates, 1);
        assert_eq!(report.completion.graph_counts.unavailable, 1);
        let edge = edges(&report).next().unwrap();
        assert_eq!(edge.geometry, Enclosure::Bounds { range: [0., 0.] });
        assert_eq!(edge.verdict, Verdict::Unavailable);
        assert_eq!(
            edge.method_bracket,
            Enclosure::Undefined {
                reason: Reason::UnsupportedTransfer
            }
        );
    }
    #[test]
    fn missing_slots_keep_original_lag_and_direct_founders_cover_all_later_slots() {
        let mut request = request();
        request.dates.push(Date {
            id: "date2".into(),
            time: 2.,
        });
        request.subjects[0].slots[1] = Slot::Missing {
            reason: "explicit gap".into(),
        };
        request.subjects[0].slots.push(diagonal(3., 1.));
        request.projection = Projection::Counts;
        let report = analyze(&request, &Cancellation::default()).unwrap();
        assert_eq!(report.completion.graph_counts.candidates, 0);
        assert_eq!(report.completion.direct_counts.candidates, 2);
        let windows = report
            .chunks
            .iter()
            .flat_map(|c| match &c.records {
                RecordBatch::Windows(rows) => rows.as_slice(),
                _ => &[],
            })
            .collect::<Vec<_>>();
        assert_eq!(windows.len(), 2);
        assert!(
            windows
                .iter()
                .all(|w| w.global_outcome == WindowOutcome::Unavailable)
        );
        request.scope.max_lag = 2;
        let report = analyze(&request, &Cancellation::default()).unwrap();
        assert_eq!(report.completion.graph_counts.candidates, 4);
        assert_eq!(report.completion.direct_counts.candidates, 2);
        assert_eq!(report.completion.graph_counts.agreement, 2);
    }
    #[test]
    fn refusal_and_cancellation_never_yield_a_successful_partial_report() {
        let mut request = request();
        request.founders.clear();
        request.scope.rank_masks[0].ranks.clear();
        request.limits.max_candidates = 0;
        request.subjects[0].slots[0] = diagonal(-1., 1.);
        assert!(analyze(&request, &Cancellation::default()).is_err());
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(matches!(analyze(&request, &cancel), Err(Error::Cancelled)));
    }
}
