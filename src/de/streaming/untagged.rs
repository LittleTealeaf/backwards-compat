use super::adjacent::OptionForwardingDeserializer;
use super::cautious_size_hint;
use crate::de::content::{Content, ContentDeserializer};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::de::{self, Deserialize, Deserializer, EnumAccess, MapAccess, SeqAccess, Visitor};

/// Dispatch target for [`UntaggedSingleVisitor`], used when a model has a single
/// unconditional unversioned (`_`) fallback variant.
pub trait UntaggedSingleDispatch<'de>: Sized {
    type Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>;

    fn dispatch_de<D>(deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>;
}

/// [`Visitor`] adapter delegating map visits to `H::visit_map` and non-map
/// values directly to `H::dispatch_de` without buffering sequences or enums.
pub struct UntaggedSingleVisitor<H>(pub H);

impl<'de, H> Visitor<'de> for UntaggedSingleVisitor<H>
where
    H: UntaggedSingleDispatch<'de>,
{
    type Value = H::Value;

    #[inline]
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    #[inline]
    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::Bool(v)))
    }

    #[inline]
    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::I8(v)))
    }

    #[inline]
    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::I16(v)))
    }

    #[inline]
    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::I32(v)))
    }

    #[inline]
    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::I64(v)))
    }

    #[inline]
    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::I128(Box::new(v))))
    }

    #[inline]
    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::U8(v)))
    }

    #[inline]
    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::U16(v)))
    }

    #[inline]
    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::U32(v)))
    }

    #[inline]
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::U64(v)))
    }

    #[inline]
    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::U128(Box::new(v))))
    }

    #[inline]
    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::F32(v)))
    }

    #[inline]
    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::F64(v)))
    }

    #[inline]
    fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::Char(v)))
    }

    #[inline]
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::String(Box::from(v))))
    }

    #[inline]
    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::Str(v)))
    }

    #[inline]
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::String(v.into_boxed_str())))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::ByteBuf(Box::from(v))))
    }

    #[inline]
    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::Bytes(v)))
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::ByteBuf(
            v.into_boxed_slice(),
        )))
    }

    #[inline]
    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::Unit))
    }

    #[inline]
    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_de(ContentDeserializer::<E>::new(Content::None))
    }

    #[inline]
    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = Content::deserialize(deserializer)?;
        H::dispatch_de(ContentDeserializer::new(Content::Some(Box::new(inner))))
    }

    #[inline]
    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    #[inline]
    fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        H::dispatch_de(OptionForwardingDeserializer::new(
            serde::de::value::SeqAccessDeserializer::new(seq),
        ))
    }

    #[inline]
    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.0.visit_map(map)
    }

    #[inline]
    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        H::dispatch_de(OptionForwardingDeserializer::new(
            serde::de::value::EnumAccessDeserializer::new(data),
        ))
    }
}

/// Dispatch target for [`UntaggedMultiVisitor`], used when a model has multiple
/// or cfg-gated unversioned (`_`) fallback variants requiring [`Content`] replay.
pub trait UntaggedMultiDispatch<'de>: Sized {
    type Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>;

    fn dispatch_content<E>(content: Content<'de>) -> Result<Self::Value, E>
    where
        E: de::Error;
}

/// [`Visitor`] adapter delegating map visits to `H::visit_map` and non-map
/// values into [`Content`] passed to `H::dispatch_content` for multi-arm retry.
pub struct UntaggedMultiVisitor<H>(pub H);

impl<'de, H> Visitor<'de> for UntaggedMultiVisitor<H>
where
    H: UntaggedMultiDispatch<'de>,
{
    type Value = H::Value;

    #[inline]
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    #[inline]
    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::Bool(v))
    }

    #[inline]
    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::I8(v))
    }

    #[inline]
    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::I16(v))
    }

    #[inline]
    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::I32(v))
    }

    #[inline]
    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::I64(v))
    }

    #[inline]
    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::I128(Box::new(v)))
    }

    #[inline]
    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::U8(v))
    }

    #[inline]
    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::U16(v))
    }

    #[inline]
    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::U32(v))
    }

    #[inline]
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::U64(v))
    }

    #[inline]
    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::U128(Box::new(v)))
    }

    #[inline]
    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::F32(v))
    }

    #[inline]
    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::F64(v))
    }

    #[inline]
    fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::Char(v))
    }

    #[inline]
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::String(Box::from(v)))
    }

    #[inline]
    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::Str(v))
    }

    #[inline]
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::String(v.into_boxed_str()))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::ByteBuf(Box::from(v)))
    }

    #[inline]
    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::Bytes(v))
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::ByteBuf(v.into_boxed_slice()))
    }

    #[inline]
    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::Unit)
    }

    #[inline]
    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        H::dispatch_content(Content::None)
    }

    #[inline]
    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = Content::deserialize(deserializer)?;
        H::dispatch_content(Content::Some(Box::new(inner)))
    }

    #[inline]
    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    #[inline]
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(cautious_size_hint::<Content<'de>>(seq.size_hint()));
        while let Some(elem) = seq.next_element()? {
            vec.push(elem);
        }
        H::dispatch_content(Content::Seq(vec.into_boxed_slice()))
    }

    #[inline]
    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.0.visit_map(map)
    }
}
