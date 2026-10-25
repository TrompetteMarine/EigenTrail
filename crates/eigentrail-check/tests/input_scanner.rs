//! Independent structural and interruption regressions for untrusted input inspection.
use eigentrail_core::{
    resources::{Cancellation, DEFAULT_BYTES},
    serialization::input::{RequestKind, inspect_request},
    *,
};
use std::{
    cell::Cell,
    io::{self, Read},
};

fn request() -> TraceRequest {
    TraceRequest::conditional(
        vec![0., 1.],
        vec![
            Matrix {
                n: 1,
                data: vec![1.]
            };
            2
        ],
        vec![0.; 2],
        0.,
    )
}

fn encoded(request: &TraceRequest) -> Vec<u8> {
    serialization::encode(request).unwrap()
}

#[test]
fn escaped_and_reserved_feature_strings_cannot_supply_a_budget() {
    for name in [
        "Greek α, emoji 🙂, control \u{0001}\n\t\r",
        "quoted \" axis \\ slash /",
        "f64:0000000000000000",
        "str:f64:0000000000000000",
        "\"memory_bytes\":1,{\"memory_bytes\":9999999999999999999999}",
    ] {
        let mut input = request();
        input.features[0] = name.into();
        let bytes = encoded(&input);
        let header = inspect_request(
            bytes.as_slice(),
            RequestKind::Trace,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(header.memory_bytes, DEFAULT_BYTES);
        assert_eq!(header.bytes, bytes.len());
        let mut history = families::HistoryRequest {
            schema: families::SCHEMA.into(),
            observations: input,
            configurations: vec![families::Configuration {
                id: "continuous".into(),
                lipschitz: 0.,
                class: families::PathClass::Continuous,
                witnesses: Vec::new(),
            }],
            query_times: vec![0.5],
        };
        history.configurations[0].id = name.into();
        let bytes = serialization::encode(&history).unwrap();
        let header = inspect_request(
            bytes.as_slice(),
            RequestKind::History,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(header.memory_bytes, DEFAULT_BYTES);
        assert_eq!(header.bytes, bytes.len());
    }
}

#[test]
fn malformed_string_escapes_and_excessive_nesting_are_rejected() {
    let text = String::from_utf8(encoded(&request())).unwrap();
    for replacement in [r#""bad\q""#, r#""bad\u12x4""#, "\"bad\n\""] {
        let malformed = text.replacen("\"feature_0\"", replacement, 1);
        assert_ne!(malformed, text);
        assert!(matches!(
            inspect_request(
                malformed.as_bytes(),
                RequestKind::Trace,
                &Cancellation::default()
            ),
            Err(Error::Invalid(_))
        ));
    }
    let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
    let malformed = text.replacen(
        "\"lipschitz\":\"f64:0000000000000000\"",
        &format!("\"lipschitz\":{nested}"),
        1,
    );
    assert_ne!(malformed, text);
    assert!(matches!(
        inspect_request(
            malformed.as_bytes(),
            RequestKind::Trace,
            &Cancellation::default()
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn independent_shape_mutations_are_refused_before_copying() {
    let original: serde_json::Value = serde_json::from_slice(&encoded(&request())).unwrap();
    let mut variants = Vec::new();
    let mut bad = original.clone();
    bad["covariances"][0]["n"] = 2.into();
    variants.push(bad);
    let mut bad = original.clone();
    bad["covariances"][0]["data"] = serde_json::json!([]);
    variants.push(bad);
    let mut bad = original.clone();
    bad["times"] = serde_json::json!(["f64:0000000000000000"]);
    variants.push(bad);
    let mut bad = original.clone();
    bad["features"] = serde_json::Value::Array(vec!["axis".into(); 257]);
    variants.push(bad);
    let mut bad = original;
    bad["times"] = serde_json::Value::Array(vec!["f64:0000000000000000".into(); 10001]);
    variants.push(bad);
    for malformed in variants {
        let bytes = serde_json::to_vec(&malformed).unwrap();
        assert!(matches!(
            inspect_request(
                bytes.as_slice(),
                RequestKind::Trace,
                &Cancellation::default()
            ),
            Err(Error::Invalid(_))
        ));
    }
}

struct CancelAfterRead<'a> {
    bytes: &'a [u8],
    cancel: &'a Cancellation,
    calls: &'a Cell<usize>,
    delivered: &'a Cell<usize>,
    maximum: &'a Cell<usize>,
}
impl Read for CancelAfterRead<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.calls.set(self.calls.get() + 1);
        self.maximum.set(self.maximum.get().max(out.len()));
        let count = self.bytes.len().min(out.len());
        out[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        self.delivered.set(self.delivered.get() + count);
        self.cancel.cancel();
        Ok(count)
    }
}

#[test]
fn cancellation_stops_before_the_second_bounded_read() {
    let mut input = request();
    input.features[0] = "x".repeat(4096);
    let bytes = encoded(&input);
    assert!(bytes.len() > 4096);
    let cancel = Cancellation::default();
    let (calls, delivered, maximum) = (Cell::new(0), Cell::new(0), Cell::new(0));
    let reader = CancelAfterRead {
        bytes: &bytes,
        cancel: &cancel,
        calls: &calls,
        delivered: &delivered,
        maximum: &maximum,
    };
    assert!(matches!(
        inspect_request(reader, RequestKind::Trace, &cancel),
        Err(Error::Cancelled)
    ));
    assert_eq!(calls.get(), 1);
    assert_eq!(delivered.get(), 4096);
    assert_eq!(maximum.get(), 4096);
}
