use eigentrail_core::{
    Error,
    scalar::{self, *},
};

fn encloses(actual: Interval, oracle: Interval) {
    assert!(
        actual.inf() <= oracle.inf() && actual.sup() >= oracle.sup(),
        "actual [{},{}], independent MPFR [{},{}]",
        actual.inf(),
        actual.sup(),
        oracle.inf(),
        oracle.sup()
    );
}

#[test]
fn bounded_log_contains_independent_mpfr_across_binary_domain() {
    let mut values = vec![
        f64::from_bits(1),
        f64::from_bits(2),
        f64::MIN_POSITIVE.next_down(),
        f64::MIN_POSITIVE,
        0.5,
        1.,
        1f64.next_up(),
        2.,
        4040.,
        f64::MAX,
    ];
    let mut bits = 0x6a09e667f3bcc909u64;
    for _ in 0..2048 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        let value = f64::from_bits(bits & 0x7fefffffffffffff);
        if value > 0. {
            values.push(value);
        }
    }
    for x in values {
        let result = scalar::ln(point(x)).unwrap();
        encloses(result, point(x).ln());
        assert!(
            result.sup() - result.inf() < 2e-12,
            "excessive ln width for {x}"
        );
    }
    let x = interval(0.25, 4040.).unwrap();
    encloses(scalar::ln(x).unwrap(), x.ln());
}

#[test]
fn bounded_angles_contain_independent_mpfr_at_endpoints_and_small_angles() {
    let mut values = vec![
        -1.,
        (-1f64).next_up(),
        -0.5,
        -f64::from_bits(1),
        -0.,
        0.,
        f64::from_bits(1),
        0.5,
        1f64.next_down(),
        1.,
    ];
    for i in 0..2049 {
        values.push(-1. + (i as f64) / 1024.);
    }
    for x in values {
        for (a, b) in [
            (scalar::asin(point(x)).unwrap(), point(x).asin()),
            (scalar::acos(point(x)).unwrap(), point(x).acos()),
        ] {
            encloses(a, b);
            assert!(a.sup() - a.inf() < 2e-14, "excessive angle width for {x}");
        }
    }
    let x = interval(-0.75, 0.875).unwrap();
    encloses(scalar::asin(x).unwrap(), x.asin());
    encloses(scalar::acos(x).unwrap(), x.acos());
}

#[test]
fn invalid_elementary_domains_refuse_without_nonfinite_construction() {
    for x in [Interval::EMPTY, point(0.), point(-1.), Interval::ENTIRE] {
        assert!(matches!(scalar::ln(x), Err(Error::Invalid(_))));
    }
    for x in [
        Interval::EMPTY,
        point(1f64.next_up()),
        point(-2.),
        Interval::ENTIRE,
    ] {
        assert!(matches!(scalar::asin(x), Err(Error::Invalid(_))));
        assert!(matches!(scalar::acos(x), Err(Error::Invalid(_))));
    }
    assert_eq!(scalar::ln(point(1.)).unwrap(), point(0.));
    assert_eq!(scalar::acos(point(1.)).unwrap(), point(0.));
}
