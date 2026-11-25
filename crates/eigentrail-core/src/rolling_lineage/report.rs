//! Request-bound rolling witnesses and projected records; no producer/checker decisions.
use super::Lens;
use crate::{
    Eigensystem, Error, Matrix, Result,
    resources::{Cancellation, memory},
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Agreement,
    Exclusion,
    Open,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStream {
    Graph,
    Direct,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    Root,
    Correlation,
    NormalisedPrecision,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    LineLine,
    EqualBlock,
    Containment,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmallerSide {
    None,
    Source,
    Target,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupingCause {
    Simple,
    Numerical,
    Region,
    NumericalAndRegion,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisSign {
    Positive,
    Negative,
    Unresolved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InformationLevel {
    Line,
    ProperSubspace,
    WholeSpace,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BottleneckFamily {
    LineOnly,
    TypedBlockGraph,
    RelaxedInternalLines,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSide {
    Lower,
    Upper,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowOutcome {
    Agreement,
    CompleteBreak,
    Open,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    MissingDate,
    ExcludedRank,
    UnresolvedInternalLine,
    UnresolvedSeparation,
    UnsupportedTransfer,
    NormalisationUnresolved,
    NumericalUnresolved,
    SourceUnavailable,
    OutOfScope,
    NoCandidates,
    ResourceLimit,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormKind {
    Difference,
    Gram,
}
/// Counts projection retains boundaries; radii are reconstructed from the recorded recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupPartition {
    pub subject: usize,
    pub date: usize,
    pub lens: Lens,
    pub first_node: usize,
    pub state: PartitionState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartitionState {
    Groups { ranks: Vec<[usize; 2]> },
    Unavailable { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub request_hash: String,
    pub protocol: Protocol,
    pub scope: ReportScope,
    pub chunks: Vec<Chunk>,
    pub completion: Completion,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub method: String,
    pub arithmetic_recipe: String,
    pub backend: String,
    pub format: String,
    pub angle_unit: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportScope {
    pub horizon: [usize; 2],
    pub graph_lag: usize,
    pub direct_scope: String,
    pub all_simple_bound: usize,
    pub graph_ceiling: usize,
    pub direct_ceiling: usize,
    pub direct_extra_ceiling: usize,
    pub work_ceiling: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub index: usize,
    pub first_record: usize,
    pub prev_digest: String,
    pub records: RecordBatch,
    pub digest: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateEvidence {
    pub subject: usize,
    pub date: usize,
    pub root: Eigensystem,
    pub root_numerical_rho: f64,
    pub correlation: FrameState,
    pub inverse: InverseState,
    pub normalised_precision: FrameState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameProof {
    pub midpoint: Matrix,
    pub eigensystem: Eigensystem,
    pub matrix_error: f64,
    pub numerical_rho: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewGroups {
    pub subject: usize,
    pub date: usize,
    pub lens: Lens,
    pub reference: Reference,
    pub spectrum: SpectrumState,
    pub region: RegionState,
    pub groups: Vec<GroupRecord>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupRecord {
    pub node: usize,
    pub rank: [usize; 2],
    pub columns: [usize; 2],
    pub grouping: GroupingCause,
    pub multiplicity: Multiplicity,
    pub information: InformationLevel,
    pub angular_radius_deg: f64,
    pub spectral_sum: Enclosure,
    pub covariance_share: ShareState,
    pub eligible: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeRecord {
    pub ordinal: usize,
    pub source: usize,
    pub target: usize,
    pub edge_type: EdgeType,
    pub smaller_side: SmallerSide,
    pub point: Enclosure,
    pub geometry: Enclosure,
    pub method_bracket: Enclosure,
    pub verdict: Verdict,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub candidates: usize,
    pub agreement: usize,
    pub exclusion: usize,
    pub open: usize,
    pub unavailable: usize,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountsByKind {
    pub line: Counts,
    pub equal_block: Counts,
    pub containment: Counts,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensWindow {
    pub pair: [Lens; 2],
    pub counts: Counts,
    pub outcome: WindowOutcome,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSummary {
    pub subject: usize,
    pub from_date: usize,
    pub to_date: usize,
    pub graph_range: [usize; 2],
    pub global_counts: Counts,
    pub global_outcome: WindowOutcome,
    pub lens_pairs: Vec<LensWindow>,
    pub complete: bool,
    pub candidate_digest: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackedDecisions {
    pub count: usize,
    pub hex: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBatch {
    pub stream: CandidateStream,
    pub first: usize,
    pub decisions: PackedDecisions,
    pub digest: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyState {
    pub family: BottleneckFamily,
    pub lower: Bottleneck,
    pub upper: Bottleneck,
    pub lower_parent: Parent,
    pub upper_parent: Parent,
    pub reach: ReachState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedPath {
    pub query: usize,
    pub target: usize,
    pub family: BottleneckFamily,
    pub side: PathSide,
    pub nodes: Vec<usize>,
    pub slot_gaps: Vec<usize>,
    pub cost: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparseNormWitness {
    pub stream: CandidateStream,
    pub ordinal: usize,
    pub kind: NormKind,
    pub matrix: Matrix,
    pub entry_error: f64,
    pub eigensystem: Eigensystem,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub status: CompletionStatus,
    pub chunks: usize,
    pub records: usize,
    pub terminal_digest: String,
    pub graph_digest: String,
    pub direct_digest: String,
    pub graph_counts: Counts,
    pub direct_counts: Counts,
    pub graph_kind_counts: CountsByKind,
    pub direct_kind_counts: CountsByKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    Complete,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FrameState {
    NotRequested,
    Available { proof: FrameProof },
    Unavailable { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InverseState {
    NotRequested,
    Available {
        midpoint: Matrix,
        operator_error: f64,
    },
    Unavailable {
        reason: Reason,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpectrumState {
    Available { intervals: Vec<[f64; 2]> },
    Unavailable { reason: Reason },
    Unbounded { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegionState {
    Planar {
        centre_correlation: [f64; 2],
        population_correlation: [f64; 2],
        cutoff: [f64; 2],
        numerical_rho: f64,
        sign: AxisSign,
    },
    Point {
        numerical_rho: f64,
    },
    Conditional {
        mapped_rho: f64,
        numerical_rho: f64,
        total_rho: f64,
    },
    Unsupported {
        reason: Reason,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Multiplicity {
    Unknown,
    Exact { count: usize },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Enclosure {
    Bounds { range: [f64; 2] },
    Undefined { reason: Reason },
    Unbounded { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShareState {
    Covariance { range: [f64; 2] },
    NotApplicable,
    Unavailable { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FounderRecord {
    Origin {
        query: usize,
        origin: FounderOrigin,
    },
    Target {
        query: usize,
        node: usize,
        direct: DirectResult,
        families: Vec<FamilyState>,
    },
    Unavailable {
        query: usize,
        date: usize,
        lens: Lens,
        reason: Reason,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FounderOrigin {
    Node { node: usize },
    InternalLine { group: usize },
    Unavailable { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectResult {
    Available {
        ordinal: usize,
        point: Enclosure,
        geometry: Enclosure,
        method_bracket: Enclosure,
        verdict: Verdict,
    },
    Unavailable {
        reason: Reason,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Bottleneck {
    Bound { degrees: f64 },
    NoRoute,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Parent {
    Origin,
    Node { node: usize },
    NoRoute,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReachState {
    Agreed,
    Possible,
    Excluded,
    Unavailable { reason: Reason },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "records",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RecordBatch {
    Partitions(Vec<GroupPartition>),
    DateEvidence(Vec<DateEvidence>),
    Views(Vec<ViewGroups>),
    Edges(Vec<EdgeRecord>),
    Windows(Vec<WindowSummary>),
    Candidates(Vec<CandidateBatch>),
    Founders(Vec<FounderRecord>),
    Paths(Vec<SelectedPath>),
    Norms(Vec<SparseNormWitness>),
}
impl RecordBatch {
    pub fn len(&self) -> usize {
        match self {
            Self::Partitions(rows) => rows.len(),
            Self::DateEvidence(rows) => rows.len(),
            Self::Views(rows) => rows.len(),
            Self::Edges(rows) => rows.len(),
            Self::Windows(rows) => rows.len(),
            Self::Candidates(rows) => rows.len(),
            Self::Founders(rows) => rows.len(),
            Self::Paths(rows) => rows.len(),
            Self::Norms(rows) => rows.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn malformed() -> Error {
    memory::text("malformed packed rolling decisions")
        .map(Error::Invalid)
        .unwrap_or_else(|error| error)
}
fn overflow() -> Error {
    Error::Resource(String::new())
}
fn nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(malformed()),
    }
}
impl PackedDecisions {
    /// Four two-bit decisions per byte, earliest in low bits; lowercase hex, high padding zero.
    pub fn pack(values: &[Verdict], cancel: &Cancellation) -> Result<Self> {
        cancel.poll()?;
        let bytes = values.len() / 4 + usize::from(values.len() % 4 != 0);
        let length = bytes.checked_mul(2).ok_or_else(overflow)?;
        let mut output = memory::capacity::<u8>(length)?;
        let digits = b"0123456789abcdef";
        for (index, group) in values.chunks(4).enumerate() {
            if index % 256 == 0 {
                cancel.poll()?;
            }
            let mut value = 0u8;
            for (shift, decision) in group.iter().enumerate() {
                let code = match decision {
                    Verdict::Agreement => 0,
                    Verdict::Exclusion => 1,
                    Verdict::Open => 2,
                    Verdict::Unavailable => 3,
                };
                value |= code << (2 * shift);
            }
            output.push(digits[usize::from(value >> 4)]);
            output.push(digits[usize::from(value & 15)]);
        }
        cancel.poll()?;
        Ok(Self {
            count: values.len(),
            hex: String::from_utf8(output).expect("ASCII packed hex"),
        })
    }
    pub fn validate(&self, cancel: &Cancellation) -> Result<()> {
        let bytes = self.count / 4 + usize::from(self.count % 4 != 0);
        if bytes.checked_mul(2) != Some(self.hex.len()) {
            return Err(malformed());
        }
        for (index, &value) in self.hex.as_bytes().iter().enumerate() {
            if index % 4096 == 0 {
                cancel.poll()?;
            }
            nibble(value)?;
        }
        let remainder = self.count % 4;
        if remainder != 0 {
            let last = self.byte(bytes - 1)?;
            if last >> (remainder * 2) != 0 {
                return Err(malformed());
            }
        }
        cancel.poll()
    }
    fn byte(&self, index: usize) -> Result<u8> {
        let first = index.checked_mul(2).ok_or_else(overflow)?;
        let pair = self
            .hex
            .as_bytes()
            .get(first..first.checked_add(2).ok_or_else(overflow)?)
            .ok_or_else(malformed)?;
        Ok((nibble(pair[0])? << 4) | nibble(pair[1])?)
    }
    /// Validate the batch once before iteration; individual lookups stay constant-time and fallible.
    pub fn get(&self, index: usize) -> Result<Verdict> {
        if index >= self.count {
            return Err(malformed());
        }
        Ok(match (self.byte(index / 4)? >> (2 * (index % 4))) & 3 {
            0 => Verdict::Agreement,
            1 => Verdict::Exclusion,
            2 => Verdict::Open,
            _ => Verdict::Unavailable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_decisions_bind_order_count_and_padding() {
        let cancel = Cancellation::default();
        let kinds = [
            Verdict::Agreement,
            Verdict::Exclusion,
            Verdict::Open,
            Verdict::Unavailable,
        ];
        for length in 0..=16 {
            let values = (0..length)
                .map(|index| kinds[index % 4])
                .collect::<Vec<_>>();
            let packed = PackedDecisions::pack(&values, &cancel).unwrap();
            packed.validate(&cancel).unwrap();
            assert_eq!(
                (0..length)
                    .map(|index| packed.get(index).unwrap())
                    .collect::<Vec<_>>(),
                values
            );
            assert!(packed.get(length).is_err());
        }
        assert_eq!(PackedDecisions::pack(&kinds, &cancel).unwrap().hex, "e4");
        for hex in ["E4", "e", "e400", "ff", "zz"] {
            assert!(
                PackedDecisions {
                    count: 3,
                    hex: hex.into()
                }
                .validate(&cancel)
                .is_err()
            );
        }
        cancel.cancel();
        assert!(matches!(
            PackedDecisions::pack(&[], &cancel),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            PackedDecisions {
                count: 0,
                hex: String::new()
            }
            .validate(&cancel),
            Err(Error::Cancelled)
        ));
    }
    fn wire_unit_input() -> Report {
        Report {
            schema: super::super::SCHEMA.into(),
            request_hash: "0".repeat(64),
            protocol: Protocol {
                method: "wire unit input only".into(),
                arithmetic_recipe: "unimplemented".into(),
                backend: "unqualified".into(),
                format: "binary64".into(),
                angle_unit: "degrees".into(),
            },
            scope: ReportScope {
                horizon: [0, 1],
                graph_lag: 1,
                direct_scope: "all_later_slots".into(),
                all_simple_bound: 64,
                graph_ceiling: 4,
                direct_ceiling: 2,
                direct_extra_ceiling: 0,
                work_ceiling: 4,
            },
            chunks: vec![
                Chunk {
                    index: 0,
                    first_record: 0,
                    prev_digest: "0".repeat(64),
                    digest: "0".repeat(64),
                    records: RecordBatch::Paths(vec![SelectedPath {
                        query: 0,
                        target: 1,
                        family: BottleneckFamily::LineOnly,
                        side: PathSide::Upper,
                        nodes: vec![0, 1],
                        slot_gaps: vec![1],
                        cost: -0.,
                    }]),
                },
                Chunk {
                    index: 1,
                    first_record: 1,
                    prev_digest: "0".repeat(64),
                    digest: "0".repeat(64),
                    records: RecordBatch::Candidates(vec![CandidateBatch {
                        stream: CandidateStream::Graph,
                        first: 0,
                        decisions: PackedDecisions {
                            count: 1,
                            hex: "00".into(),
                        },
                        digest: "0".repeat(64),
                    }]),
                },
            ],
            completion: Completion {
                status: CompletionStatus::Complete,
                chunks: 2,
                records: 2,
                terminal_digest: "0".repeat(64),
                graph_digest: "0".repeat(64),
                direct_digest: "0".repeat(64),
                graph_counts: Counts::default(),
                direct_counts: Counts::default(),
                graph_kind_counts: CountsByKind::default(),
                direct_kind_counts: CountsByKind::default(),
            },
            managed_reservation: 0,
            resource_qualification: "wire unit input, not a native report".into(),
        }
    }
    #[test]
    fn strict_report_wire_roundtrip_and_mutations() {
        use crate::resources::memory::Operation;
        use crate::serialization;
        let input = wire_unit_input();
        let encoded = serialization::encode(&input).unwrap();
        assert_eq!(
            encoded,
            serialization::encode_rolling_report(&input).unwrap()
        );
        let cancelled = Cancellation::default();
        cancelled.cancel();
        assert!(matches!(
            serialization::encode_rolling_report_with_cancel(&input, &cancelled),
            Err(Error::Cancelled)
        ));
        let operation = Operation::new(1024 * 1024).unwrap();
        let decoded = operation
            .run(|| serialization::decode_in::<Report>(&encoded, &Cancellation::default()))
            .unwrap();
        assert_eq!(decoded, input);
        let streamed = operation
            .run(|| serialization::decode_rolling_report_in(&encoded, &Cancellation::default()))
            .unwrap();
        assert_eq!(streamed, input);
        assert!(matches!(
            operation.run(|| serialization::decode_rolling_report_in(&encoded, &cancelled)),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            serialization::decode_rolling_report_in(&encoded, &Cancellation::default()),
            Err(Error::Resource(_))
        ));
        let RecordBatch::Paths(paths) = &decoded.chunks[0].records else {
            panic!("wire unit paths")
        };
        assert_eq!(paths[0].cost.to_bits(), (-0f64).to_bits());
        let text = String::from_utf8(encoded).unwrap();
        for altered in [
            text.replacen("\"format\":", "\"unknown\":1,\"format\":", 1),
            text.replacen("\"cost\":\"f64:8000000000000000\"", "\"cost\":0.0", 1),
            text.replacen(
                "\"cost\":\"f64:8000000000000000\"",
                "\"cost\":\"f64:7ff0000000000000\"",
                1,
            ),
            text.replacen("\"cost\":", "\"cost\":null,\"cost\":", 1),
            text.replacen("\"status\":\"complete\"", "\"status\":\"refused\"", 1),
            text.replacen("\"hex\":\"00\"", "\"hex\":\"04\"", 1),
            text[..text.len() - 1].to_string(),
            format!(" {text}"),
            format!("{text} "),
            format!("{text}{{}}"),
        ] {
            assert_ne!(altered, text);
            assert!(matches!(
                operation.run(|| serialization::decode_in::<Report>(
                    altered.as_bytes(),
                    &Cancellation::default()
                )),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                operation.run(|| serialization::decode_rolling_report_in(
                    altered.as_bytes(),
                    &Cancellation::default()
                )),
                Err(Error::Invalid(_))
            ));
        }
    }

    #[test]
    fn planar_axes_and_spectral_variation_keep_their_own_wire_fields() {
        use crate::{resources::memory::Operation, serialization};
        let state = RegionState::Planar {
            centre_correlation: [0.2, 0.3],
            population_correlation: [0.1, 0.4],
            cutoff: [0.09, 0.11],
            numerical_rho: 1e-12,
            sign: AxisSign::Positive,
        };
        let bytes = serialization::encode(&state).unwrap();
        let operation = Operation::new(1024 * 1024).unwrap();
        let decoded = operation
            .run(|| serialization::decode_in::<RegionState>(&bytes, &Cancellation::default()))
            .unwrap();
        assert_eq!(decoded, state);
        assert_eq!(serialization::encode(&decoded).unwrap(), bytes);
        let mut bad: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        bad["mapped_rho"] = serde_json::Value::String("f64:0000000000000000".into());
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(matches!(
            operation
                .run(|| serialization::decode_in::<RegionState>(&bytes, &Cancellation::default())),
            Err(Error::Invalid(_))
        ));
    }
}
