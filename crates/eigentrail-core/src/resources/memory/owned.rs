use super::{HeapBytes, refused};
use crate::*;
fn sum(values: &[usize]) -> Result<usize> {
    values
        .iter()
        .try_fold(0usize, |s, &v| s.checked_add(v).ok_or_else(refused))
}
impl HeapBytes for String {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(self.capacity())
    }
}
impl<T: HeapBytes> HeapBytes for Vec<T> {
    fn heap_bytes(&self) -> Result<usize> {
        self.iter().try_fold(
            self.capacity()
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(refused)?,
            |s, v| s.checked_add(v.heap_bytes()?).ok_or_else(refused),
        )
    }
}
impl<T: HeapBytes> HeapBytes for Option<T> {
    fn heap_bytes(&self) -> Result<usize> {
        self.as_ref().map_or(Ok(0), HeapBytes::heap_bytes)
    }
}
impl<T: HeapBytes, E: HeapBytes> HeapBytes for std::result::Result<T, E> {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Ok(v) => v.heap_bytes(),
            Err(e) => e.heap_bytes(),
        }
    }
}
impl<T: HeapBytes, const N: usize> HeapBytes for [T; N] {
    fn heap_bytes(&self) -> Result<usize> {
        self.iter().try_fold(0usize, |s, v| {
            s.checked_add(v.heap_bytes()?).ok_or_else(refused)
        })
    }
}
macro_rules! zero {($($t:ty),*)=>{$(impl HeapBytes for $t{fn heap_bytes(&self)->Result<usize>{Ok(0)}})*};}
zero!(
    histories::Witness,
    (),
    bool,
    u8,
    usize,
    u64,
    f64,
    Range,
    Tube,
    Error,
    inari::Interval,
    families::CellScope,
    families::PartitionOverlap,
    families::Feasibility,
    jumps::JumpModel,
    crossings::CrossingWindow,
    crossings::CollisionBudget,
    crossings::Gate,
    crossings::CrossingOutcome,
    crossings::TimingContrast,
    transport::LineTransport,
    transport::InitializedLine,
    transport_workflow::FrameProof,
    transport_workflow::TransportBudget,
    transport_workflow::CovarianceBudget,
    transport_workflow::Outcome,
    transport_workflow::Gate
);
macro_rules! tuple {($($t:ident:$i:tt),*)=>{impl<$($t:HeapBytes),*> HeapBytes for ($($t,)*) {fn heap_bytes(&self)->Result<usize>{sum(&[$(self.$i.heap_bytes()?),*])}}};}
tuple!(A:0,B:1);
tuple!(A:0,B:1,C:2);
tuple!(A:0,B:1,C:2,D:3);
macro_rules! fields {($t:ty;$($field:ident),*)=>{impl HeapBytes for $t{fn heap_bytes(&self)->Result<usize>{sum(&[$(self.$field.heap_bytes()?),*])}}};}
fields!(Matrix;data);
fields!(Eigensystem;values,vectors);
fields!(TraceRequest;schema,times,covariances,features,calibration,correction);
fields!(TraceReport;schema,request_hash,eigensystems,radii,segments,summaries,calibration_scope,witness,resource_qualification);
fields!(Segment;tubes,edges,association);
fields!(Summary;share);
impl HeapBytes for Calibration {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Conditional { radii, assumptions } => {
                sum(&[radii.heap_bytes()?, assumptions.heap_bytes()?])
            }
            Self::Gaussian { .. } => Ok(0),
        }
    }
}
impl HeapBytes for Association {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Unique(v) => v.heap_bytes(),
            _ => Ok(0),
        }
    }
}
impl HeapBytes for Quantity {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Undefined(s) => s.heap_bytes(),
            _ => Ok(0),
        }
    }
}
fields!(common_frame::CommonFrameModel;weights);
fields!(common_frame::CommonFrame;axes,labelled_strengths,possible_ranks);
fields!(common_frame::CommonFrameReport;schema,request_hash,model_hash,eigensystems,radii,calibration_scope,frame,scope,resource_qualification);
impl HeapBytes for common_frame::CommonFrameAssessment {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Resolved { report } => report.heap_bytes(),
            Self::UnresolvedAnchor {
                request_hash,
                model_hash,
                eigensystems,
                radii,
                calibration_scope,
                axes,
            } => sum(&[
                request_hash.heap_bytes()?,
                model_hash.heap_bytes()?,
                eigensystems.heap_bytes()?,
                radii.heap_bytes()?,
                calibration_scope.heap_bytes()?,
                axes.heap_bytes()?,
            ]),
        }
    }
}
fields!(common_frame_queries::Workflow;queries,witnesses,scope);
fields!(common_frame_queries::QueryRow;state);
fields!(common_frame_queries::PolarAffineWitness;diagonal_nodes,nodal_error_bounds,step_norm_bounds);
impl HeapBytes for common_frame_queries::QueryState {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Bounds { summaries, .. } => summaries.heap_bytes(),
            _ => Ok(0),
        }
    }
}
fields!(families::AffineWitness;nodes);
fields!(families::Configuration;id,class,witnesses);
fields!(families::HistoryRequest;schema,observations,configurations,query_times);
fields!(families::GridNode;observations,queries);
fields!(families::ConfigurationReport;id,class,grid,continuous_conditional,jumps,common_frame,common_frame_queries,continuous_workflow,crossings,transports,jump_workflow,cells,partition_overlaps);
fields!(families::HistoryReport;schema,request_hash,configurations,scope,resource_qualification);
impl HeapBytes for families::PathClass {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::CommonFrame { model } => model.heap_bytes(),
            Self::TransverseCrossings { windows } => windows.heap_bytes(),
            Self::InitializedTransport { model } => model.heap_bytes(),
            _ => Ok(0),
        }
    }
}
fields!(jumps::JumpCell;alternative);
fields!(jumps::JumpFamily;cells,mandatory,optional,scope);
fields!(jumps::JumpReport;schema,request_hash,model_hash,eigensystems,radii,differences,family,calibration_scope,resource_qualification);
fields!(curves::CentreEvaluation;schema,request_hash,eigensystem);
fields!(continuous_queries::Workflow;scope,lineage,queries,witnesses);
fields!(continuous_queries::SegmentLineage;start,end);
fields!(continuous_queries::Origin;origins);
fields!(continuous_queries::Query;centre,state);
fields!(continuous_queries::BlockSummary;origins,share,local_displacement,reference_displacement);
fields!(continuous_queries::QueryWitness;values,evaluation_errors);
impl HeapBytes for continuous_queries::QueryState {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Bounds { blocks, .. } => blocks.heap_bytes(),
            Self::InconsistentOuterBounds => Ok(0),
        }
    }
}
fields!(crossings::CrossingReport;slope_anchors,slopes,left,right,scope);
fields!(crossings::CrossingProof;compressed,slope_anchors,budget,left_errors,right_errors,left_candidates,right_candidates,left,right,contrasts,timing);
fields!(crossings::CrossingAssessment;schema,request_hash,window_hash,baseline,proof,scope,resource_qualification);
fields!(transport::BlockTransport;rotation);

fields!(transport_workflow::TransportRequest;schema,times,reference_nodes,nodes,initial_frame,initial_gauge);
fields!(transport_workflow::DistanceProof;eigensystem);
fields!(transport_workflow::InitializationProof;reference_basis,basis);
fields!(transport_workflow::StepProof;reference,reconstructed,rotation);
fields!(transport_workflow::RotationProof;matrix,frame);
fields!(transport_workflow::CovarianceProof;observation_indices,distances,gaps,node_gaps,budget,outcome,scope);
fields!(transport_workflow::TransportAssessment;schema,request_hash,reference_bounds,node_bounds,initial,endpoints,steps,rotation,frame,budget,covariance,outcome,scope,resource_qualification);

fields!(jump_queries::Workflow;scope,components,queries,witnesses);
fields!(jump_queries::Component;lineage);
fields!(jump_queries::Query;state);
fields!(jump_queries::Side;modes);
fields!(jump_queries::StepWitness;cells,values);
impl HeapBytes for jump_queries::State {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Observed { modes, .. } => modes.heap_bytes(),
            Self::Smooth { query, .. } => query.heap_bytes(),
            Self::JumpPossible { sides, .. } => sides.heap_bytes(),
        }
    }
}

fields!(lineage_summary::Request; schema,history);
fields!(lineage_summary::Report; schema,request_hash,history,rows,resource_qualification);
fields!(lineage_summary::Row; state);
zero!(lineage_summary::InfoLevel, lineage_summary::Motion);
impl HeapBytes for lineage_summary::Target {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::OrderedBlock { origins, .. } => origins.heap_bytes(),
            Self::AnchorAxis { .. } => Ok(0),
        }
    }
}
impl HeapBytes for lineage_summary::RowState {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Bounds {
                target,
                share,
                reference_displacement,
                ..
            } => sum(&[
                target.heap_bytes()?,
                share.heap_bytes()?,
                reference_displacement.heap_bytes()?,
            ]),
            Self::UnresolvedAnchor | Self::InconsistentOuterBounds => Ok(0),
        }
    }
}
