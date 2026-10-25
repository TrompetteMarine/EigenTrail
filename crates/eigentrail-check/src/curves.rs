use eigentrail_core::{
    curves::CentreEvaluation,
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
    scalar::*,
    *,
};
pub fn check_evaluation(
    request: &TraceRequest,
    evaluation: &CentreEvaluation,
    cancel: &Cancellation,
) -> Result<()> {
    let retained = crate::retained_bytes(&[request.heap_bytes()?, evaluation.heap_bytes()?])?;
    memory::ensure_operation(request.memory_bytes, retained, || {
        validate(request)?;
        let j = evaluation.segment;
        let t = evaluation.time;
        if evaluation.schema != SCHEMA
            || evaluation.request_hash != serialization::digest_with_cancel(request, cancel)?
            || !t.is_finite()
            || j.checked_add(1).is_none_or(|k| k >= request.times.len())
            || t < request.times[j]
            || t > request.times[j + 1]
        {
            return Err(Error::Invalid("checker: evaluation scope".into()));
        }
        let matrices = calibration::normalized(request)?;
        let p = request.features.len();
        let weight = (point(t) - point(request.times[j]))
            / (point(request.times[j + 1]) - point(request.times[j]));
        let mut matrix = Matrix {
            n: p,
            data: resources::zeros(p * p)?,
        };
        let mut error = point(0.);
        for (i, value) in matrix.data.iter_mut().enumerate() {
            let ideal = (point(1.) - weight) * point(matrices[j].data[i])
                + weight * point(matrices[j + 1].data[i]);
            *value = ideal.inf() / 2. + ideal.sup() / 2.;
            error += square(ideal - point(*value));
        }
        let normal = calibration::normalized_error(request, j)?
            .max(calibration::normalized_error(request, j + 1)?);
        if !evaluation.evaluation_error.is_finite()
            || evaluation.evaluation_error < (error.sqrt() + point(normal)).sup()
        {
            return Err(Error::Invalid(
                "checker: understated interior evaluation error".into(),
            ));
        }
        crate::eigensystem::inspect(&matrix, &evaluation.eigensystem, cancel)?;
        Ok(())
    })
}
