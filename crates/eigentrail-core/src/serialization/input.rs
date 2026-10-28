//! Inspect supported request envelopes before allocating interpreter/file copies.
use crate::{
    Error, Result,
    resources::{self, Cancellation},
};
use std::io::Read;

pub const MAX_INPUT_BYTES: usize = resources::DEFAULT_BYTES / 2;

#[derive(Clone, Copy)]
pub enum RequestKind {
    Trace,
    History,
    LineageSummary,
    Transport,
}

pub struct InputHeader {
    pub memory_bytes: usize,
    pub bytes: usize,
}

/// Limits owned input payload only; internal work/output still need shared accounting.
pub fn preflight(request: &str, companions: &[&str], kind: RequestKind) -> Result<usize> {
    preflight_with_cancel(request, companions, kind, &Cancellation::default())
}

pub fn preflight_with_cancel(
    request: &str,
    companions: &[&str],
    kind: RequestKind,
    cancel: &Cancellation,
) -> Result<usize> {
    preflight_with_poll(request, companions, kind, || cancel.poll())
}

/// Interpreter boundaries poll their own signal handler while inspecting borrowed input.
pub fn preflight_with_poll(
    request: &str,
    companions: &[&str],
    kind: RequestKind,
    mut poll: impl FnMut() -> Result<()>,
) -> Result<usize> {
    let header = inspect_with_poll(request.as_bytes(), kind, &mut poll)?;
    let total = companions.iter().try_fold(header.bytes, |total, text| {
        total
            .checked_add(text.len())
            .ok_or_else(|| Error::Resource("serialized input size overflow".into()))
    })?;
    if total > header.memory_bytes.min(MAX_INPUT_BYTES) {
        return Err(Error::Resource(
            "aggregate serialized inputs exceed input budget".into(),
        ));
    }
    poll()?;
    Ok(header.memory_bytes)
}

/// Heap-free inspection; canonical/typed/scientific validation remains in decode/check.
pub fn inspect_request(
    reader: impl Read,
    kind: RequestKind,
    cancel: &Cancellation,
) -> Result<InputHeader> {
    inspect_with_poll(reader, kind, &mut || cancel.poll())
}

fn inspect_with_poll(
    reader: impl Read,
    kind: RequestKind,
    poll: &mut dyn FnMut() -> Result<()>,
) -> Result<InputHeader> {
    poll()?;
    let mut scan = Scanner {
        reader,
        buffer: [0; 4096],
        position: 0,
        end: 0,
        bytes: 0,
        poll,
        package_keys: false,
    };
    let shape = match kind {
        RequestKind::Trace => scan.trace()?,
        RequestKind::History => scan.history()?,
        RequestKind::LineageSummary => scan.lineage_summary()?,
        RequestKind::Transport => scan.transport()?,
    };
    if scan.peek()?.is_some() {
        return invalid("canonical request must end after its outer object");
    }
    if scan.bytes > shape.memory_bytes {
        return Err(Error::Resource(
            "serialized request exceeds declared input budget".into(),
        ));
    }
    match kind {
        RequestKind::Transport => {
            crate::transport_workflow::preflight(shape.features, shape.dates, shape.memory_bytes)?;
        }
        _ => {
            resources::preflight(shape.features, shape.dates, shape.memory_bytes)?;
        }
    }
    (scan.poll)()?;
    Ok(InputHeader {
        memory_bytes: shape.memory_bytes,
        bytes: scan.bytes,
    })
}

/// Supported package schemas have bounded literal field names; values may contain Unicode.
pub(crate) fn validate_package_keys(bytes: &[u8], cancel: &Cancellation) -> Result<()> {
    let mut poll = || cancel.poll();
    let mut scan = Scanner {
        reader: bytes,
        buffer: [0; 4096],
        position: 0,
        end: 0,
        bytes: 0,
        poll: &mut poll,
        package_keys: true,
    };
    scan.value(1)?;
    if scan.peek()?.is_some() {
        return invalid("trailing package JSON bytes");
    }
    Ok(())
}

fn invalid<T>(message: &'static str) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

#[derive(Clone, Copy)]
struct Shape {
    features: usize,
    dates: usize,
    memory_bytes: usize,
    domain: [f64; 2],
}

struct Scanner<'a, R> {
    reader: R,
    buffer: [u8; 4096],
    position: usize,
    end: usize,
    bytes: usize,
    poll: &'a mut dyn FnMut() -> Result<()>,
    package_keys: bool,
}

impl<R: Read> Scanner<'_, R> {
    fn peek(&mut self) -> Result<Option<u8>> {
        if self.position == self.end {
            (self.poll)()?;
            self.end = loop {
                match self.reader.read(&mut self.buffer) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (self.poll)()?,
                    Err(e) => return Err(Error::Invalid(e.to_string())),
                }
            };
            self.position = 0;
        }
        Ok((self.position < self.end).then(|| self.buffer[self.position]))
    }

    fn byte(&mut self) -> Result<u8> {
        let next = self
            .peek()?
            .ok_or_else(|| Error::Invalid("unfinished JSON input".into()))?;
        if self.bytes == MAX_INPUT_BYTES {
            return Err(Error::Resource(
                "serialized input exceeds input ceiling".into(),
            ));
        }
        self.position += 1;
        self.bytes += 1;
        Ok(next)
    }

    fn expect(&mut self, expected: u8) -> Result<()> {
        if self.byte()? != expected {
            return invalid("malformed/noncanonical JSON input");
        }
        Ok(())
    }

    fn take(&mut self, expected: u8) -> Result<bool> {
        if self.peek()? == Some(expected) {
            self.byte()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn field(&mut self, fields: &[&[u8]], seen: &mut u32) -> Result<usize> {
        self.expect(b'"')?;
        let mut key = [0; 32];
        let mut length = 0;
        loop {
            match self.byte()? {
                b'"' => break,
                b'\\' | 0..=31 => return invalid("literal supported request field required"),
                byte => {
                    if length == key.len() {
                        return invalid("unknown request field");
                    }
                    key[length] = byte;
                    length += 1;
                }
            }
        }
        let index = fields
            .iter()
            .position(|field| *field == &key[..length])
            .ok_or_else(|| Error::Invalid("unknown request field".into()))?;
        let bit = 1u32 << index;
        if *seen & bit != 0 {
            return invalid("duplicate request field");
        }
        *seen |= bit;
        self.expect(b':')?;
        Ok(index)
    }

    fn unsigned(&mut self) -> Result<usize> {
        let first = self.byte()?;
        if !first.is_ascii_digit() {
            return invalid("unsigned request integer required");
        }
        let mut value = usize::from(first - b'0');
        while self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
            if first == b'0' {
                return invalid("canonical unsigned integer required");
            }
            let digit = usize::from(self.byte()? - b'0');
            value = value
                .checked_mul(10)
                .and_then(|n| n.checked_add(digit))
                .ok_or_else(|| Error::Invalid("request integer overflow".into()))?;
        }
        Ok(value)
    }

    fn array_count(&mut self, maximum: usize) -> Result<usize> {
        self.expect(b'[')?;
        if self.take(b']')? {
            return Ok(0);
        }
        let mut count = 0;
        loop {
            if count == maximum {
                return invalid("request array exceeds shape ceiling");
            }
            self.value(1)?;
            count += 1;
            if self.take(b']')? {
                return Ok(count);
            }
            self.expect(b',')?;
        }
    }

    fn matrix(&mut self) -> Result<usize> {
        let mut seen = 0;
        let (mut elements, mut dimension) = (0, 0);
        self.expect(b'{')?;
        loop {
            match self.field(&[b"data", b"n"], &mut seen)? {
                0 => elements = self.array_count(256 * 256)?,
                _ => dimension = self.unsigned()?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if seen != 3 || !(1..=256).contains(&dimension) || elements != dimension * dimension {
            return invalid("square matrix dimension/data required");
        }
        Ok(dimension)
    }

    fn matrices(&mut self, maximum: usize, transport: bool) -> Result<(usize, usize)> {
        self.expect(b'[')?;
        if self.take(b']')? {
            return Ok((0, 0));
        }
        let (mut count, mut dimension) = (0, 0);
        loop {
            if count == maximum {
                return if transport {
                    Err(Error::Exhausted(
                        "transport refinement limit 65536 cells".into(),
                    ))
                } else {
                    invalid("too many covariance dates")
                };
            }
            let current = self.matrix()?;
            if count > 0 && current != dimension {
                return invalid("common covariance dimension required");
            }
            dimension = current;
            count += 1;
            if self.take(b']')? {
                return Ok((count, dimension));
            }
            self.expect(b',')?;
        }
    }

    fn trace(&mut self) -> Result<Shape> {
        const FIELDS: &[&[u8]] = &[
            b"schema",
            b"times",
            b"covariances",
            b"features",
            b"lipschitz",
            b"calibration",
            b"correction",
            b"target",
            b"numerical_allowance",
            b"memory_bytes",
        ];
        let mut seen = 0;
        let mut domain = [0.; 2];
        let (mut dates, mut features, mut matrices, mut dimension, mut memory_bytes) =
            (0, 0, 0, 0, 0);
        self.expect(b'{')?;
        loop {
            match self.field(FIELDS, &mut seen)? {
                1 => (dates, domain) = self.times(10000, false)?,
                2 => (matrices, dimension) = self.matrices(10000, false)?,
                3 => features = self.array_count(256)?,
                9 => memory_bytes = self.unsigned()?,
                _ => self.value(1)?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if seen != (1 << FIELDS.len()) - 1 || matrices != dates || dimension != features {
            return invalid("complete request with consistent feature/date shapes required");
        }
        if memory_bytes > resources::DEFAULT_BYTES {
            return invalid("managed budget must be at most 512 MiB");
        }
        Ok(Shape {
            features,
            dates,
            memory_bytes,
            domain,
        })
    }

    fn history(&mut self) -> Result<Shape> {
        let mut seen = 0;
        let (mut observations, mut configurations) = (None, 0);
        let mut models = [None; 16];
        self.expect(b'{')?;
        loop {
            match self.field(
                &[
                    b"schema",
                    b"observations",
                    b"configurations",
                    b"query_times",
                ],
                &mut seen,
            )? {
                1 => observations = Some(self.trace()?),
                2 => configurations = self.configurations(&mut models)?,
                3 => {
                    self.array_count(10000)?;
                }
                _ => self.value(1)?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if seen != 15 || configurations == 0 {
            return invalid("complete history request required");
        }
        let observations =
            observations.ok_or_else(|| Error::Invalid("history observations required".into()))?;
        for model in models.into_iter().flatten() {
            if model.features != observations.features
                || model.memory_bytes != observations.memory_bytes
                || model.domain != observations.domain
            {
                return invalid(
                    "transport model must match observation dimension, budget and domain",
                );
            }
        }
        Ok(observations)
    }

    fn lineage_summary(&mut self) -> Result<Shape> {
        let mut seen = 0;
        let mut history = None;
        let mut delta = 0.;
        self.expect(b'{')?;
        loop {
            match self.field(&[b"schema", b"history", b"delta"], &mut seen)? {
                1 => history = Some(self.history()?),
                2 => delta = self.finite()?,
                _ => self.value(1)?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if seen != 7 || delta <= 0. {
            return invalid("complete lineage-summary with finite positive delta required");
        }
        history.ok_or_else(|| Error::Invalid("lineage-summary history required".into()))
    }

    fn finite(&mut self) -> Result<f64> {
        self.expect(b'"')?;
        for byte in b"f64:" {
            self.expect(*byte)?;
        }
        let mut bits = 0u64;
        for _ in 0..16 {
            let digit = match self.byte()? {
                b @ b'0'..=b'9' => b - b'0',
                b @ b'a'..=b'f' => b - b'a' + 10,
                _ => return invalid("canonical finite IEEE-754 encoding required"),
            };
            bits = (bits << 4) | u64::from(digit);
        }
        self.expect(b'"')?;
        let value = f64::from_bits(bits);
        if !value.is_finite() {
            return invalid("finite request value required");
        }
        Ok(value)
    }

    fn times(&mut self, maximum: usize, transport: bool) -> Result<(usize, [f64; 2])> {
        self.expect(b'[')?;
        if self.take(b']')? {
            return Ok((0, [0.; 2]));
        }
        let mut count = 0;
        let mut domain = [0.; 2];
        loop {
            if count == maximum {
                return if transport {
                    Err(Error::Exhausted(
                        "transport refinement limit 65536 cells".into(),
                    ))
                } else {
                    invalid("too many observation dates")
                };
            }
            let time = self.finite()?;
            if count == 0 {
                domain[0] = time;
            } else if time <= domain[1] {
                return invalid("strictly ordered request times required");
            }
            domain[1] = time;
            count += 1;
            if self.take(b']')? {
                return Ok((count, domain));
            }
            self.expect(b',')?;
        }
    }

    fn transport(&mut self) -> Result<Shape> {
        const FIELDS: &[&[u8]] = &[
            b"schema",
            b"dimension",
            b"times",
            b"reference_nodes",
            b"nodes",
            b"block",
            b"initial_frame",
            b"initial_gauge",
            b"reference_speed_limit",
            b"max_speed",
            b"endpoint_error_limit",
            b"initial_correction_limit",
            b"initial_mismatch_limit",
            b"target",
            b"memory_bytes",
        ];
        let mut seen = 0;
        let (mut features, mut dates, mut memory_bytes) = (0, 0, 0);
        let (mut reference, mut nodes, mut frame) = ((0, 0), (0, 0), 0);
        let mut domain = [0.; 2];
        self.expect(b'{')?;
        loop {
            match self.field(FIELDS, &mut seen)? {
                1 => features = self.unsigned()?,
                2 => (dates, domain) = self.times(65537, true)?,
                3 => reference = self.matrices(65537, true)?,
                4 => nodes = self.matrices(65537, true)?,
                6 => frame = self.matrix()?,
                7 => {
                    self.matrix()?;
                }
                14 => memory_bytes = self.unsigned()?,
                _ => self.value(1)?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if seen != (1 << FIELDS.len()) - 1
            || reference != (dates, features)
            || nodes != (dates, features)
            || frame != features
        {
            return invalid("complete transport request with consistent geometry shapes required");
        }
        crate::transport_workflow::preflight(features, dates, memory_bytes)?;
        Ok(Shape {
            features,
            dates,
            memory_bytes,
            domain,
        })
    }

    fn class(&mut self) -> Result<Option<Shape>> {
        let mut seen = 0;
        let mut initialized = None;
        let mut model = None;
        self.expect(b'{')?;
        loop {
            match self.field(&[b"kind", b"model", b"windows"], &mut seen)? {
                0 => {
                    self.expect(b'"')?;
                    let mut tag = [0u8; 32];
                    let mut length = 0;
                    loop {
                        let byte = self.byte()?;
                        if byte == b'"' {
                            break;
                        }
                        if length == tag.len() || !byte.is_ascii_alphabetic() {
                            return invalid("supported literal path class required");
                        }
                        tag[length] = byte;
                        length += 1;
                    }
                    initialized = Some(&tag[..length] == b"InitializedTransport");
                }
                1 => match initialized {
                    Some(true) => model = Some(self.transport()?),
                    Some(false) => self.value(1)?,
                    None => return invalid("canonical class kind must precede its model"),
                },
                _ => self.value(1)?,
            }
            if self.take(b'}')? {
                break;
            }
            self.expect(b',')?;
        }
        if initialized.is_none() || (initialized == Some(true) && (seen != 3 || model.is_none())) {
            return invalid("complete initialized transport class required");
        }
        Ok(model)
    }

    fn configurations(&mut self, models: &mut [Option<Shape>; 16]) -> Result<usize> {
        self.expect(b'[')?;
        if self.take(b']')? {
            return Ok(0);
        }
        let mut count = 0;
        loop {
            if count == models.len() {
                return invalid("request array exceeds shape ceiling");
            }
            let mut seen = 0;
            self.expect(b'{')?;
            loop {
                match self.field(&[b"id", b"lipschitz", b"class", b"witnesses"], &mut seen)? {
                    2 => models[count] = self.class()?,
                    _ => self.value(1)?,
                }
                if self.take(b'}')? {
                    break;
                }
                self.expect(b',')?;
            }
            if seen != 15 {
                return invalid("complete history configuration required");
            }
            count += 1;
            if self.take(b']')? {
                return Ok(count);
            }
            self.expect(b',')?;
        }
    }

    fn string(&mut self) -> Result<()> {
        self.expect(b'"')?;
        loop {
            match self.byte()? {
                b'"' => return Ok(()),
                b'\\' => match self.byte()? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        for _ in 0..4 {
                            if !self.byte()?.is_ascii_hexdigit() {
                                return invalid("malformed JSON escape");
                            }
                        }
                    }
                    _ => return invalid("malformed JSON escape"),
                },
                0..=31 => return invalid("unescaped JSON control byte"),
                _ => {}
            }
        }
    }

    fn package_key(&mut self) -> Result<()> {
        self.expect(b'"')?;
        for _ in 0..=64 {
            match self.byte()? {
                b'"' => return Ok(()),
                b'\\' | 0..=31 | 127..=255 => return invalid("literal package field required"),
                _ => {}
            }
        }
        invalid("package field name too long")
    }

    fn number(&mut self) -> Result<()> {
        self.take(b'-')?;
        match self.byte()? {
            b'0' => {}
            b'1'..=b'9' => {
                while self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
                    self.byte()?;
                }
            }
            _ => return invalid("malformed JSON number"),
        }
        if self.take(b'.')? {
            self.digits()?;
        }
        if matches!(self.peek()?, Some(b'e' | b'E')) {
            self.byte()?;
            if matches!(self.peek()?, Some(b'+' | b'-')) {
                self.byte()?;
            }
            self.digits()?;
        }
        Ok(())
    }

    fn digits(&mut self) -> Result<()> {
        if !self.byte()?.is_ascii_digit() {
            return invalid("malformed JSON number");
        }
        while self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
            self.byte()?;
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<()> {
        if depth > 128 {
            return invalid("JSON nesting limit exceeded");
        }
        match self.peek()? {
            Some(b'"') => self.string(),
            Some(b'[') => {
                self.byte()?;
                if self.take(b']')? {
                    return Ok(());
                }
                loop {
                    self.value(depth + 1)?;
                    if self.take(b']')? {
                        return Ok(());
                    }
                    self.expect(b',')?;
                }
            }
            Some(b'{') => {
                self.byte()?;
                if self.take(b'}')? {
                    return Ok(());
                }
                loop {
                    if self.package_keys {
                        self.package_key()?;
                    } else {
                        self.string()?;
                    }
                    self.expect(b':')?;
                    self.value(depth + 1)?;
                    if self.take(b'}')? {
                        return Ok(());
                    }
                    self.expect(b',')?;
                }
            }
            Some(b't' | b'f' | b'n') => {
                let literal: &[u8] = match self.peek()? {
                    Some(b't') => b"true",
                    Some(b'f') => b"false",
                    _ => b"null",
                };
                for &byte in literal {
                    self.expect(byte)?;
                }
                Ok(())
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => invalid("malformed/noncanonical JSON value"),
        }
    }
}
