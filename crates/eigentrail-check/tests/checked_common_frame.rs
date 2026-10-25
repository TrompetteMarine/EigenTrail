use eigentrail_core::{common_frame::*, resources::Cancellation, *};
fn request() -> TraceRequest {
    TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 2,
                data: vec![0.8, 0., 0., 0.2],
            },
            Matrix {
                n: 2,
                data: vec![0.2, 0., 0., 0.8],
            },
        ],
        vec![1e-10; 2],
        0.6_f64.next_up(),
    )
}
fn model() -> CommonFrameModel {
    CommonFrameModel {
        weights: vec![1., 0.],
        common_frame_assumed: true,
    }
}
fn checked() -> (TraceRequest, CommonFrameModel, CommonFrameReport) {
    let request = request();
    let model = model();
    let cancel = Cancellation::default();
    let report = analyze(&request, &model, &cancel).unwrap();
    eigentrail_check::common_frame::check(&request, &model, &report, &cancel).unwrap();
    (request, model, report)
}
#[test]
fn independent_common_frame_rank_exchange_and_gauge() {
    let (request, model, report) = checked();
    let cancel = Cancellation::default();
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/oracles/common_frame.json")).unwrap();
    assert_eq!(
        serde_json::to_value(&report.frame.possible_ranks).unwrap(),
        oracle["ranks"]
    );
    for (date, row) in [[0.8, 0.2], [0.2, 0.8]].iter().enumerate() {
        for (i, &truth) in row.iter().enumerate() {
            let bounds = &report.frame.labelled_strengths[date][i];
            assert!(bounds.lower <= truth && bounds.upper >= truth);
        }
    }
    let mut rotated = request.clone();
    for matrix in &mut rotated.covariances {
        let a = matrix.data[0];
        let b = matrix.data[3];
        matrix.data = vec![
            a * 0.36 + b * 0.64,
            (a - b) * 0.48,
            (a - b) * 0.48,
            a * 0.64 + b * 0.36,
        ];
    }
    let rotated_report = analyze(&rotated, &model, &cancel).unwrap();
    eigentrail_check::common_frame::check(&rotated, &model, &rotated_report, &cancel).unwrap();
    assert_eq!(
        rotated_report.frame.possible_ranks,
        report.frame.possible_ranks
    );
    let bytes = serialization::encode(&report).unwrap();
    assert_eq!(
        serialization::decode::<CommonFrameReport>(&bytes).unwrap(),
        report
    );
    let mut gauge = report.clone();
    for v in &mut gauge.frame.axes.vectors.data[..2] {
        *v = -*v;
    }
    eigentrail_check::common_frame::check(&request, &model, &gauge, &cancel).unwrap();
}
#[test]
fn common_frame_rejects_tampered_axes_strengths_ranks_and_scope() {
    let (request, model, report) = checked();
    let cancel = Cancellation::default();
    let mut cases = Vec::new();
    let mut bad = report.clone();
    bad.frame.axis_error = 0.;
    cases.push(bad);
    let mut bad = report.clone();
    bad.frame.labelled_strengths[0][0].upper = 0.5;
    cases.push(bad);
    let mut bad = report.clone();
    bad.frame.possible_ranks[1][0] = [1, 1];
    cases.push(bad);
    let mut bad = report.clone();
    bad.eigensystems.pop();
    cases.push(bad);
    let mut bad = report.clone();
    bad.radii[0] = 0.;
    cases.push(bad);
    let mut bad = report.clone();
    bad.frame.axes.vectors.data.swap(0, 2);
    bad.frame.axes.vectors.data.swap(1, 3);
    cases.push(bad);
    let mut bad = report.clone();
    bad.frame.axes.vectors.data[0] = 1.1;
    cases.push(bad);
    let mut bad = report.clone();
    bad.calibration_scope = "finite-sample certified bootstrap".into();
    cases.push(bad);
    let mut bad = report.clone();
    bad.resource_qualification = "qualified".into();
    cases.push(bad);
    for bad in cases {
        assert!(eigentrail_check::common_frame::check(&request, &model, &bad, &cancel).is_err());
    }
    let changed = CommonFrameModel {
        weights: vec![0., 1.],
        ..model.clone()
    };
    assert!(eigentrail_check::common_frame::check(&request, &changed, &report, &cancel).is_err());
    let mut forged = report;
    forged.model_hash = serialization::digest(&changed).unwrap();
    assert!(eigentrail_check::common_frame::check(&request, &changed, &forged, &cancel).is_err());
}
#[test]
fn common_frame_structural_and_numeric_boundaries_remain_explicit() {
    let (request, model, report) = checked();
    let cancel = Cancellation::default();
    let absent = CommonFrameModel {
        common_frame_assumed: false,
        ..model.clone()
    };
    assert!(matches!(
        analyze(&request, &absent, &cancel),
        Err(Error::Exhausted(_))
    ));
    assert!(eigentrail_check::common_frame::check(&request, &absent, &report, &cancel).is_err());
    let tied = CommonFrameModel {
        weights: vec![0.5, 0.5],
        ..model.clone()
    };
    assert!(matches!(
        analyze(&request, &tied, &cancel),
        Err(Error::Exhausted(_))
    ));
    for weights in [vec![0., 0.], vec![1.], vec![-1., 2.], vec![f64::NAN, 0.]] {
        assert!(
            analyze(
                &request,
                &CommonFrameModel {
                    weights,
                    common_frame_assumed: true
                },
                &cancel
            )
            .is_err()
        );
    }
    let huge = CommonFrameModel {
        weights: vec![f64::MAX; 2],
        ..model.clone()
    };
    assert!(matches!(
        analyze(&request, &huge, &cancel),
        Err(Error::Exhausted(_))
    ));
    let extreme = TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![f64::MAX]
            };
            2
        ],
        vec![f64::MAX; 2],
        0.,
    );
    assert!(matches!(
        analyze(&extreme, &model, &cancel),
        Err(Error::Exhausted(_))
    ));
    let mut broad_request = request.clone();
    if let Calibration::Conditional { radii, .. } = &mut broad_request.calibration {
        *radii = vec![f64::MAX; 2];
    }
    let mut broad = report.clone();
    broad.request_hash = serialization::digest(&broad_request).unwrap();
    broad.radii = vec![f64::MAX; 2];
    broad.frame.axes.backward = f64::MAX;
    assert!(
        eigentrail_check::common_frame::check(&broad_request, &model, &broad, &cancel).is_err()
    );
    cancel.cancel();
    assert_eq!(analyze(&request, &model, &cancel), Err(Error::Cancelled));
}
