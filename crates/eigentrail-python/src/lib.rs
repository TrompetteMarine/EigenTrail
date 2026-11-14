//! Thin Python boundary.
use eigentrail_core::{
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes, Managed, Operation},
        worker::Worker,
    },
    serialization::input::{self, RequestKind},
    *,
};
use numpy::PyReadonlyArray3;
use pyo3::{exceptions::PyValueError, prelude::*};

fn py_error(e: Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
fn with_interrupt<T: Send + HeapBytes>(
    py: Python<'_>,
    operation: Operation,
    work: impl FnOnce(Cancellation) -> Result<T> + Send,
) -> PyResult<T> {
    std::thread::scope(|scope| {
        let mut worker = Worker::spawn(scope, &operation, work).map_err(py_error)?;
        loop {
            let waiting = &mut worker;
            match py.detach(move || waiting.recv_timeout(std::time::Duration::from_millis(25))) {
                Ok(r) => return r.map(Managed::into_inner).map_err(py_error),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(PyValueError::new_err(
                        "worker terminated without a completed certificate",
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(e) = py.check_signals() {
                        worker.cancel();
                        return Err(e);
                    }
                }
            }
        }
    })
}

fn preflight_json_inputs(
    py: Python<'_>,
    request: &str,
    companions: &[&str],
    kind: RequestKind,
) -> PyResult<usize> {
    let mut signal_error = None;
    let result = input::preflight_with_poll(request, companions, kind, || {
        py.check_signals().map_err(|error| {
            signal_error = Some(error);
            Error::Cancelled
        })
    });
    match signal_error {
        Some(error) => Err(error),
        None => result.map_err(py_error),
    }
}

fn copy_json_text(py: Python<'_>, operation: &Operation, text: &str) -> PyResult<Managed<String>> {
    prepare(operation, || {
        py.check_signals()?;
        let mut bytes = memory::capacity(text.len()).map_err(py_error)?;
        for chunk in text.as_bytes().chunks(4096) {
            py.check_signals()?;
            bytes.extend_from_slice(chunk);
        }
        Ok(String::from_utf8(bytes).expect("copied input UTF-8"))
    })
}

fn prepare<T: HeapBytes>(
    operation: &Operation,
    work: impl FnOnce() -> PyResult<T>,
) -> PyResult<Managed<T>> {
    let mut language_error = None;
    let prepared = operation.own(|| match work() {
        Ok(value) => Ok(value),
        Err(error) => {
            language_error = Some(error);
            Err(Error::Cancelled)
        }
    });
    match language_error {
        Some(error) => Err(error),
        None => prepared.map_err(py_error),
    }
}

fn run(py: Python<'_>, operation: Operation, request: Managed<TraceRequest>) -> PyResult<String> {
    with_interrupt(py, operation, move |cancel| {
        let report = track_with_cancel(&request, &cancel)?;
        eigentrail_check::check_with_cancel(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}

#[pyfunction]
fn track_json(py: Python<'_>, request: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = track_with_cancel(&req, &cancel)?;
        eigentrail_check::check_with_cancel(&req, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_json(py: Python<'_>, request: &str, report: &str) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let report_text = copy_json_text(py, &operation, report)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let rep: TraceReport = serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::check_with_cancel(&req, &rep, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn analyze_jumps_json(py: Python<'_>, request: &str, model: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[model], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let model_text = copy_json_text(py, &operation, model)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: jumps::JumpModel = serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report = jumps::analyze_with_cancel(&req, &model, &cancel)?;
        eigentrail_check::jumps::check_with_cancel(&req, &model, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_jumps_json(py: Python<'_>, request: &str, model: &str, report: &str) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[model, report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let model_text = copy_json_text(py, &operation, model)?;
    let report_text = copy_json_text(py, &operation, report)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: jumps::JumpModel = serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report: jumps::JumpReport = serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::jumps::check_with_cancel(&req, &model, &report, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn analyze_transport_json(py: Python<'_>, request: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[], RequestKind::Transport)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    with_interrupt(py, operation, move |cancel| {
        let request: transport_workflow::TransportRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let assessment = transport_workflow::analyze(&request, &cancel)?;
        eigentrail_check::transport::check(&request, &assessment, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&assessment, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_transport_json(py: Python<'_>, request: &str, assessment: &str) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[assessment], RequestKind::Transport)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let assessment_text = copy_json_text(py, &operation, assessment)?;
    with_interrupt(py, operation, move |cancel| {
        let request: transport_workflow::TransportRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let assessment: transport_workflow::TransportAssessment =
            serialization::decode_in(assessment_text.as_bytes(), &cancel)?;
        drop(assessment_text);
        eigentrail_check::transport::check(&request, &assessment, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn analyze_crossings_json(py: Python<'_>, request: &str, window: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[window], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let window_text = copy_json_text(py, &operation, window)?;
    with_interrupt(py, operation, move |cancel| {
        let request: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let window: crossings::CrossingWindow =
            serialization::decode_in(window_text.as_bytes(), &cancel)?;
        drop(window_text);
        let assessment = crossings::analyze(&request, &window, &cancel)?;
        eigentrail_check::crossings::check(&request, &window, &assessment, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&assessment, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_crossings_json(
    py: Python<'_>,
    request: &str,
    window: &str,
    assessment: &str,
) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[window, assessment], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let window_text = copy_json_text(py, &operation, window)?;
    let assessment_text = copy_json_text(py, &operation, assessment)?;
    with_interrupt(py, operation, move |cancel| {
        let request: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let window: crossings::CrossingWindow =
            serialization::decode_in(window_text.as_bytes(), &cancel)?;
        drop(window_text);
        let assessment: crossings::CrossingAssessment =
            serialization::decode_in(assessment_text.as_bytes(), &cancel)?;
        drop(assessment_text);
        eigentrail_check::crossings::check(&request, &window, &assessment, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn track_covariances(
    py: Python<'_>,
    data: PyReadonlyArray3<'_, f64>,
    times: &Bound<'_, PyAny>,
    radii: &Bound<'_, PyAny>,
    lipschitz: f64,
) -> PyResult<String> {
    let (m, p) = covariance_shape(&data)?;
    resources::preflight(p, m, resources::DEFAULT_BYTES).map_err(py_error)?;
    check_metadata_length(times, m, "times")?;
    check_metadata_length(radii, m, "radii")?;
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(py_error)?;
    let request = prepare(&operation, || {
        let times = copy_metadata(py, times, m, "times")?;
        let radii = copy_metadata(py, radii, m, "radii")?;
        validate_metadata(&times, &radii, lipschitz)?;
        let matrices = copy_covariances(py, &data, m, p)?;
        TraceRequest::try_conditional(times, matrices, radii, lipschitz).map_err(py_error)
    })?;
    run(py, operation, request)
}

fn covariance_shape(data: &PyReadonlyArray3<'_, f64>) -> PyResult<(usize, usize)> {
    let array = data.as_array();
    let shape = array.shape();
    let (m, p) = (shape[0], shape[1]);
    if shape[2] != p {
        return Err(PyValueError::new_err("expected (dates,features,features)"));
    }
    Ok((m, p))
}

fn metadata_length(input: &Bound<'_, PyAny>) -> PyResult<Option<usize>> {
    if input.hasattr("__len__")? {
        input.len().map(Some)
    } else {
        Ok(None)
    }
}

fn check_metadata_length(input: &Bound<'_, PyAny>, expected: usize, name: &str) -> PyResult<()> {
    if metadata_length(input)?.is_some_and(|length| length != expected) {
        return Err(py_error(Error::Invalid(format!(
            "expected {expected} {name} values"
        ))));
    }
    Ok(())
}

fn copy_metadata(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    count: usize,
    name: &str,
) -> PyResult<Vec<f64>> {
    py.check_signals()?;
    let mut values = resources::zeros(count).map_err(py_error)?;
    let mut iterator = input.try_iter()?;
    for value in &mut values {
        py.check_signals()?;
        let item = iterator
            .next()
            .transpose()?
            .ok_or_else(|| py_error(Error::Invalid(format!("expected {count} {name} values"))))?;
        *value = item.extract::<f64>()?;
    }
    py.check_signals()?;
    if iterator.next().transpose()?.is_some() {
        return Err(py_error(Error::Invalid(format!(
            "expected {count} {name} values"
        ))));
    }
    Ok(values)
}

fn copy_window_values(py: Python<'_>, input: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<f64>> {
    if let Some(length) = metadata_length(input)? {
        if !(1..=window::MAX_POINTS).contains(&length) {
            return Err(py_error(Error::Invalid(
                "window vectors require 1..=10000 entries".into(),
            )));
        }
        return copy_metadata(py, input, length, name);
    }
    let mut values = Vec::new();
    for item in input.try_iter()? {
        py.check_signals()?;
        if values.len() == window::MAX_POINTS {
            return Err(py_error(Error::Invalid(
                "window vectors require 1..=10000 entries".into(),
            )));
        }
        memory::push(&mut values, item?.extract::<f64>()?).map_err(py_error)?;
    }
    if values.is_empty() {
        return Err(py_error(Error::Invalid(
            "window vectors require 1..=10000 entries".into(),
        )));
    }
    Ok(values)
}

// Keep the observation contract explicit rather than deriving it from samples.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
fn window_envelope_json(
    py: Python<'_>,
    sample_times: &Bound<'_, PyAny>,
    weights: &Bound<'_, PyAny>,
    target_time: f64,
    window_radius: f64,
    lipschitz: f64,
    target_mismatch_bound: f64,
    target_description: &str,
    assumptions: &str,
) -> PyResult<String> {
    for values in [sample_times, weights] {
        if metadata_length(values)?.is_some_and(|n| !(1..=window::MAX_POINTS).contains(&n)) {
            return Err(py_error(Error::Invalid(
                "window vectors require 1..=10000 entries".into(),
            )));
        }
    }
    for text in [target_description, assumptions] {
        if text.trim().is_empty() || text.len() > window::MAX_TEXT_BYTES {
            return Err(py_error(Error::Invalid(
                "nonempty window target and assumptions of at most 4096 UTF-8 bytes required"
                    .into(),
            )));
        }
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(py_error)?;
    let request = prepare(&operation, || {
        let request = window::WindowRequest {
            sample_times: copy_window_values(py, sample_times, "sample time")?,
            weights: copy_window_values(py, weights, "weight")?,
            target_time,
            window_radius,
            lipschitz,
            target_mismatch_bound,
            target_description: memory::text(target_description).map_err(py_error)?,
            assumptions: memory::text(assumptions).map_err(py_error)?,
        };
        window::validate(&request).map_err(py_error)?;
        Ok(request)
    })?;
    with_interrupt(py, operation, move |cancel| {
        String::from_utf8(eigentrail_check::window::encode_checked(&request, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
fn temporal_gaussian_json(
    py: Python<'_>,
    dimension: u32,
    observations: u32,
    windows: u32,
    covariance_bound: f64,
    correlation_bound: f64,
    alpha: f64,
    assumptions: &str,
) -> PyResult<String> {
    if assumptions.trim().is_empty() || assumptions.len() > temporal::MAX_TEXT_BYTES {
        return Err(py_error(Error::Invalid(
            "nonempty temporal assumptions of at most 4096 UTF-8 bytes required".into(),
        )));
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(py_error)?;
    let request = prepare(&operation, || {
        py.check_signals()?;
        let request = temporal::TemporalRequest {
            dimension,
            observations,
            windows,
            covariance_bound,
            correlation_bound,
            alpha,
            assumptions: memory::text(assumptions).map_err(py_error)?,
        };
        temporal::validate(&request).map_err(py_error)?;
        Ok(request)
    })?;
    with_interrupt(py, operation, move |cancel| {
        String::from_utf8(eigentrail_check::temporal::encode_checked(
            &request, &cancel,
        )?)
        .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}

#[pyfunction]
fn spd_tube_geometry_json(
    py: Python<'_>,
    dimension: u32,
    centre_floor: f64,
    radius: f64,
) -> PyResult<String> {
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(py_error)?;
    let request = prepare(&operation, || {
        py.check_signals()?;
        let request = geometry::SpdTubeRequest {
            dimension,
            centre_floor,
            radius,
        };
        geometry::validate_spd(&request).map_err(py_error)?;
        Ok(request)
    })?;
    with_interrupt(py, operation, move |cancel| {
        String::from_utf8(eigentrail_check::geometry::encode_checked_spd(
            &request, &cancel,
        )?)
        .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}

fn copy_queries(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    known_length: Option<usize>,
    features: usize,
    dates: usize,
) -> PyResult<Vec<f64>> {
    if let Some(length) = known_length {
        return copy_metadata(py, input, length, "query time");
    }
    let mut values = Vec::new();
    for item in input.try_iter()? {
        py.check_signals()?;
        let item = item?;
        families::preflight_common_frame(
            features,
            dates,
            values.len() + 1,
            resources::DEFAULT_BYTES,
        )
        .map_err(py_error)?;
        memory::push(&mut values, item.extract::<f64>()?).map_err(py_error)?;
    }
    Ok(values)
}

fn validate_metadata(times: &[f64], radii: &[f64], lipschitz: f64) -> PyResult<()> {
    scalar::nonnegative(lipschitz, "global covariance regularity").map_err(py_error)?;
    if times.iter().any(|t| !t.is_finite())
        || times.windows(2).any(|pair| pair[0] >= pair[1])
        || radii.iter().any(|r| !r.is_finite() || *r < 0.)
    {
        return Err(py_error(Error::Invalid(
            "strictly increasing finite times and nonnegative finite radii required".into(),
        )));
    }
    scalar::upper(
        scalar::point(lipschitz)
            * (scalar::point(*times.last().unwrap()) - scalar::point(times[0])),
    )
    .map_err(py_error)?;
    Ok(())
}

fn copy_covariances(
    py: Python<'_>,
    data: &PyReadonlyArray3<'_, f64>,
    m: usize,
    p: usize,
) -> PyResult<Vec<Matrix>> {
    if covariance_shape(data)? != (m, p) {
        return Err(py_error(Error::Invalid(
            "covariance shape changed during metadata conversion".into(),
        )));
    }
    let mut matrices = memory::capacity(m).map_err(py_error)?;
    for d in 0..m {
        py.check_signals()?;
        let array = data.as_array();
        if array.shape() != [m, p, p] {
            return Err(py_error(Error::Invalid(
                "covariance shape changed during input copying".into(),
            )));
        }
        let mut values = resources::zeros(p * p).map_err(py_error)?;
        for j in 0..p {
            for i in 0..p {
                values[i + j * p] = array[[d, i, j]];
            }
        }
        matrices.push(Matrix { n: p, data: values });
    }
    Ok(matrices)
}

#[pyfunction(signature = (data, times, radii, lipschitz, query_times, common_frame_weights=None, *, witness=None))]
#[allow(clippy::too_many_arguments)] // Preserve the public positional metadata arguments.
fn track_history_covariances(
    py: Python<'_>,
    data: PyReadonlyArray3<'_, f64>,
    times: &Bound<'_, PyAny>,
    radii: &Bound<'_, PyAny>,
    lipschitz: f64,
    query_times: &Bound<'_, PyAny>,
    common_frame_weights: Option<&Bound<'_, PyAny>>,
    witness: Option<PyReadonlyArray3<'_, f64>>,
) -> PyResult<String> {
    track_history_arrays(
        py,
        data,
        times,
        radii,
        lipschitz,
        query_times,
        common_frame_weights,
        witness,
        None,
    )
}

#[pyfunction(signature = (data, times, radii, lipschitz, query_times, tolerance, common_frame_weights=None, *, witness=None))]
#[allow(clippy::too_many_arguments)] // Keep the separate stability route compatible with array metadata.
fn track_stability_covariances(
    py: Python<'_>,
    data: PyReadonlyArray3<'_, f64>,
    times: &Bound<'_, PyAny>,
    radii: &Bound<'_, PyAny>,
    lipschitz: f64,
    query_times: &Bound<'_, PyAny>,
    tolerance: f64,
    common_frame_weights: Option<&Bound<'_, PyAny>>,
    witness: Option<PyReadonlyArray3<'_, f64>>,
) -> PyResult<String> {
    track_history_arrays(
        py,
        data,
        times,
        radii,
        lipschitz,
        query_times,
        common_frame_weights,
        witness,
        Some(tolerance),
    )
}

#[allow(clippy::too_many_arguments)] // Both public routes share the same managed input boundary.
fn track_history_arrays(
    py: Python<'_>,
    data: PyReadonlyArray3<'_, f64>,
    times: &Bound<'_, PyAny>,
    radii: &Bound<'_, PyAny>,
    lipschitz: f64,
    query_times: &Bound<'_, PyAny>,
    common_frame_weights: Option<&Bound<'_, PyAny>>,
    witness: Option<PyReadonlyArray3<'_, f64>>,
    stability_tolerance: Option<f64>,
) -> PyResult<String> {
    if let Some(tolerance) = stability_tolerance {
        stability::validate_tolerance(tolerance).map_err(py_error)?;
        if common_frame_weights.is_some() {
            return Err(py_error(Error::Invalid(
                "stability assessment requires the continuous history model".into(),
            )));
        }
    }
    let (m, p) = covariance_shape(&data)?;
    resources::preflight(p, m, resources::DEFAULT_BYTES).map_err(py_error)?;
    if let Some(nodes) = &witness
        && covariance_shape(nodes)? != (m, p)
    {
        return Err(py_error(Error::Invalid(
            "witness shape must match the observation covariance array".into(),
        )));
    }
    check_metadata_length(times, m, "times")?;
    check_metadata_length(radii, m, "radii")?;
    if let Some(weights) = common_frame_weights {
        check_metadata_length(weights, m, "common-frame weights")?;
    }
    let query_length = metadata_length(query_times)?;
    families::preflight_common_frame(p, m, query_length.unwrap_or(0), resources::DEFAULT_BYTES)
        .map_err(py_error)?;
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(py_error)?;
    let request = prepare(&operation, || {
        let query_times = copy_queries(py, query_times, query_length, p, m)?;
        let times = copy_metadata(py, times, m, "times")?;
        let radii = copy_metadata(py, radii, m, "radii")?;
        validate_metadata(&times, &radii, lipschitz)?;
        let class = if let Some(weights) = common_frame_weights {
            let model = common_frame::CommonFrameModel {
                weights: copy_metadata(py, weights, m, "common-frame weights")?,
                common_frame_assumed: true,
            };
            common_frame::validate_model(&model, m).map_err(py_error)?;
            families::PathClass::CommonFrame { model }
        } else {
            families::PathClass::Continuous
        };
        if query_times
            .iter()
            .any(|t| !t.is_finite() || *t < times[0] || *t > times[m - 1])
        {
            return Err(py_error(Error::Invalid(
                "finite query times inside observation domain required".into(),
            )));
        }
        let matrices = copy_covariances(py, &data, m, p)?;
        let observations =
            TraceRequest::try_conditional(times, matrices, radii, lipschitz).map_err(py_error)?;
        let mut witnesses = memory::capacity(usize::from(witness.is_some())).map_err(py_error)?;
        if let Some(nodes) = &witness {
            witnesses.push(families::AffineWitness {
                nodes: copy_covariances(py, nodes, m, p)?,
                widening: 0.,
            });
        }
        let mut configurations = memory::capacity(1).map_err(py_error)?;
        configurations.push(families::Configuration {
            id: memory::text(if common_frame_weights.is_some() {
                "common-frame"
            } else {
                "continuous"
            })
            .map_err(py_error)?,
            lipschitz,
            class,
            witnesses,
        });
        Ok(families::HistoryRequest {
            schema: memory::text(if common_frame_weights.is_some() {
                families::SCHEMA
            } else {
                families::SCHEMA_V2
            })
            .map_err(py_error)?,
            observations,
            configurations,
            query_times,
        })
    })?;
    with_interrupt(py, operation, move |cancel| {
        let report = families::analyze(&request, &cancel)?;
        let encoded = if let Some(tolerance) = stability_tolerance {
            eigentrail_check::stability::encode_checked(&request, &report, tolerance, &cancel)?
        } else {
            eigentrail_check::families::check(&request, &report, &cancel)?;
            serialization::encode_with_cancel(&report, &cancel)?
        };
        String::from_utf8(encoded).map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn analyze_stability_json(py: Python<'_>, request: &str, tolerance: f64) -> PyResult<String> {
    stability::validate_tolerance(tolerance).map_err(py_error)?;
    let limit = preflight_json_inputs(py, request, &[], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    with_interrupt(py, operation, move |cancel| {
        let request: families::HistoryRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = families::analyze(&request, &cancel)?;
        String::from_utf8(eigentrail_check::stability::encode_checked(
            &request, &report, tolerance, &cancel,
        )?)
        .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn analyze_histories_json(py: Python<'_>, request: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    with_interrupt(py, operation, move |cancel| {
        let request: families::HistoryRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = families::analyze(&request, &cancel)?;
        eigentrail_check::families::check(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_histories_json(py: Python<'_>, request: &str, report: &str) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[report], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let report_text = copy_json_text(py, &operation, report)?;
    with_interrupt(py, operation, move |cancel| {
        let request: families::HistoryRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report: families::HistoryReport =
            serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::families::check(&request, &report, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn analyze_lineage_summary_json(py: Python<'_>, request: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[], RequestKind::LineageSummary)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    with_interrupt(py, operation, move |cancel| {
        let request: lineage_summary::Request =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = lineage_summary::analyze(&request, &cancel)?;
        eigentrail_check::lineage_summary::check(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_lineage_summary_json(py: Python<'_>, request: &str, report: &str) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[report], RequestKind::LineageSummary)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let report_text = copy_json_text(py, &operation, report)?;
    with_interrupt(py, operation, move |cancel| {
        let request: lineage_summary::Request =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report: lineage_summary::Report =
            serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::lineage_summary::check(&request, &report, &cancel)?;
        Ok(true)
    })
}
#[pyfunction]
fn analyze_common_frame_json(py: Python<'_>, request: &str, model: &str) -> PyResult<String> {
    let limit = preflight_json_inputs(py, request, &[model], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let model_text = copy_json_text(py, &operation, model)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: common_frame::CommonFrameModel =
            serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report = common_frame::analyze(&req, &model, &cancel)?;
        eigentrail_check::common_frame::check(&req, &model, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| Error::Invalid("UTF-8".into()))
    })
}
#[pyfunction]
fn check_common_frame_json(
    py: Python<'_>,
    request: &str,
    model: &str,
    report: &str,
) -> PyResult<bool> {
    let limit = preflight_json_inputs(py, request, &[model, report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(py_error)?;
    let request_text = copy_json_text(py, &operation, request)?;
    let model_text = copy_json_text(py, &operation, model)?;
    let report_text = copy_json_text(py, &operation, report)?;
    with_interrupt(py, operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: common_frame::CommonFrameModel =
            serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report: common_frame::CommonFrameReport =
            serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::common_frame::check(&req, &model, &report, &cancel)?;
        Ok(true)
    })
}
#[pymodule]
fn _eigentrail(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(window_envelope_json, m)?)?;
    m.add_function(wrap_pyfunction!(temporal_gaussian_json, m)?)?;
    m.add_function(wrap_pyfunction!(spd_tube_geometry_json, m)?)?;
    m.add_function(wrap_pyfunction!(track_stability_covariances, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_stability_json, m)?)?;
    m.add_function(wrap_pyfunction!(track_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_json, m)?)?;
    m.add_function(wrap_pyfunction!(track_covariances, m)?)?;
    m.add_function(wrap_pyfunction!(track_history_covariances, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_jumps_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_jumps_json, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_transport_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_transport_json, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_crossings_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_crossings_json, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_histories_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_histories_json, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_lineage_summary_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_lineage_summary_json, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_common_frame_json, m)?)?;
    m.add_function(wrap_pyfunction!(check_common_frame_json, m)?)?;
    Ok(())
}
