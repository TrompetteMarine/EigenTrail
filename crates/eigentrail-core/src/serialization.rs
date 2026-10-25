use crate::{
    Error, Result,
    resources::{Cancellation, memory},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
pub mod decode;
mod encode;
pub mod input;
pub use decode::{DecodeOwned, decode_in};
fn floats(v: &mut Value, encode: bool, cancel: &Cancellation) -> Result<()> {
    cancel.poll()?;
    match v {
        Value::Array(a) => {
            for x in a {
                floats(x, encode, cancel)?;
            }
        }
        Value::Object(m) => {
            for x in m.values_mut() {
                floats(x, encode, cancel)?;
            }
        }
        Value::Number(n) if encode && n.is_f64() => {
            let x = n
                .as_f64()
                .ok_or_else(|| Error::Invalid("float encoding".into()))?;
            if !x.is_finite() {
                return Err(Error::Invalid("nonfinite float".into()));
            }
            *v = Value::String(format!("f64:{:016x}", x.to_bits()));
        }
        Value::String(s) if encode && (s.starts_with("f64:") || s.starts_with("str:")) => {
            *s = format!("str:{s}");
        }
        Value::String(s) if !encode && s.starts_with("str:") => {
            *s = s[4..].to_string();
        }
        Value::String(s) if !encode && s.starts_with("f64:") => {
            if s.len() != 20
                || !s[4..]
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(Error::Invalid("malformed float bits".into()));
            }
            let bits = u64::from_str_radix(&s[4..], 16)
                .map_err(|_| Error::Invalid("float bits".into()))?;
            let x = f64::from_bits(bits);
            *v = Value::Number(
                serde_json::Number::from_f64(x)
                    .ok_or_else(|| Error::Invalid("nonfinite float bits".into()))?,
            );
        }
        Value::Null => {
            return Err(Error::Invalid(
                "null/nonfinite values are not a numerical quantity".into(),
            ));
        }
        _ => {}
    }
    Ok(())
}
struct CheckedReader<'a> {
    bytes: &'a [u8],
    cancel: &'a Cancellation,
    until_poll: usize,
}
impl Read for CheckedReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.until_poll == 0 {
            poll_io(self.cancel)?;
            self.until_poll = 4096;
        }
        let n = output.len().min(self.bytes.len()).min(self.until_poll);
        output[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        self.until_poll -= n;
        Ok(n)
    }
}
struct CheckedWriter<'a> {
    bytes: Vec<u8>,
    cancel: &'a Cancellation,
}
impl Write for CheckedWriter<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        poll_io(self.cancel)?;
        let n = input.len().min(4096);
        self.bytes
            .try_reserve(n)
            .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
        self.bytes.extend_from_slice(&input[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        poll_io(self.cancel)
    }
}
fn poll_io(cancel: &Cancellation) -> io::Result<()> {
    cancel
        .poll()
        .map_err(|_| io::Error::other("EigenTrail cancelled"))
}
fn json_result<T>(
    result: std::result::Result<T, serde_json::Error>,
    cancel: &Cancellation,
) -> Result<T> {
    cancel.poll()?;
    result.map_err(|e| {
        if e.io_error_kind() == Some(io::ErrorKind::OutOfMemory) {
            Error::Resource("canonical output allocation refused".into())
        } else {
            Error::Invalid(e.to_string())
        }
    })
}
fn canonical(value: &Value, cancel: &Cancellation) -> Result<Vec<u8>> {
    let mut writer = CheckedWriter {
        bytes: Vec::new(),
        cancel,
    };
    json_result(serde_json::to_writer(&mut writer, value), cancel)?;
    Ok(writer.bytes)
}
/// Own interpreter text before worker detachment; no Python/R objects cross threads.
pub fn owned_text(text: &str) -> Result<String> {
    memory::text(text)
}
pub fn encode<T: Serialize>(record: &T) -> Result<Vec<u8>> {
    encode_with_cancel(record, &Cancellation::default())
}
pub fn encode_with_cancel<T: Serialize>(record: &T, cancel: &Cancellation) -> Result<Vec<u8>> {
    let limit = memory::active_limit().unwrap_or(crate::resources::DEFAULT_BYTES);
    memory::ensure_operation(limit, 0, || {
        cancel.poll()?;
        encode::to_vec(record, cancel)
    })
}
pub fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    decode_with_cancel(bytes, &Cancellation::default())
}
pub fn decode_with_cancel<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
    cancel: &Cancellation,
) -> Result<T> {
    // Generic compatibility path; supported production records use decode_in.
    cancel.poll()?;
    let reader = CheckedReader {
        bytes,
        cancel,
        until_poll: 0,
    };
    let mut value: Value = json_result(serde_json::from_reader(reader), cancel)?;
    // Canonical equality rejects duplicate keys and alternate float spellings.
    if canonical(&value, cancel)? != bytes {
        return Err(Error::Invalid(
            "canonical JSON required (sorted keys, no duplicate keys or whitespace)".into(),
        ));
    }
    floats(&mut value, false, cancel)?;
    let record: T = json_result(serde_json::from_value(value), cancel)?;
    if encode_with_cancel(&record, cancel)? != bytes {
        return Err(Error::Invalid(
            "noncanonical typed float representation".into(),
        ));
    }
    Ok(record)
}
pub fn digest<T: Serialize>(v: &T) -> Result<String> {
    digest_with_cancel(v, &Cancellation::default())
}
pub fn digest_with_cancel<T: Serialize>(v: &T, cancel: &Cancellation) -> Result<String> {
    struct HashSink(Sha256);
    impl encode::Sink for HashSink {
        fn write(&mut self, bytes: &[u8]) -> Result<()> {
            self.0.update(bytes);
            Ok(())
        }
    }
    let limit = memory::active_limit().unwrap_or(crate::resources::DEFAULT_BYTES);
    memory::ensure_operation(limit, 0, || {
        cancel.poll()?;
        let mut sink = HashSink(Sha256::new());
        encode::to_sink(v, &mut sink, cancel)?;
        cancel.poll()?;
        memory::format(format_args!("{:x}", sink.0.finalize()))
    })
}

/// Check exact canonical equality without keeping a second complete wire buffer.
pub fn compare_encoding<T: ?Sized + Serialize>(
    record: &T,
    bytes: &[u8],
    cancel: &Cancellation,
) -> Result<()> {
    struct CompareSink<'a> {
        bytes: &'a [u8],
        position: usize,
    }
    impl encode::Sink for CompareSink<'_> {
        fn write(&mut self, piece: &[u8]) -> Result<()> {
            let end = self.position.checked_add(piece.len());
            if end.and_then(|end| self.bytes.get(self.position..end)) != Some(piece) {
                return Err(Error::Invalid(memory::text(
                    "noncanonical typed representation",
                )?));
            }
            self.position = end.expect("checked comparison length");
            Ok(())
        }
    }
    let limit = memory::active_limit().unwrap_or(crate::resources::DEFAULT_BYTES);
    memory::ensure_operation(limit, 0, || {
        cancel.poll()?;
        let mut sink = CompareSink { bytes, position: 0 };
        encode::to_sink(record, &mut sink, cancel)?;
        if sink.position != bytes.len() {
            return Err(Error::Invalid(memory::text(
                "noncanonical typed representation",
            )?));
        }
        cancel.poll()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn io_poll_quantum_and_cancelled_results() {
        let cancel = Cancellation::default();
        let data = [b' '; 8192];
        let mut reader = CheckedReader {
            bytes: &data,
            cancel: &cancel,
            until_poll: 0,
        };
        let mut output = [0; 8192];
        assert_eq!(reader.read(&mut output).unwrap(), 4096);
        let mut writer = CheckedWriter {
            bytes: Vec::new(),
            cancel: &cancel,
        };
        assert_eq!(writer.write(&data).unwrap(), 4096);
        cancel.cancel();
        assert!(reader.read(&mut output).is_err());
        assert!(writer.write(&data).is_err());
        assert_eq!(
            decode_with_cancel::<Vec<f64>>(b"[]", &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(
            encode_with_cancel(&vec![1.], &cancel),
            Err(Error::Cancelled)
        );
    }

    #[test]
    fn canonical_streams_preserve_bits_text_order_and_hash() {
        #[derive(Serialize)]
        struct Wire<'a> {
            z: [f64; 3],
            k: &'a str,
            a: &'a str,
        }
        let record = Wire {
            z: [-0., f64::from_bits(1), f64::MAX],
            k: "\"\n\0é",
            a: "f64:literal",
        };
        let expected = "{\"a\":\"str:f64:literal\",\"k\":\"\\\"\\n\\u0000é\",\"z\":[\"f64:8000000000000000\",\"f64:0000000000000001\",\"f64:7fefffffffffffff\"]}";
        let bytes = encode(&record).unwrap();
        assert_eq!(bytes, expected.as_bytes());
        let cancel = Cancellation::default();
        compare_encoding(&record, &bytes, &cancel).unwrap();
        assert_eq!(
            digest(&record).unwrap(),
            format!("{:x}", Sha256::digest(&bytes))
        );
        let mut altered = bytes.clone();
        altered.push(b' ');
        assert!(compare_encoding(&record, &altered, &cancel).is_err());
        altered[5] ^= 1;
        assert!(compare_encoding(&record, &altered, &cancel).is_err());
        cancel.cancel();
        assert_eq!(
            compare_encoding(&record, &bytes, &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(digest_with_cancel(&record, &cancel), Err(Error::Cancelled));
    }

    #[test]
    fn dynamic_map_keys_keep_legacy_sorting_and_last_value() {
        struct Map;
        impl Serialize for Map {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("f64:key", "str:value")?;
                map.serialize_entry("\n", &false)?;
                map.serialize_entry("f64:key", "last")?;
                map.serialize_entry(&-0f64, &true)?;
                map.end()
            }
        }
        assert_eq!(
            encode(&Map).unwrap(),
            b"{\"\\n\":false,\"-0.0\":true,\"f64:key\":\"last\"}"
        );
        assert!(encode(&f64::NAN).is_err());
        assert!(encode(&None::<f64>).is_err());
        assert!(encode(&u128::MAX).is_err());
    }
}
