mod borrowed;
mod owned;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ref;

use crate::de::streaming::cautious_size_hint;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Unexpected, Visitor};

pub use borrowed::ContentRefDeserializer;
pub use owned::ContentDeserializer;

#[derive(Debug)]
pub enum Content<'de> {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(Box<u128>),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(Box<i128>),
    F32(f32),
    F64(f64),
    Char(char),
    String(Box<str>),
    Str(&'de str),
    ByteBuf(Box<[u8]>),
    Bytes(&'de [u8]),
    None,
    Some(Box<Self>),
    Unit,
    Newtype(Box<Self>),
    Seq(Box<[Self]>),
    Map(Box<[(Self, Self)]>),
}

impl Content<'_> {
    #[cold]
    pub(super) fn unexpected(&self) -> Unexpected<'_> {
        match *self {
            Self::Bool(b) => Unexpected::Bool(b),
            Self::U8(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U16(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U32(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U64(n) => Unexpected::Unsigned(n),
            Self::U128(_) => Unexpected::Other("128-bit unsigned integer"),
            Self::I8(n) => Unexpected::Signed(i64::from(n)),
            Self::I16(n) => Unexpected::Signed(i64::from(n)),
            Self::I32(n) => Unexpected::Signed(i64::from(n)),
            Self::I64(n) => Unexpected::Signed(n),
            Self::I128(_) => Unexpected::Other("128-bit signed integer"),
            Self::F32(f) => Unexpected::Float(f64::from(f)),
            Self::F64(f) => Unexpected::Float(f),
            Self::Char(c) => Unexpected::Char(c),
            Self::String(ref s) => Unexpected::Str(s),
            Self::Str(s) => Unexpected::Str(s),
            Self::ByteBuf(ref b) => Unexpected::Bytes(b),
            Self::Bytes(b) => Unexpected::Bytes(b),
            Self::None | Self::Some(_) => Unexpected::Option,
            Self::Unit => Unexpected::Unit,
            Self::Newtype(_) => Unexpected::NewtypeStruct,
            Self::Seq(_) => Unexpected::Seq,
            Self::Map(_) => Unexpected::Map,
        }
    }
}

impl<'de> Deserialize<'de> for Content<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ContentVisitor { value: PhantomData })
    }
}

struct ContentVisitor<'de> {
    value: PhantomData<Content<'de>>,
}

impl<'de> Visitor<'de> for ContentVisitor<'de> {
    type Value = Content<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_bool<F>(self, v: bool) -> Result<Self::Value, F> {
        Ok(Content::Bool(v))
    }

    fn visit_i8<F>(self, v: i8) -> Result<Self::Value, F> {
        Ok(Content::I8(v))
    }

    fn visit_i16<F>(self, v: i16) -> Result<Self::Value, F> {
        Ok(Content::I16(v))
    }

    fn visit_i32<F>(self, v: i32) -> Result<Self::Value, F> {
        Ok(Content::I32(v))
    }

    fn visit_i64<F>(self, v: i64) -> Result<Self::Value, F> {
        Ok(Content::I64(v))
    }

    fn visit_i128<F>(self, v: i128) -> Result<Self::Value, F> {
        Ok(Content::I128(Box::new(v)))
    }

    fn visit_u8<F>(self, v: u8) -> Result<Self::Value, F> {
        Ok(Content::U8(v))
    }

    fn visit_u16<F>(self, v: u16) -> Result<Self::Value, F> {
        Ok(Content::U16(v))
    }

    fn visit_u32<F>(self, v: u32) -> Result<Self::Value, F> {
        Ok(Content::U32(v))
    }

    fn visit_u64<F>(self, v: u64) -> Result<Self::Value, F> {
        Ok(Content::U64(v))
    }

    fn visit_u128<F>(self, v: u128) -> Result<Self::Value, F> {
        Ok(Content::U128(Box::new(v)))
    }

    fn visit_f32<F>(self, v: f32) -> Result<Self::Value, F> {
        Ok(Content::F32(v))
    }

    fn visit_f64<F>(self, v: f64) -> Result<Self::Value, F> {
        Ok(Content::F64(v))
    }

    fn visit_char<F>(self, v: char) -> Result<Self::Value, F> {
        Ok(Content::Char(v))
    }

    fn visit_str<F>(self, v: &str) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Content::String(Box::from(v)))
    }

    fn visit_borrowed_str<F>(self, v: &'de str) -> Result<Self::Value, F> {
        Ok(Content::Str(v))
    }

    fn visit_string<F>(self, v: String) -> Result<Self::Value, F> {
        Ok(Content::String(v.into_boxed_str()))
    }

    fn visit_bytes<F>(self, v: &[u8]) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Content::ByteBuf(Box::from(v)))
    }

    fn visit_borrowed_bytes<F>(self, v: &'de [u8]) -> Result<Self::Value, F> {
        Ok(Content::Bytes(v))
    }

    fn visit_byte_buf<F>(self, v: Vec<u8>) -> Result<Self::Value, F> {
        Ok(Content::ByteBuf(v.into_boxed_slice()))
    }

    fn visit_unit<F>(self) -> Result<Self::Value, F> {
        Ok(Content::Unit)
    }

    fn visit_none<F>(self) -> Result<Self::Value, F> {
        Ok(Content::None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Content::Some(Box::new(v)))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Content::Newtype(Box::new(v)))
    }

    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(cautious_size_hint::<Content<'de>>(seq.size_hint()));
        while let Some(e) = seq.next_element()? {
            vec.push(e);
        }
        Ok(Content::Seq(vec.into_boxed_slice()))
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut vec = Vec::with_capacity(cautious_size_hint::<(Content<'de>, Content<'de>)>(
            map.size_hint(),
        ));
        while let Some(kv) = map.next_entry()? {
            vec.push(kv);
        }
        Ok(Content::Map(vec.into_boxed_slice()))
    }
}
