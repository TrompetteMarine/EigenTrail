//! Independently reconstruct every dated candidate, projection and reach claim.
pub mod angles;
pub mod frames;
pub mod reach;
pub mod views;

use eigentrail_core::{
    Error, Result,
    resources::{Cancellation, memory, memory::HeapBytes},
    rolling_lineage::{Lens, Projection, Request, SCHEMA, Slot, Uncertainty, integrity, report::*},
    scalar::point,
    serialization,
};
use frames::VerifiedDate;

fn invalid(message: &str) -> Error {
    memory::text(message)
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn plus(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| Error::Resource(String::new()))
}
fn require(test: bool, message: &str) -> Result<()> {
    if test { Ok(()) } else { Err(invalid(message)) }
}
fn equal_wire<T: serde::Serialize>(actual: &T, expected: &T, cancel: &Cancellation) -> Result<()> {
    memory::scope(|| {
        let bytes = serialization::encode_with_cancel(expected, cancel)?;
        serialization::compare_encoding(actual, &bytes, cancel)
    })
}

#[derive(Default)]
struct Records<'a> {
    dates: Vec<&'a DateEvidence>,
    partitions: Vec<&'a GroupPartition>,
    views: Vec<&'a ViewGroups>,
    norms: Vec<&'a SparseNormWitness>,
    edges: Vec<&'a EdgeRecord>,
    windows: Vec<&'a WindowSummary>,
    batches: Vec<&'a CandidateBatch>,
    founders: Vec<&'a FounderRecord>,
    paths: Vec<&'a SelectedPath>,
}
fn batch_kind(batch: &RecordBatch) -> usize {
    match batch {
        RecordBatch::DateEvidence(_) => 0,
        RecordBatch::Partitions(_) => 1,
        RecordBatch::Views(_) => 2,
        RecordBatch::Norms(_) => 3,
        RecordBatch::Edges(_) => 4,
        RecordBatch::Windows(_) => 5,
        RecordBatch::Candidates(_) => 6,
        RecordBatch::Founders(_) => 7,
        RecordBatch::Paths(_) => 8,
    }
}
fn records<'a>(
    request: &Request,
    report: &'a Report,
    cancel: &Cancellation,
) -> Result<Records<'a>> {
    let mut records = Records::default();
    let mut previous = integrity::chunk_seed(&report.request_hash, cancel)?;
    let mut first = 0;
    let mut last_kind = 0;
    for (index, chunk) in report.chunks.iter().enumerate() {
        cancel.poll()?;
        let kind = batch_kind(&chunk.records);
        require(
            !chunk.records.is_empty()
                && chunk.records.len() <= request.limits.chunk_records
                && chunk.index == index
                && chunk.first_record == first
                && chunk.prev_digest == previous
                && kind >= last_kind,
            "rolling checker: chunk order, size or coverage",
        )?;
        require(
            chunk.digest == integrity::chunk_digest(&report.request_hash, chunk, cancel)?,
            "rolling checker: chunk digest",
        )?;
        last_kind = kind;
        first = plus(first, chunk.records.len())?;
        previous = memory::text(&chunk.digest)?;
        macro_rules! append {
            ($rows:expr,$destination:ident) => {
                for row in $rows {
                    memory::push(&mut records.$destination, row)?;
                }
            };
        }
        match &chunk.records {
            RecordBatch::DateEvidence(rows) => append!(rows, dates),
            RecordBatch::Partitions(rows) => append!(rows, partitions),
            RecordBatch::Views(rows) => append!(rows, views),
            RecordBatch::Norms(rows) => append!(rows, norms),
            RecordBatch::Edges(rows) => append!(rows, edges),
            RecordBatch::Windows(rows) => append!(rows, windows),
            RecordBatch::Candidates(rows) => append!(rows, batches),
            RecordBatch::Founders(rows) => append!(rows, founders),
            RecordBatch::Paths(rows) => append!(rows, paths),
        }
    }
    require(
        report.completion.chunks == report.chunks.len()
            && report.completion.records == first
            && report.completion.terminal_digest == previous,
        "rolling checker: terminal coverage",
    )?;
    require(
        records.edges.len() <= request.limits.max_edge_records,
        "rolling checker: projected edge limit",
    )?;
    let mut last = None;
    for &norm in &records.norms {
        let key = (
            match norm.stream {
                CandidateStream::Graph => 0,
                CandidateStream::Direct => 1,
            },
            norm.ordinal,
        );
        require(
            last.is_none_or(|previous| previous < key),
            "rolling checker: duplicate or reordered sparse witness",
        )?;
        last = Some(key);
    }
    for batch in &records.batches {
        batch.decisions.validate(cancel)?;
        require(
            batch.digest == integrity::candidate_batch_digest(&report.request_hash, batch, cancel)?,
            "rolling checker: candidate batch digest",
        )?;
    }
    Ok(records)
}

struct Norms<'a> {
    rows: &'a [&'a SparseNormWitness],
    graph: usize,
    direct: usize,
    split: usize,
}
impl<'a> Norms<'a> {
    fn new(rows: &'a [&'a SparseNormWitness]) -> Self {
        let split = rows.partition_point(|row| row.stream == CandidateStream::Graph);
        Self {
            rows,
            graph: 0,
            direct: split,
            split,
        }
    }
    fn next(
        &mut self,
        stream: CandidateStream,
        ordinal: usize,
    ) -> Result<Option<&'a SparseNormWitness>> {
        let (index, end) = match stream {
            CandidateStream::Graph => (&mut self.graph, self.split),
            CandidateStream::Direct => (&mut self.direct, self.rows.len()),
        };
        if *index == end {
            return Ok(None);
        }
        let row = self.rows[*index];
        require(
            row.ordinal >= ordinal,
            "rolling checker: unused sparse witness before candidate",
        )?;
        if row.ordinal == ordinal {
            *index += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }
    fn finish(&self) -> Result<()> {
        require(
            self.graph == self.split && self.direct == self.rows.len(),
            "rolling checker: extra sparse witnesses",
        )
    }
}

struct Cached<'a> {
    root: angles::CheckerFrame<'a>,
    correlation: Option<angles::CheckerFrame<'a>>,
    precision: Option<angles::CheckerFrame<'a>>,
}
impl<'a> Cached<'a> {
    fn new(date: &VerifiedDate<'a>, cancel: &Cancellation) -> Result<Self> {
        Ok(Self {
            root: angles::CheckerFrame::new(date.root.eigensystem, cancel)?,
            correlation: date
                .correlation
                .map(|f| angles::CheckerFrame::new(f.eigensystem, cancel))
                .transpose()?,
            precision: date
                .normalised_precision
                .map(|f| angles::CheckerFrame::new(f.eigensystem, cancel))
                .transpose()?,
        })
    }
    fn frame(&self, reference: Reference) -> Option<&angles::CheckerFrame<'a>> {
        match reference {
            Reference::Root => Some(&self.root),
            Reference::Correlation => self.correlation.as_ref(),
            Reference::NormalisedPrecision => self.precision.as_ref(),
        }
    }
}
fn bounds_kind(a: &GroupRecord, b: &GroupRecord) -> (EdgeType, SmallerSide) {
    let x = a.rank[1] - a.rank[0];
    let y = b.rank[1] - b.rank[0];
    if x == y {
        (
            (if x == 1 {
                EdgeType::LineLine
            } else {
                EdgeType::EqualBlock
            }),
            SmallerSide::None,
        )
    } else {
        (
            EdgeType::Containment,
            if x < y {
                SmallerSide::Source
            } else {
                SmallerSide::Target
            },
        )
    }
}
fn unavailable(view: &ViewGroups, group: &GroupRecord) -> Option<Reason> {
    if let RegionState::Unsupported { reason } = view.region {
        Some(reason)
    } else if group.grouping == GroupingCause::Unsupported {
        Some(Reason::UnsupportedTransfer)
    } else {
        None
    }
}
fn reconstruct_edge(
    request: &Request,
    source: (&ViewGroups, &GroupRecord, &Cached<'_>),
    target: (&ViewGroups, &GroupRecord, &Cached<'_>),
    ordinal: usize,
    witness: Option<&SparseNormWitness>,
    cancel: &Cancellation,
) -> Result<EdgeRecord> {
    cancel.poll()?;
    let (a_view, a, a_frames) = source;
    let (b_view, b, b_frames) = target;
    let (kind, smaller_side) = bounds_kind(a, b);
    let n = request.features.len();
    let aq = a.rank[1] - a.rank[0];
    let bq = b.rank[1] - b.rank[0];
    let trivial = aq == n || bq == n;
    let unsupported = unavailable(a_view, a).or_else(|| unavailable(b_view, b));
    let central = if trivial || unsupported.is_some() {
        require(
            witness.is_none(),
            "rolling checker: unused norm for ambient/unsupported comparison",
        )?;
        if trivial { [0., 0.] } else { [0., 90.] }
    } else {
        let first = a_frames
            .frame(a_view.reference)
            .ok_or_else(|| invalid("rolling checker: source frame absent"))?;
        let second = b_frames
            .frame(b_view.reference)
            .ok_or_else(|| invalid("rolling checker: target frame absent"))?;
        angles::check_angle_frames(first, a.columns, second, b.columns, witness, cancel)?
    };
    let geometry = if trivial {
        Enclosure::Bounds { range: [0., 0.] }
    } else if let Some(reason) = unsupported {
        Enclosure::Undefined { reason }
    } else {
        let total = point(a.angular_radius_deg) + point(b.angular_radius_deg);
        let lo = (point(central[0]) - total).inf().max(0.);
        let hi = (point(central[1]) + total).sup().min(90.);
        require(
            lo.is_finite() && hi.is_finite() && lo <= hi,
            "rolling checker: inconsistent angular enclosure",
        )?;
        Enclosure::Bounds {
            range: [lo + 0., hi + 0.],
        }
    };
    let method_bracket = match unsupported {
        Some(reason) => Enclosure::Undefined { reason },
        None if matches!(request.uncertainty, Uncertainty::Conditional { .. })
            && aq.min(bq) == 1
            && aq != bq =>
        {
            Enclosure::Bounds { range: [0., 90.] }
        }
        None => geometry.clone(),
    };
    let verdict = match &method_bracket {
        Enclosure::Bounds { range } => {
            if range[1] <= request.tolerance_deg {
                Verdict::Agreement
            } else if request.tolerance_deg < range[0] {
                Verdict::Exclusion
            } else {
                Verdict::Open
            }
        }
        _ => Verdict::Unavailable,
    };
    Ok(EdgeRecord {
        ordinal,
        source: a.node,
        target: b.node,
        edge_type: kind,
        smaller_side,
        point: Enclosure::Bounds { range: central },
        geometry,
        method_bracket,
        verdict,
    })
}
fn count(counts: &mut Counts, edge: &EdgeRecord) -> Result<()> {
    counts.candidates = plus(counts.candidates, 1)?;
    let counter = match edge.verdict {
        Verdict::Agreement => &mut counts.agreement,
        Verdict::Exclusion => &mut counts.exclusion,
        Verdict::Open => &mut counts.open,
        Verdict::Unavailable => &mut counts.unavailable,
    };
    *counter = plus(*counter, 1)?;
    Ok(())
}
fn kind_count(counts: &mut CountsByKind, edge: &EdgeRecord) -> Result<()> {
    count(
        match edge.edge_type {
            EdgeType::LineLine => &mut counts.line,
            EdgeType::EqualBlock => &mut counts.equal_block,
            EdgeType::Containment => &mut counts.containment,
        },
        edge,
    )
}
fn window_outcome(counts: Counts) -> WindowOutcome {
    if counts.agreement != 0 {
        WindowOutcome::Agreement
    } else if counts.open != 0 {
        WindowOutcome::Open
    } else if counts.candidates != 0 && counts.exclusion == counts.candidates {
        WindowOutcome::CompleteBreak
    } else {
        WindowOutcome::Unavailable
    }
}
fn selected(request: &Request, subject: usize, from: usize, to: usize, pair: [Lens; 2]) -> bool {
    match &request.projection {
        Projection::Full => true,
        Projection::Counts => false,
        Projection::SelectedEdges { blocks } => blocks.iter().any(|b| {
            (
                b.subject,
                b.from_date,
                b.to_date,
                b.source_lens,
                b.target_lens,
            ) == (subject, from, to, pair[0], pair[1])
        }),
    }
}
fn full_view(request: &Request, view: &ViewGroups) -> bool {
    if !request.founders.is_empty() || matches!(request.projection, Projection::Full) {
        return true;
    }
    match &request.projection {
        Projection::SelectedEdges { blocks } => blocks.iter().any(|b| {
            b.subject == view.subject
                && ((b.from_date == view.date && b.source_lens == view.lens)
                    || (b.to_date == view.date && b.target_lens == view.lens))
        }),
        _ => false,
    }
}

struct Date<'a> {
    verified: VerifiedDate<'a>,
    views: Vec<ViewGroups>,
    founder: bool,
}
struct Direct {
    query: usize,
    edge: EdgeRecord,
}
struct Replay<'a> {
    request: &'a Request,
    records: &'a Records<'a>,
    cancel: &'a Cancellation,
    request_hash: &'a str,
    norms: Norms<'a>,
    reach: reach::Reach<'a>,
    batch: usize,
    window: usize,
    edge: usize,
    graph: Counts,
    direct: Counts,
    graph_kinds: CountsByKind,
    direct_kinds: CountsByKind,
    graph_hash: integrity::CandidateHasher,
    direct_hash: integrity::CandidateHasher,
    extra: usize,
}
impl<'a> Replay<'a> {
    fn batch(&self, stream: CandidateStream, first: usize) -> Result<&'a CandidateBatch> {
        let batch = *self
            .records
            .batches
            .get(self.batch)
            .ok_or_else(|| invalid("rolling checker: omitted categorical batch"))?;
        require(
            batch.stream == stream && batch.first == first,
            "rolling checker: categorical stream or coverage start",
        )?;
        Ok(batch)
    }
    fn graph_window(
        &mut self,
        subject: usize,
        from: usize,
        to: usize,
        dates: Option<(&Date<'a>, &Date<'a>)>,
        direct: &mut Vec<Direct>,
    ) -> Result<()> {
        let cancel = self.cancel;
        let first = self.graph.candidates;
        let batch = self.batch(CandidateStream::Graph, first)?;
        let mut digest = integrity::CandidateHasher::new(
            self.request_hash,
            CandidateStream::Graph,
            Some([subject, from, to]),
        )?;
        let mut global = Counts::default();
        let mut pairs = Vec::new();
        let cached = dates
            .map(|(a, b)| {
                Ok((
                    Cached::new(&a.verified, cancel)?,
                    Cached::new(&b.verified, cancel)?,
                ))
            })
            .transpose()?;
        for &a_lens in &self.request.lenses {
            for &b_lens in &self.request.lenses {
                cancel.poll()?;
                if !self.request.scope.lens_travel.contains(&[a_lens, b_lens]) {
                    continue;
                }
                let mut counts = Counts::default();
                if let (Some((source, target)), Some((a_frames, b_frames))) =
                    (dates, cached.as_ref())
                {
                    let a = source
                        .views
                        .iter()
                        .find(|v| v.lens == a_lens)
                        .ok_or_else(|| {
                            invalid("rolling checker: missing reconstructed source view")
                        })?;
                    let b = target
                        .views
                        .iter()
                        .find(|v| v.lens == b_lens)
                        .ok_or_else(|| {
                            invalid("rolling checker: missing reconstructed target view")
                        })?;
                    for ag in a.groups.iter().filter(|g| g.eligible) {
                        for bg in b.groups.iter().filter(|g| g.eligible) {
                            cancel.poll()?;
                            require(
                                plus(self.graph.candidates, self.extra)?
                                    < self.request.limits.max_candidates,
                                "rolling checker: candidate work exceeds cap",
                            )?;
                            let ordinal = self.graph.candidates;
                            let norm = self.norms.next(CandidateStream::Graph, ordinal)?;
                            let edge = reconstruct_edge(
                                self.request,
                                (a, ag, a_frames),
                                (b, bg, b_frames),
                                ordinal,
                                norm,
                                cancel,
                            )?;
                            require(
                                batch.decisions.get(global.candidates)? == edge.verdict,
                                "rolling checker: altered categorical decision",
                            )?;
                            self.graph_hash.push(
                                ordinal,
                                edge.source,
                                edge.target,
                                edge.verdict,
                            )?;
                            digest.push(ordinal, edge.source, edge.target, edge.verdict)?;
                            count(&mut self.graph, &edge)?;
                            kind_count(&mut self.graph_kinds, &edge)?;
                            count(&mut global, &edge)?;
                            count(&mut counts, &edge)?;
                            self.reach.edge(&edge, cancel)?;
                            for (query, founder) in self.request.founders.iter().enumerate() {
                                if (founder.subject, founder.date, founder.lens, founder.rank)
                                    == (subject, from, a_lens, ag.rank)
                                {
                                    memory::push(
                                        direct,
                                        Direct {
                                            query,
                                            edge: edge.clone(),
                                        },
                                    )?;
                                }
                            }
                            if selected(self.request, subject, from, to, [a_lens, b_lens]) {
                                let actual =
                                    self.records.edges.get(self.edge).ok_or_else(|| {
                                        invalid("rolling checker: omitted projected edge")
                                    })?;
                                equal_wire(*actual, &edge, cancel)?;
                                self.edge = plus(self.edge, 1)?;
                            }
                        }
                    }
                }
                memory::push(
                    &mut pairs,
                    LensWindow {
                        pair: [a_lens, b_lens],
                        counts,
                        outcome: window_outcome(counts),
                    },
                )?;
            }
        }
        require(
            batch.decisions.count == global.candidates,
            "rolling checker: truncated or extended categorical batch",
        )?;
        self.batch = plus(self.batch, 1)?;
        let expected = WindowSummary {
            subject,
            from_date: from,
            to_date: to,
            graph_range: [first, self.graph.candidates],
            global_counts: global,
            global_outcome: window_outcome(global),
            lens_pairs: pairs,
            complete: true,
            candidate_digest: digest.finish()?,
        };
        let actual = self
            .records
            .windows
            .get(self.window)
            .ok_or_else(|| invalid("rolling checker: omitted window"))?;
        require(
            **actual == expected,
            "rolling checker: window counts, scope or verdict differ",
        )?;
        self.window = plus(self.window, 1)?;
        Ok(())
    }
    fn direct_edge(
        &mut self,
        query: usize,
        mut edge: EdgeRecord,
        batch: &mut Option<&'a CandidateBatch>,
        first: usize,
    ) -> Result<()> {
        if batch.is_none() {
            *batch = Some(self.batch(CandidateStream::Direct, first)?);
        }
        let ordinal = self.direct.candidates;
        require(
            batch
                .expect("checked direct batch")
                .decisions
                .get(ordinal - first)?
                == edge.verdict,
            "rolling checker: direct categorical decision",
        )?;
        edge.ordinal = ordinal;
        self.direct_hash
            .push(ordinal, edge.source, edge.target, edge.verdict)?;
        self.reach.direct(query, &edge, self.cancel)?;
        count(&mut self.direct, &edge)?;
        kind_count(&mut self.direct_kinds, &edge)
    }
    fn direct_date(
        &mut self,
        target: &Date<'a>,
        active: &[Date<'a>],
        mut reused: Vec<Direct>,
        lag: usize,
    ) -> Result<()> {
        let cancel = self.cancel;
        let subject = target.verified.subject;
        let to = target.verified.date;
        let first = self.direct.candidates;
        let mut batch = None;
        reused.sort_unstable_by_key(|r| (r.query, r.edge.target));
        let mut reused = reused.into_iter().peekable();
        let target_frames = Cached::new(&target.verified, cancel)?;
        for (query, founder) in self.request.founders.iter().enumerate() {
            cancel.poll()?;
            if founder.subject != subject || founder.date >= to {
                continue;
            }
            if to - founder.date <= lag {
                while reused.peek().is_some_and(|r| r.query == query) {
                    let item = reused.next().expect("peeked direct comparison");
                    require(
                        self.norms
                            .next(CandidateStream::Direct, self.direct.candidates)?
                            .is_none(),
                        "rolling checker: duplicate direct norm instead of graph reuse",
                    )?;
                    self.direct_edge(query, item.edge, &mut batch, first)?;
                }
            } else {
                let Some(source) = active.iter().find(|d| d.verified.date == founder.date) else {
                    continue;
                };
                let Some(a) = source.views.iter().find(|v| v.lens == founder.lens) else {
                    continue;
                };
                let Some(ag) = a
                    .groups
                    .iter()
                    .find(|g| g.eligible && g.rank == founder.rank)
                else {
                    continue;
                };
                let source_frames = Cached::new(&source.verified, cancel)?;
                for b in &target.views {
                    if !self.request.scope.lens_travel.contains(&[a.lens, b.lens]) {
                        continue;
                    }
                    for bg in b.groups.iter().filter(|g| g.eligible) {
                        require(
                            plus(self.graph.candidates, self.extra)?
                                < self.request.limits.max_candidates,
                            "rolling checker: extra direct work exceeds cap",
                        )?;
                        let ordinal = self.direct.candidates;
                        let norm = self.norms.next(CandidateStream::Direct, ordinal)?;
                        let edge = reconstruct_edge(
                            self.request,
                            (a, ag, &source_frames),
                            (b, bg, &target_frames),
                            ordinal,
                            norm,
                            cancel,
                        )?;
                        self.extra = plus(self.extra, 1)?;
                        self.direct_edge(query, edge, &mut batch, first)?;
                    }
                }
            }
        }
        require(
            reused.next().is_none(),
            "rolling checker: unmatched reused direct comparison",
        )?;
        if let Some(batch) = batch {
            require(
                batch.decisions.count == self.direct.candidates - first,
                "rolling checker: direct batch coverage differs",
            )?;
            self.batch = plus(self.batch, 1)?;
        }
        Ok(())
    }
}

fn partition(actual: &GroupPartition, view: &ViewGroups) -> Result<()> {
    require(
        (actual.subject, actual.date, actual.lens) == (view.subject, view.date, view.lens),
        "rolling checker: partition identity",
    )?;
    let first = view
        .groups
        .first()
        .ok_or_else(|| invalid("rolling checker: reconstructed empty view"))?
        .node;
    let PartitionState::Groups { ranks } = &actual.state else {
        return Err(invalid("rolling checker: omitted group partition"));
    };
    require(
        actual.first_node == first
            && ranks.len() == view.groups.len()
            && ranks.iter().zip(&view.groups).all(|(a, b)| *a == b.rank),
        "rolling checker: group partition altered",
    )
}

/// Every competitor is reconstructed independently, including unprojected edges.
/// Only integrity hashing and typed records are shared with the producer.
pub fn check(request: &Request, report: &Report, cancel: &Cancellation) -> Result<()> {
    cancel.poll()?;
    let admission = eigentrail_core::rolling_lineage::validate_structure(request, cancel)?;
    let retained = crate::retained_bytes(&[request.heap_bytes()?, report.heap_bytes()?])?;
    memory::ensure_operation(request.limits.max_bytes, retained, || {
        require(
            report.schema == SCHEMA
                && report.request_hash == serialization::digest_with_cancel(request, cancel)?
                && report.managed_reservation == admission.managed_reservation
                && report.resource_qualification
                    == "shared managed allocations; bounded subject/lag frames; no process-RSS guarantee",
            "rolling checker: schema, request binding or resource qualification",
        )?;
        let protocol = &report.protocol;
        require(
            protocol.method == "rolling-lineage-i001-r3-draft-f-conservative"
                && protocol.arithmetic_recipe == "rolling-scalar-outward-v1"
                && protocol.backend == "inari-2.0.0/faer-0.24.4"
                && protocol.format == "binary64"
                && protocol.angle_unit == "degrees",
            "rolling checker: unsupported numerical protocol",
        )?;
        let expected_scope = ReportScope {
            horizon: [0, request.dates.len() - 1],
            graph_lag: admission.lag,
            direct_scope: memory::text(&request.scope.direct_scope)?,
            all_simple_bound: admission.all_simple_bound,
            graph_ceiling: admission.eligible_ceiling,
            direct_ceiling: admission.direct_ceiling,
            direct_extra_ceiling: admission.direct_extra_ceiling,
            work_ceiling: admission.work_ceiling,
        };
        require(
            report.scope == expected_scope,
            "rolling checker: altered query scope",
        )?;
        let indexed = records(request, report, cancel)?;
        let mut replay = Replay {
            request,
            records: &indexed,
            cancel,
            request_hash: &report.request_hash,
            norms: Norms::new(&indexed.norms),
            reach: reach::Reach::new(request, cancel)?,
            batch: 0,
            window: 0,
            edge: 0,
            graph: Counts::default(),
            direct: Counts::default(),
            graph_kinds: CountsByKind::default(),
            direct_kinds: CountsByKind::default(),
            graph_hash: integrity::CandidateHasher::new(
                &report.request_hash,
                CandidateStream::Graph,
                None,
            )?,
            direct_hash: integrity::CandidateHasher::new(
                &report.request_hash,
                CandidateStream::Direct,
                None,
            )?,
            extra: 0,
        };
        let (mut date_cursor, mut partition_cursor, mut view_cursor, mut next_node) = (0, 0, 0, 0);
        for subject in 0..request.subjects.len() {
            let mut active: Vec<Date<'_>> = Vec::new();
            for date in 0..request.dates.len() {
                cancel.poll()?;
                if matches!(request.subjects[subject].slots[date], Slot::Observed { .. }) {
                    let evidence = *indexed.dates.get(date_cursor).ok_or_else(|| {
                        invalid("rolling checker: omitted observed root evidence")
                    })?;
                    require(
                        (evidence.subject, evidence.date) == (subject, date),
                        "rolling checker: repeated, reordered or mislabelled date evidence",
                    )?;
                    date_cursor = plus(date_cursor, 1)?;
                    let verified = frames::inspect_date(request, evidence, cancel)?;
                    let reconstructed =
                        views::reconstruct_date(request, &verified, next_node, cancel)?;
                    for view in &reconstructed {
                        let actual = indexed
                            .partitions
                            .get(partition_cursor)
                            .ok_or_else(|| invalid("rolling checker: omitted partition"))?;
                        partition(actual, view)?;
                        partition_cursor = plus(partition_cursor, 1)?;
                        if full_view(request, view) {
                            let actual = indexed.views.get(view_cursor).ok_or_else(|| {
                                invalid("rolling checker: omitted projected view")
                            })?;
                            equal_wire(*actual, view, cancel)?;
                            view_cursor = plus(view_cursor, 1)?;
                        }
                        replay.reach.register_view(view, cancel)?;
                        if let Some(last) = view.groups.last() {
                            next_node = plus(last.node, 1)?;
                        }
                    }
                    let founder = request
                        .founders
                        .iter()
                        .any(|f| f.subject == subject && f.date == date);
                    memory::push(
                        &mut active,
                        Date {
                            verified,
                            views: reconstructed,
                            founder,
                        },
                    )?;
                }
                let target = active.iter().find(|d| d.verified.date == date);
                let mut direct = Vec::new();
                for lag in 1..=admission.lag.min(date) {
                    let from = date - lag;
                    let source = active.iter().find(|d| d.verified.date == from);
                    replay.graph_window(subject, from, date, source.zip(target), &mut direct)?;
                }
                if let Some(target) = target {
                    replay.direct_date(target, &active, direct, admission.lag)?;
                }
                active.retain(|d| d.founder || date - d.verified.date < admission.lag);
            }
        }
        require(
            date_cursor == indexed.dates.len()
                && partition_cursor == indexed.partitions.len()
                && view_cursor == indexed.views.len(),
            "rolling checker: extra source or view records",
        )?;
        require(
            replay.batch == indexed.batches.len()
                && replay.window == indexed.windows.len()
                && replay.edge == indexed.edges.len(),
            "rolling checker: extra batches, windows or projected edges",
        )?;
        replay.norms.finish()?;
        require(
            report.completion.graph_counts == replay.graph
                && report.completion.direct_counts == replay.direct
                && report.completion.graph_kind_counts == replay.graph_kinds
                && report.completion.direct_kind_counts == replay.direct_kinds,
            "rolling checker: terminal comparison counts",
        )?;
        require(
            report.completion.graph_digest == replay.graph_hash.finish()?
                && report.completion.direct_digest == replay.direct_hash.finish()?,
            "rolling checker: complete candidate stream digest",
        )?;
        replay
            .reach
            .finish(&indexed.founders, &indexed.paths, cancel)?;
        cancel.poll()
    })
}
