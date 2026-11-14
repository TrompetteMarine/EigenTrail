//! Thin R boundary.
use eigentrail_core::{
    self as core,
    resources::{
        self, Cancellation,
        memory::{self, HeapBytes, Managed, Operation},
        worker::Worker,
    },
    serialization::input::{self, RequestKind},
    *,
};
use extendr_api::prelude::*;
unsafe extern "C" {
    fn eigentrail_check_interrupt() -> i32;
}
fn fail(e: core::Error) -> extendr_api::Error {
    extendr_api::Error::Other(e.to_string())
}
fn interrupt() -> extendr_api::Result<()> {
    if unsafe { eigentrail_check_interrupt() } != 0 {
        Ok(())
    } else {
        Err(fail(core::Error::Cancelled))
    }
}
fn with_interrupt<T: Send + HeapBytes>(
    operation: Operation,
    work: impl FnOnce(Cancellation) -> core::Result<T> + Send,
) -> extendr_api::Result<T> {
    std::thread::scope(|scope| {
        let worker = Worker::spawn(scope, &operation, work).map_err(fail)?;
        loop {
            match worker.recv_timeout(std::time::Duration::from_millis(25)) {
                Ok(result) => return result.map(Managed::into_inner).map_err(fail),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(extendr_api::Error::Other("worker incomplete".into()));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(e) = interrupt() {
                        worker.cancel();
                        return Err(e);
                    }
                }
            }
        }
    })
}

fn preflight_json_inputs(
    request: &str,
    companions: &[&str],
    kind: RequestKind,
) -> extendr_api::Result<usize> {
    let mut signal_error = None;
    let result = input::preflight_with_poll(request, companions, kind, || {
        interrupt().map_err(|error| {
            signal_error = Some(error);
            core::Error::Cancelled
        })
    });
    match signal_error {
        Some(error) => Err(error),
        None => result.map_err(fail),
    }
}

fn copy_json_text(operation: &Operation, text: &str) -> extendr_api::Result<Managed<String>> {
    prepare(operation, || {
        interrupt()?;
        let mut bytes = memory::capacity(text.len()).map_err(fail)?;
        for chunk in text.as_bytes().chunks(4096) {
            interrupt()?;
            bytes.extend_from_slice(chunk);
        }
        Ok(String::from_utf8(bytes).expect("copied input UTF-8"))
    })
}

fn prepare<T: HeapBytes>(
    operation: &Operation,
    work: impl FnOnce() -> extendr_api::Result<T>,
) -> extendr_api::Result<Managed<T>> {
    let mut language_error = None;
    let prepared = operation.own(|| match work() {
        Ok(value) => Ok(value),
        Err(error) => {
            language_error = Some(error);
            Err(core::Error::Cancelled)
        }
    });
    match language_error {
        Some(error) => Err(error),
        None => prepared.map_err(fail),
    }
}

fn run(operation: Operation, request: Managed<TraceRequest>) -> extendr_api::Result<String> {
    with_interrupt(operation, move |cancel| {
        let report = track_with_cancel(&request, &cancel)?;
        eigentrail_check::check_with_cancel(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn track_json(request: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    with_interrupt(operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = track_with_cancel(&req, &cancel)?;
        eigentrail_check::check_with_cancel(&req, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_json(request: &str, report: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let report_text = copy_json_text(&operation, report)?;
    with_interrupt(operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let rep: TraceReport = serialization::decode_in(report_text.as_bytes(), &cancel)?;
        drop(report_text);
        eigentrail_check::check_with_cancel(&req, &rep, &cancel)?;
        Ok(true)
    })
}
#[extendr]
fn analyze_jumps_json(request: &str, model: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[model], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let model_text = copy_json_text(&operation, model)?;
    with_interrupt(operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: jumps::JumpModel = serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report = jumps::analyze_with_cancel(&req, &model, &cancel)?;
        eigentrail_check::jumps::check_with_cancel(&req, &model, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_jumps_json(request: &str, model: &str, report: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[model, report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let model_text = copy_json_text(&operation, model)?;
    let report_text = copy_json_text(&operation, report)?;
    with_interrupt(operation, move |cancel| {
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
#[extendr]
fn analyze_transport_json(request: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[], RequestKind::Transport)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    with_interrupt(operation, move |cancel| {
        let request: transport_workflow::TransportRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let assessment = transport_workflow::analyze(&request, &cancel)?;
        eigentrail_check::transport::check(&request, &assessment, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&assessment, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_transport_json(request: &str, assessment: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[assessment], RequestKind::Transport)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let assessment_text = copy_json_text(&operation, assessment)?;
    with_interrupt(operation, move |cancel| {
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
#[extendr]
fn analyze_crossings_json(request: &str, window: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[window], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let window_text = copy_json_text(&operation, window)?;
    with_interrupt(operation, move |cancel| {
        let request: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let window: crossings::CrossingWindow =
            serialization::decode_in(window_text.as_bytes(), &cancel)?;
        drop(window_text);
        let assessment = crossings::analyze(&request, &window, &cancel)?;
        eigentrail_check::crossings::check(&request, &window, &assessment, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&assessment, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_crossings_json(
    request: &str,
    window: &str,
    assessment: &str,
) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[window, assessment], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let window_text = copy_json_text(&operation, window)?;
    let assessment_text = copy_json_text(&operation, assessment)?;
    with_interrupt(operation, move |cancel| {
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
#[extendr]
fn track_covariances(
    data: Robj,
    times: Robj,
    radii: Robj,
    lipschitz: f64,
) -> extendr_api::Result<String> {
    let (p, m) = covariance_shape(&data)?;
    resources::preflight(p, m, resources::DEFAULT_BYTES).map_err(fail)?;
    let input = data.as_real_slice().ok_or_else(|| {
        extendr_api::Error::Other(
            "double covariance input required; integer coercion is not implicit".into(),
        )
    })?;
    let t = times
        .as_real_slice()
        .ok_or_else(|| extendr_api::Error::Other("double times required".into()))?;
    let r = radii
        .as_real_slice()
        .ok_or_else(|| extendr_api::Error::Other("double radii required".into()))?;
    if t.len() != m || r.len() != m || input.len() != m * p * p {
        return Err(extendr_api::Error::Other(
            "array/time/radius lengths".into(),
        ));
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(fail)?;
    let request = prepare(&operation, || {
        let matrices = copy_covariances(input, p, m)?;
        TraceRequest::try_conditional(copy_values(t)?, matrices, copy_values(r)?, lipschitz)
            .map_err(fail)
    })?;
    run(operation, request)
}

fn covariance_shape(data: &Robj) -> extendr_api::Result<(usize, usize)> {
    let dims = data.get_attrib("dim").ok_or_else(|| {
        extendr_api::Error::Other("expected a double array (features,features,dates)".into())
    })?;
    let d = dims
        .as_integer_slice()
        .ok_or_else(|| extendr_api::Error::Other("integer dimensions required".into()))?;
    if d.len() != 3 || d[0] <= 0 || d[0] != d[1] || d[2] < 2 {
        return Err(extendr_api::Error::Other("array dimensions".into()));
    }
    let (p, m) = (d[0] as usize, d[2] as usize);
    Ok((p, m))
}

fn copy_values(input: &[f64]) -> extendr_api::Result<Vec<f64>> {
    let mut values = resources::zeros(input.len()).map_err(fail)?;
    values.copy_from_slice(input);
    Ok(values)
}

// All target and sampling premises remain explicit caller declarations.
#[allow(clippy::too_many_arguments)]
#[extendr]
fn window_envelope_json(
    sample_times: Robj,
    weights: Robj,
    target_time: f64,
    window_radius: f64,
    lipschitz: f64,
    target_mismatch_bound: f64,
    target_description: &str,
    assumptions: &str,
) -> extendr_api::Result<String> {
    let sample_times = sample_times.as_real_slice().ok_or_else(|| {
        fail(core::Error::Invalid(
            "double window sample times required".into(),
        ))
    })?;
    let weights = weights.as_real_slice().ok_or_else(|| {
        fail(core::Error::Invalid(
            "double window weights required".into(),
        ))
    })?;
    if !(1..=window::MAX_POINTS).contains(&sample_times.len())
        || sample_times.len() != weights.len()
    {
        return Err(fail(core::Error::Invalid(
            "equal window vectors with 1..=10000 entries required".into(),
        )));
    }
    for text in [target_description, assumptions] {
        if text.trim().is_empty() || text.len() > window::MAX_TEXT_BYTES {
            return Err(fail(core::Error::Invalid(
                "nonempty window target and assumptions of at most 4096 UTF-8 bytes required"
                    .into(),
            )));
        }
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(fail)?;
    let request = prepare(&operation, || {
        interrupt()?;
        let request = window::WindowRequest {
            sample_times: copy_values(sample_times)?,
            weights: copy_values(weights)?,
            target_time,
            window_radius,
            lipschitz,
            target_mismatch_bound,
            target_description: memory::text(target_description).map_err(fail)?,
            assumptions: memory::text(assumptions).map_err(fail)?,
        };
        window::validate(&request).map_err(fail)?;
        Ok(request)
    })?;
    with_interrupt(operation, move |cancel| {
        String::from_utf8(eigentrail_check::window::encode_checked(&request, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}

// R doubles represent every u32 exactly; reject truncation and saturation.
fn positive_count(value: f64, name: &str) -> extendr_api::Result<u32> {
    if !value.is_finite() || value < 1. || value > f64::from(u32::MAX) || value.fract() != 0. {
        return Err(fail(core::Error::Invalid(format!(
            "positive exact integer {name} no greater than 4294967295 required"
        ))));
    }
    Ok(value as u32)
}

#[extendr]
fn temporal_gaussian_json(
    dimension: f64,
    observations: f64,
    windows: f64,
    covariance_bound: f64,
    correlation_bound: f64,
    alpha: f64,
    assumptions: &str,
) -> extendr_api::Result<String> {
    let dimension = positive_count(dimension, "dimension")?;
    let observations = positive_count(observations, "observation count")?;
    let windows = positive_count(windows, "window count")?;
    if assumptions.trim().is_empty() || assumptions.len() > temporal::MAX_TEXT_BYTES {
        return Err(fail(core::Error::Invalid(
            "nonempty temporal assumptions of at most 4096 UTF-8 bytes required".into(),
        )));
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(fail)?;
    let request = prepare(&operation, || {
        interrupt()?;
        let request = temporal::TemporalRequest {
            dimension,
            observations,
            windows,
            covariance_bound,
            correlation_bound,
            alpha,
            assumptions: memory::text(assumptions).map_err(fail)?,
        };
        temporal::validate(&request).map_err(fail)?;
        Ok(request)
    })?;
    with_interrupt(operation, move |cancel| {
        String::from_utf8(eigentrail_check::temporal::encode_checked(
            &request, &cancel,
        )?)
        .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}

#[extendr]
fn spd_tube_geometry_json(
    dimension: f64,
    centre_floor: f64,
    radius: f64,
) -> extendr_api::Result<String> {
    let dimension = positive_count(dimension, "dimension")?;
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(fail)?;
    let request = prepare(&operation, || {
        interrupt()?;
        let request = geometry::SpdTubeRequest {
            dimension,
            centre_floor,
            radius,
        };
        geometry::validate_spd(&request).map_err(fail)?;
        Ok(request)
    })?;
    with_interrupt(operation, move |cancel| {
        String::from_utf8(eigentrail_check::geometry::encode_checked_spd(
            &request, &cancel,
        )?)
        .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}

fn copy_covariances(input: &[f64], p: usize, m: usize) -> extendr_api::Result<Vec<Matrix>> {
    let mut matrices = memory::capacity(m).map_err(fail)?;
    for date in 0..m {
        interrupt()?;
        let mut values = resources::zeros(p * p).map_err(fail)?;
        values.copy_from_slice(&input[date * p * p..(date + 1) * p * p]);
        matrices.push(Matrix { n: p, data: values });
    }
    Ok(matrices)
}

#[extendr]
fn track_history_covariances(
    data: Robj,
    times: Robj,
    radii: Robj,
    lipschitz: f64,
    query_times: Robj,
    common_frame_weights: Robj,
    witness: Robj,
) -> extendr_api::Result<String> {
    track_history_arrays(
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

fn stability_tolerance(value: &Robj) -> extendr_api::Result<f64> {
    let values = value
        .as_real_slice()
        .filter(|values| values.len() == 1)
        .ok_or_else(|| {
            fail(core::Error::Invalid(
                "one double stability tolerance required".into(),
            ))
        })?;
    stability::validate_tolerance(values[0]).map_err(fail)?;
    Ok(values[0])
}

#[extendr]
fn track_stability_covariances(
    data: Robj,
    times: Robj,
    radii: Robj,
    lipschitz: f64,
    query_times: Robj,
    tolerance: Robj,
    witness: Robj,
) -> extendr_api::Result<String> {
    let tolerance = stability_tolerance(&tolerance)?;
    track_history_arrays(
        data,
        times,
        radii,
        lipschitz,
        query_times,
        Robj::from(()),
        witness,
        Some(tolerance),
    )
}

#[allow(clippy::too_many_arguments)] // Both registered routes share managed history construction.
fn track_history_arrays(
    data: Robj,
    times: Robj,
    radii: Robj,
    lipschitz: f64,
    query_times: Robj,
    common_frame_weights: Robj,
    witness: Robj,
    stability_tolerance: Option<f64>,
) -> extendr_api::Result<String> {
    let (p, m) = covariance_shape(&data)?;
    let input = data.as_real_slice().ok_or_else(|| {
        extendr_api::Error::Other(
            "double covariance input required; integer coercion is not implicit".into(),
        )
    })?;
    let t = times
        .as_real_slice()
        .ok_or_else(|| extendr_api::Error::Other("double times required".into()))?;
    let r = radii
        .as_real_slice()
        .ok_or_else(|| extendr_api::Error::Other("double radii required".into()))?;
    let queries = query_times
        .as_real_slice()
        .ok_or_else(|| extendr_api::Error::Other("double query times required".into()))?;
    let weights = if common_frame_weights.is_null() {
        None
    } else {
        Some(common_frame_weights.as_real_slice().ok_or_else(|| {
            extendr_api::Error::Other("double common-frame weights required".into())
        })?)
    };
    families::preflight_common_frame(p, m, queries.len(), resources::DEFAULT_BYTES)
        .map_err(fail)?;
    if t.len() != m || r.len() != m || input.len() != m * p * p {
        return Err(extendr_api::Error::Other(
            "array/time/radius lengths".into(),
        ));
    }
    let witness_nodes = if witness.is_null() {
        None
    } else {
        if !matches!(covariance_shape(&witness), Ok(shape) if shape == (p, m)) {
            return Err(fail(core::Error::Invalid(
                "witness shape must match the observation covariance array".into(),
            )));
        }
        let nodes = witness
            .as_real_slice()
            .ok_or_else(|| fail(core::Error::Invalid("double witness array required".into())))?;
        if nodes.len() != input.len() {
            return Err(fail(core::Error::Invalid("witness array length".into())));
        }
        Some(nodes)
    };
    scalar::nonnegative(lipschitz, "global covariance regularity").map_err(fail)?;
    if t.iter().any(|t| !t.is_finite())
        || t.windows(2).any(|pair| pair[0] >= pair[1])
        || r.iter().any(|r| !r.is_finite() || *r < 0.)
    {
        return Err(fail(core::Error::Invalid(
            "strictly increasing finite times and nonnegative finite radii required".into(),
        )));
    }
    scalar::upper(scalar::point(lipschitz) * (scalar::point(t[m - 1]) - scalar::point(t[0])))
        .map_err(fail)?;
    if queries
        .iter()
        .any(|tq| !tq.is_finite() || *tq < t[0] || *tq > t[m - 1])
    {
        return Err(fail(core::Error::Invalid(
            "finite query times inside observation domain required".into(),
        )));
    }
    if weights.is_some_and(|values| values.len() != m) {
        return Err(fail(core::Error::Invalid(
            "one common-frame weight per date required".into(),
        )));
    }
    let operation = Operation::new(resources::DEFAULT_BYTES).map_err(fail)?;
    let request = prepare(&operation, || {
        let class = if let Some(weights) = weights {
            let model = common_frame::CommonFrameModel {
                weights: copy_values(weights)?,
                common_frame_assumed: true,
            };
            common_frame::validate_model(&model, m).map_err(fail)?;
            families::PathClass::CommonFrame { model }
        } else {
            families::PathClass::Continuous
        };
        let matrices = copy_covariances(input, p, m)?;
        let observations =
            TraceRequest::try_conditional(copy_values(t)?, matrices, copy_values(r)?, lipschitz)
                .map_err(fail)?;
        let query_times = copy_values(queries)?;
        let mut witnesses = memory::capacity(usize::from(witness_nodes.is_some())).map_err(fail)?;
        if let Some(nodes) = witness_nodes {
            witnesses.push(families::AffineWitness {
                nodes: copy_covariances(nodes, p, m)?,
                widening: 0.,
            });
        }
        let mut configurations = memory::capacity(1).map_err(fail)?;
        configurations.push(families::Configuration {
            id: memory::text(if weights.is_some() {
                "common-frame"
            } else {
                "continuous"
            })
            .map_err(fail)?,
            lipschitz,
            class,
            witnesses,
        });
        Ok(families::HistoryRequest {
            schema: memory::text(if weights.is_some() {
                families::SCHEMA
            } else {
                families::SCHEMA_V2
            })
            .map_err(fail)?,
            observations,
            configurations,
            query_times,
        })
    })?;
    with_interrupt(operation, move |cancel| {
        let report = families::analyze(&request, &cancel)?;
        let encoded = if let Some(tolerance) = stability_tolerance {
            eigentrail_check::stability::encode_checked(&request, &report, tolerance, &cancel)?
        } else {
            eigentrail_check::families::check(&request, &report, &cancel)?;
            serialization::encode_with_cancel(&report, &cancel)?
        };
        String::from_utf8(encoded).map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn analyze_stability_json(request: &str, tolerance: Robj) -> extendr_api::Result<String> {
    let tolerance = stability_tolerance(&tolerance)?;
    let limit = preflight_json_inputs(request, &[], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    with_interrupt(operation, move |cancel| {
        let request: families::HistoryRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = families::analyze(&request, &cancel)?;
        String::from_utf8(eigentrail_check::stability::encode_checked(
            &request, &report, tolerance, &cancel,
        )?)
        .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn analyze_histories_json(request: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    with_interrupt(operation, move |cancel| {
        let request: families::HistoryRequest =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = families::analyze(&request, &cancel)?;
        eigentrail_check::families::check(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_histories_json(request: &str, report: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[report], RequestKind::History)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let report_text = copy_json_text(&operation, report)?;
    with_interrupt(operation, move |cancel| {
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
#[extendr]
fn analyze_lineage_summary_json(request: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[], RequestKind::LineageSummary)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    with_interrupt(operation, move |cancel| {
        let request: lineage_summary::Request =
            serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let report = lineage_summary::analyze(&request, &cancel)?;
        eigentrail_check::lineage_summary::check(&request, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_lineage_summary_json(request: &str, report: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[report], RequestKind::LineageSummary)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let report_text = copy_json_text(&operation, report)?;
    with_interrupt(operation, move |cancel| {
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
#[extendr]
fn analyze_common_frame_json(request: &str, model: &str) -> extendr_api::Result<String> {
    let limit = preflight_json_inputs(request, &[model], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let model_text = copy_json_text(&operation, model)?;
    with_interrupt(operation, move |cancel| {
        let req: TraceRequest = serialization::decode_in(request_text.as_bytes(), &cancel)?;
        drop(request_text);
        let model: common_frame::CommonFrameModel =
            serialization::decode_in(model_text.as_bytes(), &cancel)?;
        drop(model_text);
        let report = common_frame::analyze(&req, &model, &cancel)?;
        eigentrail_check::common_frame::check(&req, &model, &report, &cancel)?;
        String::from_utf8(serialization::encode_with_cancel(&report, &cancel)?)
            .map_err(|_| core::Error::Invalid("UTF-8".into()))
    })
}
#[extendr]
fn check_common_frame_json(request: &str, model: &str, report: &str) -> extendr_api::Result<bool> {
    let limit = preflight_json_inputs(request, &[model, report], RequestKind::Trace)?;
    let operation = Operation::new(limit).map_err(fail)?;
    let request_text = copy_json_text(&operation, request)?;
    let model_text = copy_json_text(&operation, model)?;
    let report_text = copy_json_text(&operation, report)?;
    with_interrupt(operation, move |cancel| {
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
extendr_module! {mod eigentrail;fn window_envelope_json;fn temporal_gaussian_json;fn spd_tube_geometry_json;fn track_stability_covariances;fn analyze_stability_json;fn track_json;fn check_json;fn track_covariances;fn track_history_covariances;fn analyze_jumps_json;fn check_jumps_json;fn analyze_crossings_json;fn check_crossings_json;fn analyze_transport_json;fn check_transport_json;fn analyze_histories_json;fn check_histories_json;fn analyze_common_frame_json;fn check_common_frame_json;fn analyze_lineage_summary_json;fn check_lineage_summary_json;}
