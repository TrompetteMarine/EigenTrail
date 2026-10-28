//! Additional admission regressions; the original frozen readout tests stay unchanged.
use eigentrail_core::{
    Error, Quantity, Range,
    lineage_summary::{self as readout, Motion, Request},
    resources::Cancellation,
    serialization,
};
use serde_json::Value;

fn constant_line() -> Request {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../fixtures/oracles/lineage_readout_oracles.json"
    ))
    .unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "constant_line3")
        .unwrap();
    Request {
        schema: "eigentrail.lineage-summary.v1".into(),
        history: serialization::decode(&serde_json::to_vec(&case["history"]).unwrap()).unwrap(),
        delta: 0.05,
    }
}

#[test]
fn empty_open_singletons_are_invalid_even_for_a_whole_space_target() {
    for (lower_closed, upper_closed) in [(false, false), (false, true), (true, false)] {
        let empty = Quantity::Interval(Range {
            lower: 0.,
            upper: 0.,
            lower_closed,
            upper_closed,
        });
        for (q, p) in [(1, 3), (3, 3)] {
            assert!(matches!(
                readout::classify_motion(&empty, q, p, 0.05),
                Err(Error::Invalid(_))
            ));
        }
    }
    let singleton = Quantity::Interval(Range::closed(0., 0.));
    assert_eq!(
        readout::classify_motion(&singleton, 1, 3, 0.05).unwrap(),
        Motion::WithinTolerance
    );
    assert_eq!(
        readout::classify_motion(&singleton, 3, 3, 0.05).unwrap(),
        Motion::Unresolved
    );
}

#[test]
fn direct_rust_requests_obey_configuration_and_query_count_limits() {
    let request = constant_line();
    let cancel = Cancellation::default();
    let report = readout::analyze(&request, &cancel).unwrap();
    eigentrail_check::lineage_summary::check(&request, &report, &cancel).unwrap();

    for count in [0, 17] {
        let mut invalid = request.clone();
        invalid.history.configurations = vec![request.history.configurations[0].clone(); count];
        for (index, configuration) in invalid.history.configurations.iter_mut().enumerate() {
            configuration.id = format!("configuration-{index}");
        }
        assert!(matches!(
            readout::validate_request(&invalid),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            readout::analyze(&invalid, &cancel),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            eigentrail_check::lineage_summary::check(&invalid, &report, &cancel),
            Err(Error::Invalid(_))
        ));
    }

    let mut too_many_queries = request;
    too_many_queries.history.query_times = vec![0.; 10_001];
    assert!(matches!(
        readout::validate_request(&too_many_queries),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        readout::analyze(&too_many_queries, &cancel),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        eigentrail_check::lineage_summary::check(&too_many_queries, &report, &cancel),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn checker_refuses_an_extra_directly_constructed_row() {
    let request = constant_line();
    let cancel = Cancellation::default();
    let mut report = readout::analyze(&request, &cancel).unwrap();
    eigentrail_check::lineage_summary::check(&request, &report, &cancel).unwrap();
    // This analytical fixture has three separated rank-one blocks at each of three queries.
    assert_eq!(report.rows.len(), 9);
    report.rows.push(report.rows[0].clone());
    assert!(matches!(
        eigentrail_check::lineage_summary::check(&request, &report, &cancel),
        Err(Error::Invalid(_))
    ));
}
