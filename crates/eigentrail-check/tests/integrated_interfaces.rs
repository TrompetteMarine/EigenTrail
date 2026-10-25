//! Independent normalization/risk and fixed-coordinate interface expectations.
use eigentrail_core::{resources::Cancellation, *};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/integrated_interfaces.json"
    ))
    .unwrap()
}
fn fraction(value: &serde_json::Value) -> f64 {
    let text = value.as_str().unwrap();
    text.split_once('/').map_or_else(
        || text.parse().unwrap(),
        |(a, b)| a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap(),
    )
}
fn trace(value: &serde_json::Value) -> TraceRequest {
    serialization::decode(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
fn independent_normalization_correction_and_auxiliary_risk_cases() {
    let data = oracle();
    let mut reports = Vec::new();
    for case in data["calibration_cases"].as_array().unwrap() {
        let request = trace(&case["request"]);
        let expected = &case["expectation"];
        let report = track(&request).unwrap();
        eigentrail_check::check(&request, &report).unwrap();
        let normalized = calibration::normalized(&request).unwrap();
        for matrix in &normalized {
            for (actual, value) in matrix
                .data
                .iter()
                .zip(expected["computed_normalized_entries"].as_array().unwrap())
            {
                assert_eq!(*actual, fraction(value));
            }
        }
        let z = calibration::normalized_error(&request, 0).unwrap();
        assert_eq!(z > 0., expected["normalization_nonzero"].as_bool().unwrap());
        let zl = fraction(&expected["normalization_actual_error"][0]);
        let zu = fraction(&expected["allowed_normalization_enclosure_upper"]);
        assert!(z + 1e-15 >= zl && z <= zu + 1e-15);
        let lo = fraction(&expected["multiplier"][0]);
        let hi = fraction(&expected["multiplier"][1]);
        let c = fraction(&expected["correction"]);
        for (j, e) in report.eigensystems.iter().enumerate() {
            let norm = e.values.iter().fold(0f64, |a, b| a.max(b.abs()));
            let lower = lo * (norm + e.backward + zl + c) + zl + c;
            let upper = hi * (norm + e.backward + zu + c) + zu + c;
            let error = 8. * f64::EPSILON * lower.abs().max(1.);
            assert!(report.radii[j] >= lower - error);
            assert!(report.radii[j] <= upper + fraction(&expected["rounding_allowance"]));
        }
        reports.push(report);
    }
    assert_eq!(reports[0].radii, reports[1].radii);
    for j in 0..3 {
        assert!(reports[2].radii[j] > reports[0].radii[j]);
        assert!(reports[3].radii[j] > reports[0].radii[j]);
        assert!(reports[4].radii[j] > reports[2].radii[j]);
    }
    for case in data["invalid_cases"].as_array().unwrap() {
        if let Ok(request) =
            serialization::decode::<TraceRequest>(&serde_json::to_vec(&case["request"]).unwrap())
        {
            assert!(track(&request).is_err(), "{}", case["id"]);
        }
    }
    let request = trace(&data["conditional_fallback"]);
    let report = track(&request).unwrap();
    eigentrail_check::check(&request, &report).unwrap();
    assert!(report.calibration_scope.starts_with("conditional:"));
}

#[test]
fn fixed_measurement_coordinates_preserve_only_orthogonal_pca_geometry() {
    let data = oracle();
    let cancel = Cancellation::default();
    for case in data["coordinate_cases"].as_array().unwrap() {
        let request: families::HistoryRequest =
            serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap();
        let report = families::analyze(&request, &cancel).unwrap();
        eigentrail_check::families::check(&request, &report, &cancel).unwrap();
        let expected = &case["expectation"];
        let cfg = &report.configurations[0];
        assert_eq!(cfg.continuous_workflow[0].queries.len(), 5);
        for (index, query) in cfg.continuous_workflow[0].queries.iter().enumerate() {
            assert_eq!(query.time, fraction(&expected["query_times"][index]));
            for i in 0..2 {
                let e = &query.centre.eigensystem;
                assert!((e.values[i] - fraction(&expected["strengths"][i])).abs() < 1e-8);
            }
        }
        let e = &cfg.continuous_conditional.eigensystems[0];
        for column in 0..2 {
            for row in 0..2 {
                let actual = e.vectors.at(row, 0) * e.vectors.at(column, 0);
                assert!(
                    (actual - fraction(&expected["leading_projector"][row + 2 * column])).abs()
                        < 1e-8
                );
            }
        }
    }
}

#[test]
fn fixed_coordinate_crossing_and_jump_structures_are_retained() {
    let data = oracle();
    let cancel = Cancellation::default();
    for case in data["structural_coordinate_cases"].as_array().unwrap() {
        if case["kind"] == "Crossings" {
            let request = trace(&case["request"]);
            let window: crossings::CrossingWindow =
                serialization::decode(&serde_json::to_vec(&case["window"]).unwrap()).unwrap();
            let report = crossings::analyze(&request, &window, &cancel).unwrap();
            eigentrail_check::crossings::check(&request, &window, &report, &cancel).unwrap();
            assert_eq!(report.outcome, crossings::CrossingOutcome::Resolved);
            let proof = &report.proof[0];
            assert_eq!(proof.left, vec![0, 1]);
            assert_eq!(proof.right, vec![1, 0]);
            assert!(
                proof.budget[0].branch_error < fraction(&case["expectation"]["branch_error_upper"])
            );
            assert!(
                proof.timing.last().unwrap().lower <= 0.
                    && proof.timing.last().unwrap().upper >= 0.
            );
        } else {
            let request: families::HistoryRequest =
                serialization::decode(&serde_json::to_vec(&case["request"]).unwrap()).unwrap();
            let report = families::analyze(&request, &cancel).unwrap();
            eigentrail_check::families::check(&request, &report, &cancel).unwrap();
            let cfg = &report.configurations[0];
            assert_eq!(cfg.jumps[0].family.mandatory, vec![true, false]);
            assert_eq!(cfg.jumps[0].family.optional, vec![false, false]);
            for e in &cfg.continuous_conditional.eigensystems {
                assert!(e.vectors.at(0, 0).abs() < 1e-10);
                assert!((e.vectors.at(1, 0).abs() - 1.).abs() < 1e-10);
            }
        }
    }
}
