//! Frozen independent event, strict-boundary and outer-binomial campaign contracts.
use eigentrail_check::campaign_oracle::{self, CampaignDecision, CoverageOutcome};
use eigentrail_core::{resources::Cancellation, scalar::*, *};

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/calibration_campaign_v2.json"
    ))
    .unwrap()
}
fn fraction(value: &serde_json::Value) -> f64 {
    let value = value.as_str().unwrap();
    value.split_once('/').map_or_else(
        || value.parse().unwrap(),
        |(a, b)| a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap(),
    )
}
fn outcome(name: &str) -> CoverageOutcome {
    match name {
        "Covered" => CoverageOutcome::Covered,
        "Undercovered" => CoverageOutcome::Undercovered,
        "Undecided" => CoverageOutcome::Undecided,
        _ => panic!("unknown frozen outcome"),
    }
}

#[test]
fn true_operator_event_has_strict_undercoverage_and_closed_coverage_boundaries() {
    for row in fixture()["coverage_boundaries"].as_array().unwrap() {
        let bounds = Range::closed(
            fraction(&row["error_bounds"][0]),
            fraction(&row["error_bounds"][1]),
        );
        let radius = fraction(&row["radius"]);
        assert_eq!(
            campaign_oracle::classify(&bounds, radius).unwrap(),
            outcome(row["outcome"].as_str().unwrap()),
            "{}",
            row["id"]
        );
    }
    for bounds in [
        Range::closed(-1., 1.),
        Range::closed(2., 1.),
        Range::closed(f64::NAN, 1.),
        Range::closed(0., f64::INFINITY),
    ] {
        assert!(campaign_oracle::classify(&bounds, 1.).is_err());
    }
    for radius in [-1., f64::NAN, f64::INFINITY] {
        assert!(campaign_oracle::classify(&Range::closed(0., 1.), radius).is_err());
    }
}

#[test]
fn independent_analytic_errors_cover_scalar_pair_singular_and_high_dimensional_cases() {
    let cancel = Cancellation::default();
    for row in fixture()["matrix_cases"].as_array().unwrap() {
        let n = row["dimension"].as_u64().unwrap() as usize;
        let mut sample = Matrix {
            n,
            data: vec![0.; n * n],
        };
        let mut population = vec![point(0.); n * n];
        if let Some(entries) = row["sample_entries"].as_array() {
            for (entry, value) in sample.data.iter_mut().zip(entries) {
                *entry = fraction(value);
            }
            for (entry, value) in population
                .iter_mut()
                .zip(row["population_entries"].as_array().unwrap())
            {
                *entry = point(fraction(value));
            }
        } else {
            for i in 0..n {
                sample.data[i + i * n] = fraction(&row["sample_diagonal"]);
                population[i + i * n] = point(fraction(&row["population_diagonal"]));
            }
        }
        let bounds = campaign_oracle::error_bounds(&sample, &population, &cancel).unwrap();
        let exact = fraction(&row["true_operator_error"]);
        assert!(bounds.lower_closed && bounds.upper_closed && bounds.lower >= 0.);
        assert!(
            bounds.lower <= exact && exact <= bounds.upper,
            "{}: {:?}",
            row["id"],
            bounds
        );
        let actual = campaign_oracle::classify(&bounds, fraction(&row["radius"])).unwrap();
        let requirement = row["outcome_requirement"].as_str().unwrap();
        if requirement == "NeverUndercovered" {
            assert_ne!(
                actual,
                CoverageOutcome::Undercovered,
                "Frobenius is only an operator upper bound: {}",
                row["id"]
            );
        } else {
            assert_eq!(actual, outcome(requirement), "{}", row["id"]);
        }
    }
    let matrix = Matrix {
        n: 1,
        data: vec![1.5],
    };
    let bounds =
        campaign_oracle::error_bounds(&matrix, &[interval(1., 1.25).unwrap()], &cancel).unwrap();
    assert!(bounds.lower <= 0.25 && bounds.upper >= 0.5);
    assert_eq!(
        campaign_oracle::classify(&bounds, 0.375).unwrap(),
        CoverageOutcome::Undecided
    );
    assert!(
        campaign_oracle::error_bounds(
            &Matrix {
                n: 2,
                data: vec![1.; 3]
            },
            &[point(0.); 4],
            &cancel
        )
        .is_err()
    );
    assert!(campaign_oracle::error_bounds(&matrix, &[], &cancel).is_err());
    cancel.cancel();
    assert!(matches!(
        campaign_oracle::error_bounds(&matrix, &[point(1.)], &cancel),
        Err(Error::Cancelled)
    ));
}

#[test]
fn exactly_two_thousand_independent_outer_records_are_required_for_binomial_decision() {
    let data = fixture();
    assert_eq!(data["binomial"]["reject_at_failures"], 131);
    assert_eq!(
        data["binomial"]["exact_tail_checks"][0]["within_scenario_risk"],
        false
    );
    assert_eq!(
        data["binomial"]["exact_tail_checks"][1]["within_scenario_risk"],
        true
    );
    for row in data["count_cases"].as_array().unwrap() {
        let number = |field: &str| row[field].as_u64().unwrap() as usize;
        let actual = campaign_oracle::summarize_counts(
            number("covered"),
            number("undercovered"),
            number("undecided"),
            number("unfinished"),
            number("expected"),
        )
        .unwrap();
        let expected = match row["decision"].as_str().unwrap() {
            "NotRejected" => CampaignDecision::NotRejected,
            "Rejected" => CampaignDecision::Rejected,
            "Incomplete" => CampaignDecision::Incomplete,
            _ => panic!("unknown decision"),
        };
        assert_eq!(actual, expected, "{}", row["id"]);
    }
    assert!(campaign_oracle::summarize_counts(2000, 1, 0, 0, 2000).is_err());
    assert!(campaign_oracle::summarize_counts(usize::MAX, 1, 0, 0, 2000).is_err());
    assert!(campaign_oracle::summarize_counts(0, 0, 0, 0, 0).is_err());
    assert!(campaign_oracle::summarize_counts(1999, 0, 0, 0, 1999).is_err());
}
