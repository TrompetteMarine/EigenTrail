//! Fallible owned decoding of package schemas from borrowed JSON fragments.
use crate::continuous_queries as continuous;
use crate::jump_queries as jq;
use crate::transport_workflow as tw;
use crate::{
    common_frame::*,
    common_frame_queries::*,
    families::*,
    jumps::*,
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    *,
};
use serde::{
    Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{fmt, marker::PhantomData};

pub trait DecodeOwned: Sized + serde::Serialize + HeapBytes {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self>;
}

pub fn decode_in<T: DecodeOwned>(bytes: &[u8], cancel: &Cancellation) -> Result<T> {
    if memory::active_limit().is_none() {
        return Err(Error::Resource(
            "decoding requires a shared operation".into(),
        ));
    }
    memory::scope(|| {
        cancel.poll()?;
        super::input::validate_package_keys(bytes, cancel)?;
        let raw: &RawValue = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let record = T::from_raw(raw, cancel)?;
        super::compare_encoding(&record, bytes, cancel)?;
        Ok(record)
    })
}

fn invalid() -> Error {
    Error::Invalid("malformed typed package JSON".into())
}

struct Record<'a, const N: usize> {
    fields: [Option<&'a RawValue>; N],
}
impl<'a, const N: usize> Record<'a, N> {
    fn parse(
        raw: &'a RawValue,
        names: &'static [&'static str],
        cancel: &Cancellation,
    ) -> Result<Self> {
        if !raw.get().starts_with('{') {
            return Err(invalid());
        }
        struct Fields<'a, const N: usize> {
            names: &'static [&'static str],
            cancel: &'a Cancellation,
        }
        impl<'de, const N: usize> Visitor<'de> for Fields<'_, N> {
            type Value = Record<'de, N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("package record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut fields = [None; N];
                while let Some(key) = map.next_key::<&str>()? {
                    if self.cancel.poll().is_err() {
                        return Err(de::Error::custom("cancelled"));
                    }
                    let index = self
                        .names
                        .iter()
                        .position(|name| *name == key)
                        .ok_or_else(|| de::Error::custom("unknown package field"))?;
                    if fields[index].is_some() {
                        return Err(de::Error::custom("duplicate package field"));
                    }
                    fields[index] = Some(map.next_value::<&RawValue>()?);
                }
                Ok(Record { fields })
            }
        }
        let result = serde_json::Deserializer::from_str(raw.get())
            .deserialize_map(Fields::<N> { names, cancel });
        cancel.poll()?;
        result.map_err(|_| invalid())
    }
    fn required<T: DecodeOwned>(&self, index: usize, cancel: &Cancellation) -> Result<T> {
        T::from_raw(self.fields[index].ok_or_else(invalid)?, cancel)
    }
    fn defaulted<T: DecodeOwned + Default>(
        &self,
        index: usize,
        cancel: &Cancellation,
    ) -> Result<T> {
        self.fields[index].map_or_else(|| Ok(T::default()), |raw| T::from_raw(raw, cancel))
    }
    fn only(&self, indices: &[usize]) -> Result<()> {
        if self
            .fields
            .iter()
            .enumerate()
            .any(|(i, v)| v.is_some() && !indices.contains(&i))
        {
            Err(invalid())
        } else {
            Ok(())
        }
    }
    fn tag(&self, index: usize) -> Result<&str> {
        let text = self.fields[index].ok_or_else(invalid)?.get();
        text.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .ok_or_else(invalid)
    }
}

macro_rules! record {
    ($type:ty; $($field:ident : $ty:ty),* $(,)? ; $($optional:ident : $opt:ty),* $(,)?) => {
        impl DecodeOwned for $type {
            fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
                memory::scope(|| {
                    const NAMES: &[&str] = &[$(stringify!($field),)* $(stringify!($optional),)*];
                    let fields = Record::<{NAMES.len()}>::parse(raw, NAMES, cancel)?;
                    let mut index = 0..NAMES.len();
                    Ok(Self {
                        $($field: fields.required::<$ty>(index.next().unwrap(), cancel)?,)*
                        $($optional: fields.defaulted::<$opt>(index.next().unwrap(), cancel)?,)*
                    })
                })
            }
        }
    };
}

impl DecodeOwned for f64 {
    fn from_raw(raw: &RawValue, _: &Cancellation) -> Result<Self> {
        let s = raw.get();
        if s.len() != 22
            || !s.starts_with("\"f64:")
            || !s.ends_with('"')
            || !s.as_bytes()[5..21]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
        {
            return Err(invalid());
        }
        let bits = u64::from_str_radix(&s[5..21], 16).map_err(|_| invalid())?;
        let value = f64::from_bits(bits);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(invalid())
        }
    }
}
macro_rules! unsigned {
    ($($type:ty),*) => { $(impl DecodeOwned for $type {
        fn from_raw(raw: &RawValue, _: &Cancellation) -> Result<Self> {
            let s = raw.get();
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) { return Err(invalid()); }
            s.parse().map_err(|_| invalid())
        }
    })* };
}
unsigned!(usize, u64);
impl DecodeOwned for bool {
    fn from_raw(raw: &RawValue, _: &Cancellation) -> Result<Self> {
        match raw.get() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(invalid()),
        }
    }
}
impl<T: DecodeOwned> DecodeOwned for Option<T> {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        // The existing wire format rejects null rather than silently treating it as absence.
        T::from_raw(raw, cancel).map(Some)
    }
}
impl<T: DecodeOwned> DecodeOwned for Vec<T> {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            if !raw.get().starts_with('[') {
                return Err(invalid());
            }
            struct Items<'a, T> {
                cancel: &'a Cancellation,
                failure: &'a mut Option<Error>,
                marker: PhantomData<T>,
            }
            impl<'de, T: DecodeOwned> Visitor<'de> for Items<'_, T> {
                type Value = Vec<T>;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("package array")
                }
                fn visit_seq<A: SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Vec<T>, A::Error> {
                    let mut result = Vec::new();
                    while let Some(raw) = seq.next_element::<&RawValue>()? {
                        let item = self
                            .cancel
                            .poll()
                            .and_then(|_| T::from_raw(raw, self.cancel));
                        let step = item.and_then(|v| memory::push(&mut result, v));
                        if let Err(error) = step {
                            *self.failure = Some(error);
                            return Err(de::Error::custom("package decoding stopped"));
                        }
                    }
                    Ok(result)
                }
            }
            let mut failure = None;
            let result =
                serde_json::Deserializer::from_str(raw.get()).deserialize_seq(Items::<T> {
                    cancel,
                    failure: &mut failure,
                    marker: PhantomData,
                });
            cancel.poll()?;
            match result {
                Ok(v) => Ok(v),
                Err(_) => Err(failure.unwrap_or_else(invalid)),
            }
        })
    }
}
impl DecodeOwned for [usize; 2] {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            Vec::<usize>::from_raw(raw, cancel)?
                .try_into()
                .map_err(|_| invalid())
        })
    }
}
impl DecodeOwned for [f64; 2] {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            Vec::<f64>::from_raw(raw, cancel)?
                .try_into()
                .map_err(|_| invalid())
        })
    }
}

impl DecodeOwned for String {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let bytes = raw.get().as_bytes();
            if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
                return Err(invalid());
            }
            let mut out = memory::capacity::<u8>(bytes.len() - 2)?;
            let mut i = 1;
            let mut next_poll = 4096;
            while i < bytes.len() - 1 {
                if i >= next_poll {
                    cancel.poll()?;
                    next_poll = i + 4096;
                }
                let next = bytes[i];
                i += 1;
                if next != b'\\' {
                    if next < 32 {
                        return Err(invalid());
                    }
                    memory::push(&mut out, next)?;
                    continue;
                }
                let escaped = *bytes.get(i).ok_or_else(invalid)?;
                i += 1;
                let code = match escaped {
                    b'"' => b'"',
                    b'\\' => b'\\',
                    b'/' => b'/',
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'u' => {
                        let high = hex4(bytes, &mut i)?;
                        let scalar = if (0xd800..=0xdbff).contains(&high) {
                            if bytes.get(i..i + 2) != Some(b"\\u".as_slice()) {
                                return Err(invalid());
                            }
                            i += 2;
                            let low = hex4(bytes, &mut i)?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return Err(invalid());
                            }
                            0x10000 + ((high - 0xd800) << 10) + low - 0xdc00
                        } else {
                            high
                        };
                        let ch = char::from_u32(scalar).ok_or_else(invalid)?;
                        let mut encoded = [0; 4];
                        memory::extend(&mut out, ch.encode_utf8(&mut encoded).as_bytes())?;
                        continue;
                    }
                    _ => return Err(invalid()),
                };
                memory::push(&mut out, code)?;
            }
            if out.starts_with(b"str:") {
                out.drain(..4);
            } else if out.starts_with(b"f64:") {
                return Err(invalid());
            }
            String::from_utf8(out).map_err(|_| invalid())
        })
    }
}
fn hex4(bytes: &[u8], index: &mut usize) -> Result<u32> {
    let four = bytes.get(*index..*index + 4).ok_or_else(invalid)?;
    let s = std::str::from_utf8(four).map_err(|_| invalid())?;
    *index += 4;
    u32::from_str_radix(s, 16).map_err(|_| invalid())
}

record!(Matrix; n:usize, data:Vec<f64>;);
record!(TraceRequest; schema:String, times:Vec<f64>, covariances:Vec<Matrix>, features:Vec<String>,
    lipschitz:f64, calibration:Calibration, correction:Vec<f64>, target:f64,
    numerical_allowance:f64, memory_bytes:usize;);
record!(Eigensystem; values:Vec<f64>, vectors:Matrix, backward:f64, polar:f64;);
record!(Range; lower:f64, upper:f64, lower_closed:bool, upper_closed:bool;);
record!(Summary; strength:Range, share:Quantity, rank:[usize;2];);
record!(Tube; start:usize, end:usize, eta:f64, gap:f64, bound:f64, informative:bool;);
record!(Segment; tubes:Vec<Tube>, edges:Vec<Vec<usize>>, association:Association;);
record!(TraceReport; schema:String, request_hash:String, eigensystems:Vec<Eigensystem>, radii:Vec<f64>,
    segments:Vec<Segment>, summaries:Vec<Vec<Summary>>, calibration_scope:String, witness:String,
    completed:bool, managed_reservation:usize, resource_qualification:String;);
record!(CommonFrameModel; weights:Vec<f64>, common_frame_assumed:bool;);
record!(CommonFrame; axes:Eigensystem, axis_error:f64, labelled_strengths:Vec<Vec<Range>>,
    possible_ranks:Vec<Vec<[usize;2]>>;);
record!(CommonFrameReport; schema:String, request_hash:String, model_hash:String,
    eigensystems:Vec<Eigensystem>, radii:Vec<f64>, calibration_scope:String, frame:CommonFrame,
    scope:String, completed:bool, managed_reservation:usize, resource_qualification:String;);
record!(JumpModel; at_most_one_per_cell:bool, minimum_applies_to_all_candidates:bool; minimum_jump:Option<f64>);
record!(JumpCell; flagged:bool, amplitude:Range, timing:Range, alternative:String;);
record!(JumpFamily; cells:Vec<JumpCell>, mandatory:Vec<bool>, optional:Vec<bool>, scope:String;);
record!(JumpReport; schema:String, request_hash:String, model_hash:String, eigensystems:Vec<Eigensystem>,
    radii:Vec<f64>, differences:Vec<Eigensystem>, family:JumpFamily, calibration_scope:String,
    completed:bool, managed_reservation:usize, resource_qualification:String;);
record!(AffineWitness; nodes:Vec<Matrix>, widening:f64;);
record!(Configuration; id:String, lipschitz:f64, class:PathClass, witnesses:Vec<AffineWitness>;);
record!(HistoryRequest; schema:String, observations:TraceRequest, configurations:Vec<Configuration>, query_times:Vec<f64>;);
record!(GridNode; time:f64, observations:Vec<usize>, queries:Vec<usize>;);
record!(PartitionOverlap; date:usize, left_block:usize, right_block:usize, rank:usize;);
record!(ConfigurationReport; id:String, class:PathClass, lipschitz:f64, grid:Vec<GridNode>,
    continuous_conditional:TraceReport, jumps:Vec<JumpReport>, cells:Vec<CellScope>,
    partition_overlaps:Vec<PartitionOverlap>, feasibility:Feasibility;
    common_frame:Vec<CommonFrameAssessment>, common_frame_queries:Vec<Workflow>,
    continuous_workflow:Vec<continuous::Workflow>, crossings:Vec<crossings::CrossingAssessment>,
    transports:Vec<tw::TransportAssessment>, jump_workflow:Vec<jq::Workflow>);
record!(HistoryReport; schema:String, request_hash:String, configurations:Vec<ConfigurationReport>,
    scope:String, completed:bool, managed_reservation:usize, resource_qualification:String;);
record!(Workflow; queries:Vec<QueryRow>, witnesses:Vec<PolarAffineWitness>, scope:String;);
record!(QueryRow; time:f64, state:QueryState;);
record!(PolarAffineWitness; diagonal_nodes:Vec<Vec<f64>>, nodal_error_bounds:Vec<f64>,
    step_norm_bounds:Vec<f64>, feasibility:Feasibility;);
record!(curves::CentreEvaluation; schema:String, request_hash:String, segment:usize,
    time:f64, evaluation_error:f64, eigensystem:Eigensystem;);
record!(continuous::Workflow; scope:String, lineage:Vec<continuous::SegmentLineage>,
    queries:Vec<continuous::Query>, witnesses:Vec<continuous::QueryWitness>;);
record!(continuous::SegmentLineage; start:Vec<continuous::Origin>, end:Vec<continuous::Origin>;);
record!(continuous::Origin; origins:Vec<usize>, persistent:bool;);
record!(continuous::Query; time:f64, centre:curves::CentreEvaluation, state:continuous::QueryState;);
record!(continuous::BlockSummary; start:usize, end:usize, origins:Vec<usize>,
    persistent:bool, strength:Range, share:Quantity, rank:[usize;2], projector_error:f64,
    local_displacement:Quantity, reference_displacement:Quantity;);
record!(continuous::QueryWitness; values:Vec<Matrix>, evaluation_errors:Vec<f64>, feasibility:Feasibility;);
record!(jq::Workflow; scope:String, components:Vec<jq::Component>, queries:Vec<jq::Query>,
    witnesses:Vec<jq::StepWitness>;);
record!(jq::Component; cells:[usize;2], lineage:Vec<continuous::SegmentLineage>;);
record!(jq::Query; time:f64, state:jq::State;);
record!(jq::Side; date:usize, radius:f64, trace:Range, modes:Vec<Summary>;);
record!(jq::StepWitness; cells:Vec<usize>, values:Vec<Matrix>;);
impl DecodeOwned for jq::State {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let fields = Record::<9>::parse(
                raw,
                &[
                    "kind",
                    "date",
                    "trace",
                    "modes",
                    "component",
                    "query",
                    "cell",
                    "mandatory",
                    "sides",
                ],
                cancel,
            )?;
            match fields.tag(0)? {
                "Observed" => {
                    fields.only(&[0, 1, 2, 3])?;
                    Ok(Self::Observed {
                        date: fields.required(1, cancel)?,
                        trace: fields.required(2, cancel)?,
                        modes: fields.required(3, cancel)?,
                    })
                }
                "Smooth" => {
                    fields.only(&[0, 4, 5])?;
                    Ok(Self::Smooth {
                        component: fields.required(4, cancel)?,
                        query: fields.required(5, cancel)?,
                    })
                }
                "JumpPossible" => {
                    fields.only(&[0, 6, 7, 8])?;
                    Ok(Self::JumpPossible {
                        cell: fields.required(6, cancel)?,
                        mandatory: fields.required(7, cancel)?,
                        sides: fields.required(8, cancel)?,
                    })
                }
                _ => Err(invalid()),
            }
        })
    }
}

record!(crossings::CrossingWindow; block:[usize;2], centre:usize, secant:[usize;2],
    attachments:[usize;2], bracket:[f64;2], curvature:f64, exact_collision_assumed:bool;);
record!(crossings::CollisionBudget; plane_error:f64, derivative_error:f64, slope_gap:f64,
    anchor_error:f64, branch_error:f64, min_slope:f64;);
record!(crossings::TimingContrast; date:usize, estimate:Range, radius:f64, strength:Range, candidate:Range;);
record!(crossings::CrossingProof; compressed:Eigensystem, slope_anchors:Matrix,
    anchor_polar:f64, compression_error:f64, display_error:f64, budget:Vec<crossings::CollisionBudget>,
    left_errors:Vec<f64>, right_errors:Vec<f64>, left_candidates:Vec<Vec<usize>>,
    right_candidates:Vec<Vec<usize>>, left:Vec<usize>, right:Vec<usize>,
    contrasts:Vec<crossings::TimingContrast>, timing:Vec<Range>;);
record!(crossings::CrossingAssessment; schema:String, request_hash:String, window_hash:String,
    baseline:TraceReport, proof:Vec<crossings::CrossingProof>, outcome:crossings::CrossingOutcome,
    scope:String, completed:bool, managed_reservation:usize, resource_qualification:String;);

record!(tw::TransportRequest; schema:String, dimension:usize, times:Vec<f64>,
    reference_nodes:Vec<Matrix>, nodes:Vec<Matrix>, block:[usize;2], initial_frame:Matrix,
    initial_gauge:Matrix, reference_speed_limit:f64, max_speed:f64, endpoint_error_limit:f64,
    initial_correction_limit:f64, initial_mismatch_limit:f64, target:f64, memory_bytes:usize;);
record!(tw::FrameProof; polar:f64, projector_error:f64;);
record!(tw::DistanceProof; eigensystem:Eigensystem, distance:f64;);
record!(tw::InitializationProof; requested_polar:f64, gauge_polar:f64, reference_basis:Matrix,
    basis:Matrix, reference_error:f64, basis_error:f64, correction:f64, mismatch:f64;);
record!(tw::StepProof; reference:tw::DistanceProof, reconstructed:tw::DistanceProof,
    rotation:Vec<tw::RotationProof>;);
record!(tw::RotationProof; matrix:Matrix, reference_angle:f64, angle:f64, local_error:f64,
    product_error:f64, frame:Matrix, frame_error:f64;);
record!(tw::TransportBudget; reference_speed:f64, speed:f64, endpoint_error:f64,
    initial_correction:f64, initial_mismatch:f64, product_error:f64, numerical_error:f64,
    mesh:f64, comparison_error:f64;);
record!(tw::CovarianceBudget; projector_speed:f64, endpoint_error:f64, initial_uncertainty:f64,
    mesh:f64, comparison_error:f64, total_error:f64;);
record!(tw::CovarianceProof; observation_indices:Vec<usize>, distances:Vec<tw::DistanceProof>,
    gaps:Vec<f64>, node_gaps:Vec<f64>, budget:Vec<tw::CovarianceBudget>, outcome:tw::Outcome,
    scope:String;);
record!(tw::TransportAssessment; schema:String, request_hash:String, reference_bounds:Vec<tw::FrameProof>,
    node_bounds:Vec<tw::FrameProof>, initial:Vec<tw::InitializationProof>, endpoints:Vec<tw::DistanceProof>,
    steps:Vec<tw::StepProof>, rotation:Vec<Matrix>, frame:Vec<Matrix>, budget:Vec<tw::TransportBudget>,
    covariance:Vec<tw::CovarianceProof>, outcome:tw::Outcome, scope:String, completed:bool,
    managed_reservation:usize, resource_qualification:String;);

impl DecodeOwned for tw::Gate {
    fn from_raw(raw: &RawValue, _: &Cancellation) -> Result<Self> {
        match raw.get() {
            "\"InitialFibre\"" => Ok(Self::InitialFibre),
            "\"InitialMismatch\"" => Ok(Self::InitialMismatch),
            "\"EndpointError\"" => Ok(Self::EndpointError),
            "\"CutLocus\"" => Ok(Self::CutLocus),
            "\"ReferenceSpeed\"" => Ok(Self::ReferenceSpeed),
            "\"Speed\"" => Ok(Self::Speed),
            "\"Accuracy\"" => Ok(Self::Accuracy),
            "\"CovarianceGap\"" => Ok(Self::CovarianceGap),
            "\"CovarianceInitial\"" => Ok(Self::CovarianceInitial),
            "\"CovarianceAccuracy\"" => Ok(Self::CovarianceAccuracy),
            _ => Err(invalid()),
        }
    }
}
impl DecodeOwned for tw::Outcome {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let fields = Record::<2>::parse(raw, &["kind", "gate"], cancel)?;
            match fields.tag(0)? {
                "Resolved" => {
                    fields.only(&[0])?;
                    Ok(Self::Resolved)
                }
                "Unresolved" => Ok(Self::Unresolved {
                    gate: fields.required(1, cancel)?,
                }),
                _ => Err(invalid()),
            }
        })
    }
}

impl DecodeOwned for crossings::Gate {
    fn from_raw(raw: &RawValue, _: &Cancellation) -> Result<Self> {
        match raw.get() {
            "\"StructuralPremise\"" => Ok(Self::StructuralPremise),
            "\"ExternalPlane\"" => Ok(Self::ExternalPlane),
            "\"SlopeSeparation\"" => Ok(Self::SlopeSeparation),
            "\"Perturbation\"" => Ok(Self::Perturbation),
            "\"TimingSlope\"" => Ok(Self::TimingSlope),
            "\"Attachments\"" => Ok(Self::Attachments),
            "\"TimingIntersection\"" => Ok(Self::TimingIntersection),
            _ => Err(invalid()),
        }
    }
}
impl DecodeOwned for crossings::CrossingOutcome {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let fields = Record::<2>::parse(raw, &["kind", "gate"], cancel)?;
            match fields.tag(0)? {
                "Resolved" => {
                    fields.only(&[0])?;
                    Ok(Self::Resolved)
                }
                "Unresolved" => Ok(Self::Unresolved {
                    gate: fields.required(1, cancel)?,
                }),
                _ => Err(invalid()),
            }
        })
    }
}

impl DecodeOwned for continuous::QueryState {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let fields = Record::<3>::parse(raw, &["kind", "trace", "blocks"], cancel)?;
            match fields.tag(0)? {
                "Bounds" => Ok(Self::Bounds {
                    trace: fields.required(1, cancel)?,
                    blocks: fields.required(2, cancel)?,
                }),
                "InconsistentOuterBounds" => {
                    fields.only(&[0])?;
                    Ok(Self::InconsistentOuterBounds)
                }
                _ => Err(invalid()),
            }
        })
    }
}

impl DecodeOwned for Calibration {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<8>::parse(
                raw,
                &[
                    "kind",
                    "radii",
                    "assumptions",
                    "observations",
                    "estimated_mean",
                    "divisor",
                    "alpha",
                    "auxiliary_risk",
                ],
                cancel,
            )?;
            match r.tag(0)? {
                "Conditional" => {
                    r.only(&[0, 1, 2])?;
                    Ok(Self::Conditional {
                        radii: r.required(1, cancel)?,
                        assumptions: r.required(2, cancel)?,
                    })
                }
                "Gaussian" => {
                    r.only(&[0, 3, 4, 5, 6, 7])?;
                    Ok(Self::Gaussian {
                        observations: r.required(3, cancel)?,
                        estimated_mean: r.required(4, cancel)?,
                        divisor: r.required(5, cancel)?,
                        alpha: r.required(6, cancel)?,
                        auxiliary_risk: r.required(7, cancel)?,
                    })
                }
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for Quantity {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<2>::parse(raw, &["kind", "value"], cancel)?;
            match r.tag(0)? {
                "Interval" => Ok(Self::Interval(r.required(1, cancel)?)),
                "Undefined" => Ok(Self::Undefined(r.required(1, cancel)?)),
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for Association {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<2>::parse(raw, &["kind", "matching"], cancel)?;
            match r.tag(0)? {
                "Unique" => Ok(Self::Unique(r.required(1, cancel)?)),
                "Alternatives" => {
                    r.only(&[0])?;
                    Ok(Self::Alternatives)
                }
                "NoPerfectMatching" => {
                    r.only(&[0])?;
                    Ok(Self::NoPerfectMatching)
                }
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for PathClass {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<3>::parse(raw, &["kind", "model", "windows"], cancel)?;
            match r.tag(0)? {
                "Continuous" => {
                    r.only(&[0])?;
                    Ok(Self::Continuous)
                }
                "Jumps" => {
                    r.only(&[0, 1])?;
                    Ok(Self::Jumps {
                        model: r.required(1, cancel)?,
                    })
                }
                "InitializedTransport" => {
                    r.only(&[0, 1])?;
                    Ok(Self::InitializedTransport {
                        model: r.required(1, cancel)?,
                    })
                }
                "CommonFrame" => {
                    r.only(&[0, 1])?;
                    Ok(Self::CommonFrame {
                        model: r.required(1, cancel)?,
                    })
                }
                "TransverseCrossings" => {
                    r.only(&[0, 2])?;
                    Ok(Self::TransverseCrossings {
                        windows: r.required(2, cancel)?,
                    })
                }
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for CellScope {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<4>::parse(raw, &["kind", "segment", "cell", "mandatory"], cancel)?;
            match r.tag(0)? {
                "Smooth" => {
                    r.only(&[0, 1])?;
                    Ok(Self::Smooth {
                        segment: r.required(1, cancel)?,
                    })
                }
                "JumpPossible" => {
                    r.only(&[0, 2, 3])?;
                    Ok(Self::JumpPossible {
                        cell: r.required(2, cancel)?,
                        mandatory: r.required(3, cancel)?,
                    })
                }
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for Feasibility {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<2>::parse(raw, &["kind", "additional"], cancel)?;
            match r.tag(0)? {
                "WidenedAffineWitness" => Ok(Self::WidenedAffineWitness {
                    additional: r.required(1, cancel)?,
                }),
                tag => {
                    r.only(&[0])?;
                    match tag {
                        "OriginalAffineWitness" => Ok(Self::OriginalAffineWitness),
                        "OriginalStepWitness" => Ok(Self::OriginalStepWitness),
                        "Unresolved" => Ok(Self::Unresolved),
                        "UnsupportedModel" => Ok(Self::UnsupportedModel),
                        _ => Err(invalid()),
                    }
                }
            }
        })
    }
}
impl DecodeOwned for CommonFrameAssessment {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<8>::parse(
                raw,
                &[
                    "kind",
                    "report",
                    "request_hash",
                    "model_hash",
                    "eigensystems",
                    "radii",
                    "calibration_scope",
                    "axes",
                ],
                cancel,
            )?;
            match r.tag(0)? {
                "Resolved" => {
                    r.only(&[0, 1])?;
                    Ok(Self::Resolved {
                        report: r.required(1, cancel)?,
                    })
                }
                "UnresolvedAnchor" => {
                    r.only(&[0, 2, 3, 4, 5, 6, 7])?;
                    Ok(Self::UnresolvedAnchor {
                        request_hash: r.required(2, cancel)?,
                        model_hash: r.required(3, cancel)?,
                        eigensystems: r.required(4, cancel)?,
                        radii: r.required(5, cancel)?,
                        calibration_scope: r.required(6, cancel)?,
                        axes: r.required(7, cancel)?,
                    })
                }
                _ => Err(invalid()),
            }
        })
    }
}
impl DecodeOwned for QueryState {
    fn from_raw(raw: &RawValue, cancel: &Cancellation) -> Result<Self> {
        memory::scope(|| {
            let r = Record::<3>::parse(raw, &["kind", "summaries", "trace"], cancel)?;
            match r.tag(0)? {
                "Bounds" => Ok(Self::Bounds {
                    summaries: r.required(1, cancel)?,
                    trace: r.required(2, cancel)?,
                }),
                "UnresolvedAnchor" => {
                    r.only(&[0])?;
                    Ok(Self::UnresolvedAnchor)
                }
                "InconsistentOuterBounds" => {
                    r.only(&[0])?;
                    Ok(Self::InconsistentOuterBounds)
                }
                _ => Err(invalid()),
            }
        })
    }
}
