//! V2 sampler algorithm conformance from independent raw-word/rational oracles.
#[path = "../examples/support_v2/mod.rs"]
mod support_v2;

use eigentrail_core::Error;
use rand_chacha::{
    ChaCha12Rng,
    rand_core::{RngCore, SeedableRng},
};
use serde_json::Value;
use support_v2::{Sampler, logical_seed, open_uniform};

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/oracles/campaign_sampler_v2.json"
    ))
    .unwrap()
}
fn word(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().unwrap(), 16).unwrap()
}
fn fraction(value: &Value) -> f64 {
    let text = value.as_str().unwrap();
    text.split_once('/').map_or_else(
        || text.parse().unwrap(),
        |(a, b)| a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap(),
    )
}
fn within(actual: f64, expected: &Value, data: &Value) {
    assert!(actual.is_finite());
    let lo = fraction(&expected[0]);
    let hi = fraction(&expected[1]);
    let allowance = fraction(&data["numeric_absolute_allowance"])
        + fraction(&data["numeric_relative_allowance"]) * lo.abs().max(hi.abs());
    assert!(
        lo - allowance <= actual && actual <= hi + allowance,
        "{actual} outside independent interval[{lo},{hi}] +{allowance}"
    );
}
struct Scripted {
    words: Vec<u64>,
    used: usize,
}
impl Scripted {
    fn new(values: &Value) -> Self {
        Self {
            words: values.as_array().unwrap().iter().map(word).collect(),
            used: 0,
        }
    }
}
impl RngCore for Scripted {
    fn next_u64(&mut self) -> u64 {
        let word = *self
            .words
            .get(self.used)
            .expect("sampler consumed an unregistered extra word");
        self.used += 1;
        word
    }
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        for chunk in destination.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

#[test]
fn raw_uniforms_have_exact_open_endpoints_and_keep_the_registered_bins() {
    let data = oracle();
    for case in data["uniform"].as_array().unwrap() {
        let raw = word(&case["raw"]);
        let value = open_uniform(raw);
        assert!(0. < value && value < 1.);
        assert_eq!(value.to_bits(), word(&case["bits"]));
        let mut sampler = Sampler(Scripted {
            words: vec![raw],
            used: 0,
        });
        assert_eq!(sampler.uniform().to_bits(), value.to_bits());
        assert_eq!(sampler.0.used, 1);
    }
    assert_eq!(open_uniform(0), 2f64.powi(-53));
    assert_eq!(open_uniform(u64::MAX), 1. - 2f64.powi(-53));
    // Every upper52-bit bin has one exactly representable midpoint; lower12bits
    // are intentionally discarded rather than rounded into a closed endpoint.
    for k in [0, 1, (1u64 << 51) - 1, 1u64 << 51, (1u64 << 52) - 1] {
        assert_eq!(
            open_uniform(k << 12).to_bits(),
            open_uniform((k << 12) | 4095).to_bits()
        );
    }
}

#[test]
fn full_logical_identifiers_bind_master_seed_phase_date_and_chacha12_words() {
    let data = oracle();
    let mut seeds = Vec::new();
    for case in data["logical_streams"].as_array().unwrap() {
        let date = case["date"].as_u64().map(|v| v as usize);
        let seed = logical_seed(
            case["master_seed"].as_u64().unwrap(),
            case["scenario"].as_str().unwrap(),
            case["outer"].as_u64().unwrap() as usize,
            date,
            case["purpose"].as_str().unwrap(),
        )
        .unwrap();
        let expected = case["seed"].as_str().unwrap();
        assert_eq!(
            seed.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            expected
        );
        assert!(!seeds.contains(&seed));
        seeds.push(seed);
        let mut rng = ChaCha12Rng::from_seed(seed);
        for expected in case["first_words"].as_array().unwrap() {
            assert_eq!(rng.next_u64(), word(expected));
        }
    }
    for (scenario, outer, date, purpose) in [
        ("G7", 0, Some(0), "release-scatter"),
        ("G1", 2000, Some(0), "release-scatter"),
        ("G1", 0, Some(101), "release-scatter"),
        ("G1", 0, Some(0), "scatter"),
    ] {
        assert!(logical_seed(20261022, scenario, outer, date, purpose).is_err());
    }
    let release = logical_seed(20261022, "G1", 0, Some(0), "release-scatter").unwrap();
    let pilot = logical_seed(20261022, "G1", 0, Some(0), "pilot-scatter").unwrap();
    assert_ne!(release, pilot);
}

#[test]
fn box_muller_and_gamma_match_bounded_independent_values_and_consumption() {
    let data = oracle();
    for case in data["normal"].as_array().unwrap() {
        let mut sampler = Sampler(Scripted::new(&case["words"]));
        within(sampler.normal(), &case["value"], &data);
        assert_eq!(
            sampler.0.used,
            case["words_consumed"].as_u64().unwrap() as usize
        );
    }
    for case in data["gamma"].as_array().unwrap() {
        let mut sampler = Sampler(Scripted::new(&case["words"]));
        within(
            sampler.gamma(fraction(&case["shape"])).unwrap(),
            &case["value"],
            &data,
        );
        assert_eq!(
            sampler.0.used,
            case["words_consumed"].as_u64().unwrap() as usize
        );
    }
    let case = &data["negative_base_rejection"];
    let mut sampler = Sampler(Scripted::new(&case["words"]));
    within(
        sampler.gamma(fraction(&case["shape"])).unwrap(),
        &case["value"],
        &data,
    );
    assert_eq!(
        sampler.0.used,
        case["words_consumed"].as_u64().unwrap() as usize
    );
    let case = &data["acceptance_rejection"];
    let mut sampler = Sampler(Scripted::new(&case["words"]));
    within(
        sampler.gamma(fraction(&case["shape"])).unwrap(),
        &case["value"],
        &data,
    );
    assert_eq!(
        sampler.0.used,
        case["words_consumed"].as_u64().unwrap() as usize
    );
    let case = &data["iteration_exhaustion"];
    let repeated: Vec<u64> = case["repeating_words"]
        .as_array()
        .unwrap()
        .iter()
        .map(word)
        .collect();
    let mut sampler = Sampler(Scripted {
        words: repeated.repeat(case["attempt_limit"].as_u64().unwrap() as usize),
        used: 0,
    });
    assert!(matches!(
        sampler.gamma(fraction(&case["shape"])),
        Err(Error::Exhausted(_))
    ));
    assert_eq!(
        sampler.0.used,
        case["words_consumed"].as_u64().unwrap() as usize
    );
}

#[test]
fn bartlett_factor_order_normalization_and_unsupported_shape_gates_are_explicit() {
    let data = oracle();
    let case = &data["wishart"];
    let mut sampler = Sampler(Scripted::new(&case["words"]));
    let output = sampler
        .wishart(
            case["p"].as_u64().unwrap() as usize,
            case["nu"].as_u64().unwrap() as usize,
        )
        .unwrap();
    assert_eq!(output.len(), 4);
    for (actual, expected) in output.iter().zip(case["matrix"].as_array().unwrap()) {
        within(*actual, expected, &data);
    }
    assert_eq!(output[1].to_bits(), output[2].to_bits());
    assert!(output[0] * output[3] - output[1] * output[2] > 0.);
    assert_eq!(
        sampler.0.used,
        case["words_consumed"].as_u64().unwrap() as usize
    );
    for shape in [0., -1., 0.5, f64::NAN, f64::INFINITY] {
        let mut sampler = Sampler(Scripted {
            words: vec![],
            used: 0,
        });
        assert!(matches!(sampler.gamma(shape), Err(Error::Invalid(_))));
        assert_eq!(sampler.0.used, 0);
    }
    for (p, nu) in [(0, 3), (2, 1), (2, 2), (257, 258)] {
        let mut sampler = Sampler(Scripted {
            words: vec![],
            used: 0,
        });
        assert!(matches!(sampler.wishart(p, nu), Err(Error::Invalid(_))));
        assert_eq!(sampler.0.used, 0);
    }
}
