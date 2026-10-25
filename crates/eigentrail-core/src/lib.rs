//! TRACE covariance tracking. Numerical proposals are independently checked.
pub mod association;
pub mod budget;
pub mod calibration;
pub mod common_frame;
pub mod common_frame_queries;
pub mod continuous_queries;
pub mod crossings;
pub mod curves;
pub mod families;
pub mod histories;
pub mod jump_queries;
pub mod jumps;
pub mod numerics;
pub mod resources;
pub mod scalar;
pub mod serialization;
pub mod summaries;
pub mod transport;
pub mod transport_workflow;
pub mod types;
use resources::Cancellation;
pub use types::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Invalid(String),
    Resource(String),
    Exhausted(String),
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

pub fn validate(request: &TraceRequest) -> Result<usize> {
    if request.schema != SCHEMA {
        return Err(Error::Invalid("unsupported schema".into()));
    }
    let p = request.features.len();
    let m = request.times.len();
    let reservation = resources::preflight(p, m, request.memory_bytes)?;
    if request.covariances.len() != m
        || request.correction.len() != m
        || request.times.iter().any(|t| !t.is_finite())
        || request.times.windows(2).any(|t| t[0] >= t[1])
    {
        return Err(Error::Invalid(
            "ordered finite times and one matrix/correction per date required".into(),
        ));
    }
    scalar::nonnegative(request.lipschitz, "Lipschitz bound")?;
    scalar::nonnegative(request.target, "target")?;
    scalar::nonnegative(request.numerical_allowance, "numerical allowance")?;
    for &v in &request.correction {
        scalar::nonnegative(v, "correction")?;
    }
    if let Calibration::Conditional { radii, assumptions } = &request.calibration {
        if radii.len() != m || assumptions.trim().is_empty() || assumptions.len() > 4096 {
            return Err(Error::Invalid(
                "one conditional radius per date and bounded assumptions text required".into(),
            ));
        }
        for &v in radii {
            scalar::nonnegative(v, "radius")?;
        }
    }
    let duration = scalar::point(request.times[m - 1]) - scalar::point(request.times[0]);
    scalar::upper(duration)?;
    scalar::upper(duration * scalar::point(request.lipschitz))?;
    for (index, name) in request.features.iter().enumerate() {
        if name.is_empty() || name.len() > 4096 || request.features[..index].contains(name) {
            return Err(Error::Invalid(
                "unique fixed feature coordinates required".into(),
            ));
        }
    }
    for a in &request.covariances {
        if a.n != p {
            return Err(Error::Invalid("feature dimension mismatch".into()));
        }
        numerics::validate_matrix(a)?;
    }
    Ok(reservation)
}

pub fn track(request: &TraceRequest) -> Result<TraceReport> {
    track_with_cancel(request, &Cancellation::default())
}
pub fn track_with_cancel(request: &TraceRequest, cancel: &Cancellation) -> Result<TraceReport> {
    cancel.poll()?;
    crate::validate(request)?;
    resources::memory::ensure_operation(
        request.memory_bytes,
        crate::resources::memory::HeapBytes::heap_bytes(request)?,
        || {
            cancel.poll()?;
            let reservation = validate(request)?;
            let matrices = calibration::normalized(request)?;
            let mut eigensystems = resources::memory::capacity(matrices.len())?;
            for a in &matrices {
                let e = numerics::eigensystem(a, cancel)?;
                if e.values.last().unwrap() + e.backward < 0. {
                    return Err(Error::Invalid("matrix is not positive semidefinite".into()));
                }
                if !histories::try_verified_psd(a, &e)? {
                    return Err(Error::Exhausted(
                        "positive-semidefinite input could not be verified".into(),
                    ));
                }
                eigensystems.push(e);
            }
            let (radii, calibration_scope) = calibration::radii(request, &eigensystems)?;
            let mut segments = resources::memory::capacity(matrices.len() - 1)?;
            for j in 0..matrices.len() - 1 {
                cancel.poll()?;
                let dt =
                    (scalar::point(request.times[j + 1]) - scalar::point(request.times[j])).sup();
                let mut tubes = curves::tubes(
                    &eigensystems[j],
                    &eigensystems[j + 1],
                    radii[j],
                    radii[j + 1],
                    request.lipschitz,
                    dt,
                    request.target,
                )?;
                for tube in &mut tubes {
                    tube.informative &=
                        budget::numerical_component(request, &eigensystems, j, tube)?
                            <= request.numerical_allowance;
                }
                let edges = association::graph(
                    &eigensystems[j],
                    &eigensystems[j + 1],
                    &tubes,
                    request.lipschitz,
                    dt,
                    cancel,
                )?;
                let association = association::classify_checked(&edges)?;
                segments.push(Segment {
                    tubes,
                    edges,
                    association,
                });
            }
            let mut summaries = resources::memory::capacity(eigensystems.len())?;
            for (e, r) in eigensystems.iter().zip(&radii) {
                summaries.push(summaries::try_summaries(e, *r)?);
            }
            let witness = resources::memory::text(
                match histories::witness(request, &matrices, &radii, &matrices, 0., cancel)? {
                    histories::Witness::Original => {
                        "checked affine witness in original bare continuous class"
                    }
                    _ => "unresolved: observed chain did not verify; no infeasibility claim",
                },
            )?;
            Ok(TraceReport {
                schema: resources::memory::text(SCHEMA)?,
                request_hash: serialization::digest_with_cancel(request, cancel)?,
                eigensystems,
                radii,
                segments,
                summaries,
                calibration_scope,
                witness,
                completed: true,
                managed_reservation: reservation,
                resource_qualification: resources::memory::text(
                    "pending audit of native allocations and interruption latency",
                )?,
            })
        },
    )
}
