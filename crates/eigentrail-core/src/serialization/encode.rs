//! Canonical serde encoding without a Value tree; buffers join the operation budget.
use crate::{
    Error, Result,
    resources::{
        Cancellation,
        memory::{self, HeapBytes},
    },
};
use serde::{
    Serialize,
    ser::{self, SerializeSeq},
};
use std::{
    fmt::{self, Write as _},
    io,
};

pub(super) trait Sink {
    fn write(&mut self, bytes: &[u8]) -> Result<()>;
}

fn invalid(message: &'static str) -> Error {
    match memory::text(message) {
        Ok(text) => Error::Invalid(text),
        Err(error) => error,
    }
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(_message: T) -> Self {
        invalid("unsupported serialization value")
    }
}

struct VecSink<'a>(&'a mut Vec<u8>);
impl Sink for VecSink<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        memory::extend(self.0, bytes)
    }
}

pub(super) fn to_vec<T: ?Sized + Serialize>(value: &T, cancel: &Cancellation) -> Result<Vec<u8>> {
    value_bytes(value, cancel, 0)
}

fn value_bytes<T: ?Sized + Serialize>(
    value: &T,
    cancel: &Cancellation,
    depth: usize,
) -> Result<Vec<u8>> {
    memory::scope(|| {
        let mut output = memory::capacity(0)?;
        value.serialize(&mut Encoder {
            sink: &mut VecSink(&mut output),
            cancel,
            depth,
        })?;
        Ok(output)
    })
}

pub(super) fn to_sink<T: ?Sized + Serialize>(
    value: &T,
    sink: &mut dyn Sink,
    cancel: &Cancellation,
) -> Result<()> {
    memory::scope(|| {
        value.serialize(&mut Encoder {
            sink,
            cancel,
            depth: 0,
        })
    })
}

struct Encoder<'a> {
    sink: &'a mut dyn Sink,
    cancel: &'a Cancellation,
    depth: usize,
}

impl Encoder<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        for piece in bytes.chunks(4096) {
            self.cancel.poll()?;
            self.sink.write(piece)?;
        }
        Ok(())
    }

    fn enter(&mut self, count: usize) -> Result<()> {
        self.cancel.poll()?;
        if self.depth + count > 128 {
            return Err(invalid("serialization nesting exceeds 128"));
        }
        self.depth += count;
        Ok(())
    }

    fn string(&mut self, text: &str, literal: bool) -> Result<()> {
        self.write(b"\"")?;
        if literal && (text.starts_with("f64:") || text.starts_with("str:")) {
            self.write(b"str:")?;
        }
        let bytes = text.as_bytes();
        let mut start = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            if i % 4096 == 0 {
                self.cancel.poll()?;
            }
            let short: Option<&[u8]> = match byte {
                b'"' => Some(b"\\\""),
                b'\\' => Some(b"\\\\"),
                8 => Some(b"\\b"),
                9 => Some(b"\\t"),
                10 => Some(b"\\n"),
                12 => Some(b"\\f"),
                13 => Some(b"\\r"),
                _ => None,
            };
            if short.is_some() || byte < 32 {
                self.write(&bytes[start..i])?;
                if let Some(escape) = short {
                    self.write(escape)?;
                } else {
                    let hex = b"0123456789abcdef";
                    self.write(&[
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        hex[(byte >> 4) as usize],
                        hex[(byte & 15) as usize],
                    ])?;
                }
                start = i + 1;
            }
        }
        self.write(&bytes[start..])?;
        self.write(b"\"")
    }

    fn integer(&mut self, value: impl fmt::Display) -> Result<()> {
        let mut buffer = StackText {
            bytes: [0; 40],
            len: 0,
        };
        write!(&mut buffer, "{value}").map_err(|_| invalid("integer formatting overflow"))?;
        self.write(&buffer.bytes[..buffer.len])
    }

    fn float(&mut self, value: f64) -> Result<()> {
        if !value.is_finite() {
            return Err(invalid("nonfinite float"));
        }
        let mut bytes = *b"\"f64:0000000000000000\"";
        let hex = b"0123456789abcdef";
        let bits = value.to_bits();
        for i in 0..16 {
            bytes[5 + i] = hex[((bits >> (4 * (15 - i))) & 15) as usize];
        }
        self.write(&bytes)
    }
}

struct StackText {
    bytes: [u8; 40],
    len: usize,
}
impl fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.len() > self.bytes.len() - self.len {
            return Err(fmt::Error);
        }
        self.bytes[self.len..self.len + text.len()].copy_from_slice(text.as_bytes());
        self.len += text.len();
        Ok(())
    }
}

enum Key {
    Static(&'static str),
    Owned(String),
}
impl Key {
    fn text(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Owned(s) => s,
        }
    }
}
impl HeapBytes for Key {
    fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Static(_) => Ok(0),
            Self::Owned(s) => s.heap_bytes(),
        }
    }
}
struct Entry {
    key: Key,
    value: Vec<u8>,
}
impl HeapBytes for Entry {
    fn heap_bytes(&self) -> Result<usize> {
        self.key
            .heap_bytes()?
            .checked_add(self.value.heap_bytes()?)
            .ok_or_else(|| invalid("object footprint overflow"))
    }
}

struct Object<'a, 'b> {
    encoder: &'a mut Encoder<'b>,
    fields: Vec<Entry>,
    next_key: Option<Key>,
    variant: Option<&'static str>,
}
impl Object<'_, '_> {
    fn field<T: ?Sized + Serialize>(&mut self, key: Key, value: &T) -> Result<()> {
        self.encoder.cancel.poll()?;
        let encoded = value_bytes(value, self.encoder.cancel, self.encoder.depth)?;
        if let Some(previous) = self
            .fields
            .iter_mut()
            .find(|entry| entry.key.text() == key.text())
        {
            previous.value = encoded;
        } else {
            memory::push(
                &mut self.fields,
                Entry {
                    key,
                    value: encoded,
                },
            )?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        if self.next_key.is_some() {
            return Err(invalid("map key has no value"));
        }
        self.fields
            .sort_unstable_by(|a, b| a.key.text().cmp(b.key.text()));
        if let Some(variant) = self.variant {
            self.encoder.write(b"{")?;
            self.encoder.string(variant, false)?;
            self.encoder.write(b":")?;
        }
        self.encoder.write(b"{")?;
        for (index, field) in self.fields.iter().enumerate() {
            if index > 0 {
                self.encoder.write(b",")?;
            }
            self.encoder.string(field.key.text(), false)?;
            self.encoder.write(b":")?;
            self.encoder.write(&field.value)?;
        }
        self.encoder.write(b"}")?;
        if self.variant.is_some() {
            self.encoder.write(b"}")?;
        }
        self.encoder.depth -= 1 + usize::from(self.variant.is_some());
        Ok(())
    }
}

struct Sequence<'a, 'b> {
    encoder: &'a mut Encoder<'b>,
    first: bool,
    variant: bool,
}
impl Sequence<'_, '_> {
    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if !self.first {
            self.encoder.write(b",")?;
        }
        self.first = false;
        value.serialize(&mut *self.encoder)
    }
    fn finish(self) -> Result<()> {
        self.encoder
            .write(if self.variant { b"]}" } else { b"]" })?;
        self.encoder.depth -= 1 + usize::from(self.variant);
        Ok(())
    }
}

macro_rules! integers {
    ($($method:ident:$ty:ty),* $(,)?) => {$ (
        fn $method(self, value: $ty) -> Result<()> { self.integer(value) }
    )*};
}

impl<'a, 'b> ser::Serializer for &'a mut Encoder<'b> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Sequence<'a, 'b>;
    type SerializeTuple = Sequence<'a, 'b>;
    type SerializeTupleStruct = Sequence<'a, 'b>;
    type SerializeTupleVariant = Sequence<'a, 'b>;
    type SerializeMap = Object<'a, 'b>;
    type SerializeStruct = Object<'a, 'b>;
    type SerializeStructVariant = Object<'a, 'b>;
    integers!(serialize_i8:i8,serialize_i16:i16,serialize_i32:i32,serialize_i64:i64,
        serialize_u8:u8,serialize_u16:u16,serialize_u32:u32,serialize_u64:u64);
    fn serialize_i128(self, value: i128) -> Result<()> {
        if u64::try_from(value).is_err() && i64::try_from(value).is_err() {
            return Err(invalid("integer out of range"));
        }
        self.integer(value)
    }
    fn serialize_u128(self, value: u128) -> Result<()> {
        if u64::try_from(value).is_err() {
            return Err(invalid("integer out of range"));
        }
        self.integer(value)
    }
    fn serialize_bool(self, value: bool) -> Result<()> {
        self.write(if value { b"true" } else { b"false" })
    }
    fn serialize_f32(self, value: f32) -> Result<()> {
        self.float(f64::from(value))
    }
    fn serialize_f64(self, value: f64) -> Result<()> {
        self.float(value)
    }
    fn serialize_char(self, value: char) -> Result<()> {
        self.string(value.encode_utf8(&mut [0; 4]), true)
    }
    fn serialize_str(self, value: &str) -> Result<()> {
        self.string(value, true)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<()> {
        let mut sequence = self.serialize_seq(Some(value.len()))?;
        for byte in value {
            sequence.serialize_element(byte)?;
        }
        sequence.end()
    }
    fn serialize_none(self) -> Result<()> {
        Err(invalid("null is not a numerical quantity"))
    }
    fn serialize_unit(self) -> Result<()> {
        self.serialize_none()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        self.serialize_none()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<()> {
        self.string(variant, true)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<()> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()> {
        self.enter(1)?;
        self.write(b"{")?;
        self.string(variant, false)?;
        self.write(b":")?;
        value.serialize(&mut *self)?;
        self.write(b"}")?;
        self.depth -= 1;
        Ok(())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        self.enter(1)?;
        self.write(b"[")?;
        Ok(Sequence {
            encoder: self,
            first: true,
            variant: false,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.enter(2)?;
        self.write(b"{")?;
        self.string(variant, false)?;
        self.write(b":[")?;
        Ok(Sequence {
            encoder: self,
            first: true,
            variant: true,
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        self.enter(1)?;
        let fields = memory::capacity(len.unwrap_or(0))?;
        Ok(Object {
            encoder: self,
            fields,
            next_key: None,
            variant: None,
        })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.enter(2)?;
        let fields = memory::capacity(len)?;
        Ok(Object {
            encoder: self,
            fields,
            next_key: None,
            variant: Some(variant),
        })
    }
    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<()> {
        self.string(&memory::format(format_args!("{value}"))?, true)
    }
}

impl ser::SerializeSeq for Sequence<'_, '_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.element(value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}
macro_rules! sequence_impl {
    ($trait:ident,$method:ident) => {
        impl ser::$trait for Sequence<'_, '_> {
            type Ok = ();
            type Error = Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
                self.element(value)
            }
            fn end(self) -> Result<()> {
                self.finish()
            }
        }
    };
}
sequence_impl!(SerializeTuple, serialize_element);
sequence_impl!(SerializeTupleStruct, serialize_field);
sequence_impl!(SerializeTupleVariant, serialize_field);
impl ser::SerializeMap for Object<'_, '_> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        self.next_key = Some(Key::Owned(key.serialize(KeyEncoder {
            cancel: self.encoder.cancel,
        })?));
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| invalid("map value has no key"))?;
        self.field(key, value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}
impl ser::SerializeStruct for Object<'_, '_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.field(Key::Static(key), value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}
impl ser::SerializeStructVariant for Object<'_, '_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.field(Key::Static(key), value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}

struct KeyEncoder<'a> {
    cancel: &'a Cancellation,
}
struct NumberWriter<'a> {
    bytes: &'a mut Vec<u8>,
    cancel: &'a Cancellation,
    error: Option<Error>,
}
impl io::Write for NumberWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self
            .cancel
            .poll()
            .and_then(|()| memory::extend(self.bytes, bytes));
        if let Err(error) = result {
            self.error = Some(error);
            return Err(io::ErrorKind::Other.into());
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl KeyEncoder<'_> {
    fn number<T: Serialize>(&self, value: T) -> Result<String> {
        memory::scope(|| {
            let mut bytes = memory::capacity(0)?;
            let mut writer = NumberWriter {
                bytes: &mut bytes,
                cancel: self.cancel,
                error: None,
            };
            if serde_json::to_writer(&mut writer, &value).is_err() {
                return Err(writer
                    .error
                    .unwrap_or_else(|| invalid("invalid numeric map key")));
            }
            String::from_utf8(bytes).map_err(|_| invalid("invalid numeric map key encoding"))
        })
    }
}
macro_rules! key_integers {
    ($($method:ident:$ty:ty),* $(,)?) => {$ (
        fn $method(self,value:$ty)->Result<String> { memory::format(format_args!("{value}")) }
    )*};
}
impl ser::Serializer for KeyEncoder<'_> {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = ser::Impossible<String, Error>;
    type SerializeTuple = ser::Impossible<String, Error>;
    type SerializeTupleStruct = ser::Impossible<String, Error>;
    type SerializeTupleVariant = ser::Impossible<String, Error>;
    type SerializeMap = ser::Impossible<String, Error>;
    type SerializeStruct = ser::Impossible<String, Error>;
    type SerializeStructVariant = ser::Impossible<String, Error>;
    key_integers!(serialize_i8:i8,serialize_i16:i16,serialize_i32:i32,serialize_i64:i64,serialize_i128:i128,
        serialize_u8:u8,serialize_u16:u16,serialize_u32:u32,serialize_u64:u64,serialize_u128:u128);
    fn serialize_f32(self, value: f32) -> Result<String> {
        if !value.is_finite() {
            return Err(invalid("nonfinite map key"));
        }
        self.number(value)
    }
    fn serialize_f64(self, value: f64) -> Result<String> {
        if !value.is_finite() {
            return Err(invalid("nonfinite map key"));
        }
        self.number(value)
    }
    fn serialize_bool(self, value: bool) -> Result<String> {
        memory::text(if value { "true" } else { "false" })
    }
    fn serialize_str(self, value: &str) -> Result<String> {
        memory::text(value)
    }
    fn serialize_char(self, value: char) -> Result<String> {
        memory::text(value.encode_utf8(&mut [0; 4]))
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String> {
        memory::text(variant)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String> {
        value.serialize(self)
    }
    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<String> {
        memory::format(format_args!("{value}"))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_none(self) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_unit(self) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String> {
        Err(invalid("invalid map key"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(invalid("invalid map key"))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(invalid("invalid map key"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(invalid("invalid map key"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(invalid("invalid map key"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        Err(invalid("invalid map key"))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        Err(invalid("invalid map key"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(invalid("invalid map key"))
    }
}
