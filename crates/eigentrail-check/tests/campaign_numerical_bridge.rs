//! Frozen normal-only arithmetic and analytically justified proposal bridge contracts.
use eigentrail_core::{resources::Cancellation, scalar::normal_product_sum, *};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/campaign_numerical_bridge.json"
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
fn binary(value: &serde_json::Value) -> f64 {
    f64::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}

#[test]
fn normal_only_product_sum_distinguishes_exact_zero_from_underflow_and_overflow() {
    for case in oracle()["primitive_cases"].as_array().unwrap() {
        let terms: Vec<_> = case["terms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (binary(&row[0]), binary(&row[1]), binary(&row[2])))
            .collect();
        let actual = normal_product_sum(terms);
        if case["eligible"] == true {
            assert_eq!(actual, Some(binary(&case["output_bits"])), "{}", case["id"]);
        } else {
            assert!(actual.is_none(), "{} must fall back", case["id"]);
        }
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(normal_product_sum([(value, 1., 1.)]).is_none());
        assert!(normal_product_sum([(0., value, 1.)]).is_none());
    }
}

fn analytic(case: &serde_json::Value) -> (Matrix, Vec<f64>, Matrix) {
    let n = case["n"].as_u64().unwrap() as usize;
    let mut values = vec![1.; n];
    values[0] = 2.;
    let mut vectors = Matrix {
        n,
        data: vec![0.; n * n],
    };
    let mut matrix = Matrix {
        n,
        data: vec![0.; n * n],
    };
    for i in 0..n {
        vectors.data[i + i * n] = 1.;
        matrix.data[i + i * n] = values[i];
    }
    match case["recipe"].as_str().unwrap() {
        "identity" => {}
        "hadamard16" => {
            for row in 0..16 {
                for col in 0..16 {
                    vectors.data[row + col * n] = if ((row & col) as u32).count_ones() % 2 == 0 {
                        0.25
                    } else {
                        -0.25
                    };
                    matrix.data[row + col * n] = f64::from(row == col) + 1. / 16.;
                }
            }
        }
        "scaled_first" => {
            let delta = fraction(&case["scale_delta"]);
            vectors.data[0] = 1. + delta;
            matrix.data[0] = 2. * (1. + delta) * (1. + delta);
        }
        "subnormal_diagonal" => {
            let value = binary(&case["value_bits"]);
            values.fill(value);
            for i in 0..n {
                matrix.data[i + i * n] = value;
            }
        }
        _ => panic!("unknown independent recipe"),
    }
    (matrix, values, vectors)
}

#[test]
fn fast_bridge_preserves_exact_polar_inequalities_and_original_subnormal_fallback() {
    let cancel = Cancellation::default();
    for case in oracle()["bridge_cases"].as_array().unwrap() {
        let (matrix, values, vectors) = analytic(case);
        let (backward, polar) =
            numerics::verify_eigensystem(&matrix, &values, &vectors, &cancel).unwrap();
        let proposal = Eigensystem {
            values,
            vectors,
            backward,
            polar,
        };
        eigentrail_check::eigensystem::inspect(&matrix, &proposal, &cancel).unwrap();
        assert!(
            backward >= fraction(&case["true_orthogonal_reconstruction_error"]),
            "{}",
            case["id"]
        );
        assert!(
            polar >= fraction(&case["true_polar_error"]),
            "{}",
            case["id"]
        );
        assert!(
            backward <= fraction(&case["backward_upper"]),
            "{}: {}",
            case["id"],
            backward
        );
        assert!(
            polar <= fraction(&case["polar_upper"]),
            "{}: {}",
            case["id"],
            polar
        );
        if case["positive_guards"] == true {
            assert!(
                backward > 0. && polar > 0.,
                "eligible n>16 route must carry the frozen gamma allowance"
            );
        } else {
            assert_eq!(
                backward, 0.,
                "subnormal spectrum uses the original exact interval calculation"
            );
            assert_eq!(polar, 0.);
        }
        if case["recipe"] == "scaled_first" {
            let mut understated = proposal.clone();
            understated.backward = fraction(&case["true_orthogonal_reconstruction_error"]) / 2.;
            assert!(
                eigentrail_check::eigensystem::inspect(&matrix, &understated, &cancel).is_err()
            );
            understated = proposal.clone();
            understated.polar = 0.;
            assert!(
                eigentrail_check::eigensystem::inspect(&matrix, &understated, &cancel).is_err()
            );
        }
    }
    // This exact dyadic Hadamard example retains the original small-dimension route.
    let small = serde_json::json!({"n":16,"recipe":"hadamard16"});
    let (matrix, values, vectors) = analytic(&small);
    assert_eq!(
        numerics::verify_eigensystem(&matrix, &values, &vectors, &cancel).unwrap(),
        (0., 0.)
    );
    cancel.cancel();
    assert!(matches!(
        numerics::verify_eigensystem(&matrix, &values, &vectors, &cancel),
        Err(Error::Cancelled)
    ));
}

#[test]
fn whole_space_association_remains_complete_after_exact_identity_shortcut() {
    let cancel = Cancellation::default();
    let case = &oracle()["bridge_cases"][3];
    let (matrix, values, vectors) = analytic(case);
    let (backward, polar) =
        numerics::verify_eigensystem(&matrix, &values, &vectors, &cancel).unwrap();
    let proposal = Eigensystem {
        values,
        vectors,
        backward,
        polar,
    };
    eigentrail_check::eigensystem::inspect(&matrix, &proposal, &cancel).unwrap();
    let tubes = [Tube {
        start: 0,
        end: matrix.n,
        eta: 1.,
        gap: 0.,
        bound: 0.,
        informative: true,
    }];
    let producer = association::graph(&proposal, &proposal, &tubes, 0., 1., &cancel).unwrap();
    let checked =
        eigentrail_check::association::reconstruct(&proposal, &proposal, &tubes, 0., 1., &cancel)
            .unwrap();
    assert_eq!(producer, vec![vec![0]]);
    assert_eq!(checked, vec![vec![0]]);
    assert_eq!(
        association::classify_checked(&producer).unwrap(),
        Association::Unique(vec![0])
    );
    assert_eq!(
        eigentrail_check::association::classify_checked(&checked, &cancel).unwrap(),
        Association::Unique(vec![0])
    );
}
