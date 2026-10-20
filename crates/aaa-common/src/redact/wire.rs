//! 타입을 지우기 전에 글과 B64를 구별한다. JSON의 wire 형태는 serde_json과 같다.
use super::{Error, bytes, text};
use base64::Engine;
use serde::Serialize;
use serde::ser::{self, Serializer};
use serde_json::{Map, Value};

pub(super) struct Guard;

fn string(value: &str) -> String {
    let mut out = text(value);
    if value.len() >= 12
        && value.len().is_multiple_of(4)
        && let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(value)
    {
        let clean = bytes(&raw);
        if clean != raw {
            out = text(&base64::engine::general_purpose::STANDARD.encode(clean));
        }
    }
    out
}
macro_rules! scalar {
    ($($method:ident($t:ty)),* $(,)?) => {$(
        fn $method(self, v: $t) -> Result<Value, Error> {
            serde_json::value::Serializer.$method(v).map_err(|_| Error::Serialize)
        }
    )*};
}
impl Serializer for Guard {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = Seq;
    type SerializeTuple = Seq;
    type SerializeTupleStruct = Seq;
    type SerializeTupleVariant = Seq;
    type SerializeMap = Object;
    type SerializeStruct = Object;
    type SerializeStructVariant = Object;
    scalar! { serialize_bool(bool), serialize_i8(i8), serialize_i16(i16), serialize_i32(i32),
    serialize_i64(i64), serialize_i128(i128), serialize_u8(u8), serialize_u16(u16),
    serialize_u32(u32), serialize_u64(u64), serialize_u128(u128), serialize_f32(f32),
    serialize_f64(f64), serialize_char(char) }
    fn serialize_str(self, v: &str) -> Result<Value, Error> {
        Ok(Value::String(string(v)))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, Error> {
        Ok(Value::Array(bytes(v).into_iter().map(Value::from).collect()))
    }
    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<Value, Error> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<Value, Error> {
        Ok(Value::String(variant.into()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, name: &'static str, v: &T) -> Result<Value, Error> {
        if name != aaa_proto::B64_SERDE_NAME {
            return v.serialize(self);
        }
        let Value::String(encoded) = serde_json::to_value(v).map_err(|_| Error::Serialize)? else {
            return Err(Error::Serialize);
        };
        let raw = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| Error::Serialize)?;
        // B64 글자에 우연히 겹친 인증 조각은 바꾸지 않는다. 실제 바이트만 가린다.
        Ok(Value::String(base64::engine::general_purpose::STANDARD.encode(bytes(&raw))))
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<Value, Error> {
        Ok(wrap(Some(variant), v.serialize(self)?))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Seq, Error> {
        Ok(Seq::new(None))
    }
    fn serialize_tuple(self, _: usize) -> Result<Seq, Error> {
        Ok(Seq::new(None))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Seq, Error> {
        Ok(Seq::new(None))
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Seq, Error> {
        Ok(Seq::new(Some(variant)))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Object, Error> {
        Ok(Object::new(None))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Object, Error> {
        Ok(Object::new(None))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Object, Error> {
        Ok(Object::new(Some(variant)))
    }
}
fn wrap(variant: Option<&'static str>, value: Value) -> Value {
    match variant {
        Some(variant) => Value::Object([(variant.into(), value)].into_iter().collect()),
        None => value,
    }
}
pub(super) struct Seq {
    values: Vec<Value>,
    variant: Option<&'static str>,
}
impl Seq {
    fn new(variant: Option<&'static str>) -> Self {
        Self { values: Vec::new(), variant }
    }
    fn push<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Error> {
        self.values.push(v.serialize(Guard)?);
        Ok(())
    }
    fn finish(self) -> Result<Value, Error> {
        Ok(wrap(self.variant, Value::Array(self.values)))
    }
}
macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Seq {
            type Ok = Value;
            type Error = Error;
            fn $method<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Error> {
                self.push(v)
            }
            fn end(self) -> Result<Value, Error> {
                self.finish()
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);

pub(super) struct Object {
    values: Map<String, Value>,
    key: Option<String>,
    variant: Option<&'static str>,
}
impl Object {
    fn new(variant: Option<&'static str>) -> Self {
        Self { values: Map::new(), key: None, variant }
    }
    fn field<T: Serialize + ?Sized>(&mut self, key: &str, v: &T) -> Result<(), Error> {
        if self.values.insert(key.into(), v.serialize(Guard)?).is_some() {
            return Err(Error::Serialize);
        }
        Ok(())
    }
    fn finish(self) -> Result<Value, Error> {
        if self.key.is_some() {
            return Err(Error::Serialize);
        }
        Ok(wrap(self.variant, Value::Object(self.values)))
    }
}
impl ser::SerializeMap for Object {
    type Ok = Value;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(Error::Serialize);
        }
        let key = match serde_json::to_value(v).map_err(|_| Error::Serialize)? {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => return Err(Error::Serialize),
        };
        self.key = Some(string(&key));
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Error> {
        let key = self.key.take().ok_or(Error::Serialize)?;
        self.field(&key, v)
    }
    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}
impl ser::SerializeStruct for Object {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<(), Error> {
        self.field(key, v)
    }
    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}
impl ser::SerializeStructVariant for Object {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<(), Error> {
        self.field(key, v)
    }
    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}
