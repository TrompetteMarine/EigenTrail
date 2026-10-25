//! Reconstruct every jump decision from intended input and checked proposals.
use super::{eigensystem, refusal};
use eigentrail_core::{
    jumps::{JumpModel, JumpReport},
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};

pub fn check(request: &TraceRequest, model: &JumpModel, report: &JumpReport) -> Result<()> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        model.heap_bytes()?,
        report.heap_bytes()?,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        check_with_cancel(request, model, report, &Cancellation::default())
    })
}

pub fn check_with_cancel(
    request: &TraceRequest,
    model: &JumpModel,
    report: &JumpReport,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[
        request.heap_bytes()?,
        model.heap_bytes()?,
        report.heap_bytes()?,
    ])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        cancel.poll()?;
        let reservation = validate(request)?;
        if !model.at_most_one_per_cell {
            return Err(refusal("jump spacing premise required"));
        }
        if let Some(signal) = model.minimum_jump {
            nonnegative(signal, "candidate minimum jump")?;
        }
        if report.schema != eigentrail_core::jumps::SCHEMA
            || !report.completed
            || report.request_hash != serialization::digest_with_cancel(request, cancel)?
            || report.model_hash != serialization::digest_with_cancel(model, cancel)?
            || report.managed_reservation != reservation
            || report.resource_qualification
                != "pending audit of native allocations and interruption latency"
            || report.family.scope != eigentrail_core::jumps::SCOPE
        {
            return Err(refusal(
                "jump binding, scope, completion or resource promotion",
            ));
        }
        let matrices = super::calibration::inspect(
            request,
            &report.eigensystems,
            &report.radii,
            &report.calibration_scope,
            cancel,
        )?;
        let count = matrices.len() - 1;
        if report.differences.len() != count
            || report.family.cells.len() != count
            || report.family.mandatory.len() != count
            || report.family.optional.len() != count
        {
            return Err(refusal("incomplete jump candidate family"));
        }
        for j in 0..count {
            memory::scope(|| {
                cancel.poll()?;
                let left = &matrices[j];
                let right = &matrices[j + 1];
                let mut data = resources::zeros(left.n * left.n)?;
                let mut rounding = point(0.);
                for (index, value) in data.iter_mut().enumerate() {
                    *value = right.data[index] - left.data[index];
                    if !value.is_finite() {
                        return Err(refusal("jump increment overflow"));
                    }
                    rounding +=
                        square(point(right.data[index]) - point(left.data[index]) - point(*value));
                }
                let increment = Matrix { n: left.n, data };
                let proposal = &report.differences[j];
                eigensystem::inspect(&increment, proposal, cancel)?;
                let spectral_norm = proposal.values.iter().fold(0f64, |a, b| a.max(b.abs()));
                let lower = (point(spectral_norm) - point(proposal.backward) - rounding.sqrt())
                    .inf()
                    .max(0.);
                let upper = eigentrail_core::scalar::upper(
                    point(spectral_norm) + point(proposal.backward) + rounding.sqrt(),
                )?;
                let width = (point(upper) - point(lower)).sup();
                if !lower.is_finite() || !upper.is_finite() || !width.is_finite() {
                    return Err(refusal("nonfinite jump norm enclosure"));
                }
                let noise = point(report.radii[j]) + point(report.radii[j + 1]);
                let dt = point(request.times[j + 1]) - point(request.times[j]);
                let drift = point(request.lipschitz) * dt;
                let mandatory = lower > eigentrail_core::scalar::upper(drift + noise)?;
                // The finite norm width remains charged even with an otherwise strict signal margin.
                let excludes_hidden = model.minimum_applies_to_all_candidates
                    && model.minimum_jump.is_some_and(|signal| {
                        signal
                            > (point(2.) * point(request.lipschitz) * point(dt.sup())
                                + point(2.) * noise
                                + point(width))
                            .sup()
                    });
                let cell = &report.family.cells[j];
                if report.family.mandatory[j] != mandatory
                    || report.family.optional[j] != (!mandatory && !excludes_hidden)
                    || cell.flagged != mandatory
                    || cell.amplitude
                        != Range::closed(
                            (point(lower) - noise - drift).inf().max(0.),
                            eigentrail_core::scalar::upper(
                                point(lower) + point(width) + noise + drift,
                            )?,
                        )
                    || cell.timing
                        != (Range {
                            lower: request.times[j],
                            upper: request.times[j + 1],
                            lower_closed: false,
                            upper_closed: true,
                        })
                    || cell.alternative
                        != if mandatory {
                            "at least one jump under declared spacing model"
                        } else {
                            "smooth and hidden-jump candidates retained"
                        }
                {
                    return Err(refusal(
                        "jump flag, amplitude, timing or retained alternatives",
                    ));
                }

                Ok(())
            })?;
        }
        cancel.poll()?;
        Ok(())
    })
}
