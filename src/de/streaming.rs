use super::content::{Content, ContentDeserializer};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::{IntoIter as VecIntoIter, Vec};
use core::fmt;
use core::marker::PhantomData;
use serde::de::{
    self, Deserialize, DeserializeSeed, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess,
    Unexpected, VariantAccess, Visitor,
};

/// Caps pre-allocation from untrusted [`SeqAccess::size_hint`] / [`MapAccess::size_hint`]
/// values at 1 MiB worth of `T` elements so a crafted hint cannot trigger an OOM panic.
#[inline]
#[must_use]
pub const fn cautious_size_hint<T>(hint: Option<usize>) -> usize {
    match hint {
        Some(h) => {
            let elem_size = core::mem::size_of::<T>();
            let max_elems = 1024 * 1024 / if elem_size > 0 { elem_size } else { 1 };
            if h < max_elems { h } else { max_elems }
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Zero-allocation key matching `DeserializeSeed`s
// ---------------------------------------------------------------------------

/// `DeserializeSeed` for internally-tagged maps that matches the version tag key
/// without heap allocation, only buffering non-tag keys into [`Content`].
#[derive(Debug, Clone, Copy)]
pub struct InternalKey<'k>(pub &'k str);

impl<'k> InternalKey<'k> {
    #[must_use]
    pub const fn new(tag_field: &'k str) -> Self {
        Self(tag_field)
    }
}

/// Result of deserializing a map key with [`InternalKey`].
#[derive(Debug)]
pub enum InternalKeyResult<'de> {
    /// The key matched the configured tag field name.
    Tag,
    /// The key did not match the tag field name and was captured as [`Content`].
    Other(Content<'de>),
}

impl<'de> DeserializeSeed<'de> for InternalKey<'_> {
    type Value = InternalKeyResult<'de>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(InternalKeyVisitor {
            tag: self.0,
            _marker: PhantomData,
        })
    }
}

struct InternalKeyVisitor<'k, 'de> {
    tag: &'k str,
    _marker: PhantomData<Content<'de>>,
}

impl<'de> Visitor<'de> for InternalKeyVisitor<'_, 'de> {
    type Value = InternalKeyResult<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map key")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::Bool(v)))
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::I8(v)))
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::I16(v)))
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::I32(v)))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::I64(v)))
    }

    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::I128(Box::new(v))))
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::U8(v)))
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::U16(v)))
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::U32(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::U64(v)))
    }

    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::U128(Box::new(v))))
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::F32(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::F64(v)))
    }

    fn visit_char<E>(self, v: char) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::Char(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v == self.tag {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::String(Box::from(v))))
        }
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        if v == self.tag {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::Str(v)))
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        if v == self.tag {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::String(v.into_boxed_str())))
        }
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v == self.tag.as_bytes() {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::ByteBuf(Box::from(v))))
        }
    }

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        if v == self.tag.as_bytes() {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::Bytes(v)))
        }
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        if v.as_slice() == self.tag.as_bytes() {
            Ok(InternalKeyResult::Tag)
        } else {
            Ok(InternalKeyResult::Other(Content::ByteBuf(v.into_boxed_slice())))
        }
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::None))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| InternalKeyResult::Other(Content::Some(Box::new(v))))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(InternalKeyResult::Other(Content::Unit))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
            .map(|v| InternalKeyResult::Other(Content::Newtype(Box::new(v))))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(cautious_size_hint::<Content<'de>>(seq.size_hint()));
        while let Some(elem) = seq.next_element()? {
            vec.push(elem);
        }
        Ok(InternalKeyResult::Other(Content::Seq(vec.into_boxed_slice())))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut vec = Vec::with_capacity(cautious_size_hint::<(Content<'de>, Content<'de>)>(
            map.size_hint(),
        ));
        while let Some(kv) = map.next_entry()? {
            vec.push(kv);
        }
        Ok(InternalKeyResult::Other(Content::Map(vec.into_boxed_slice())))
    }
}

/// `DeserializeSeed` for adjacently-tagged maps that matches the tag and content
/// field names without heap allocation.
///
/// When `capture_other` is `false`, unrecognized keys are discarded with zero
/// allocation (draining nested containers via [`IgnoredAny`]). When `true`,
/// unrecognized keys are captured into [`Content`] so an untagged (`_`) fallback
/// can replay them.
#[derive(Debug, Clone, Copy)]
pub struct AdjacentKey<'k> {
    pub tag: &'k str,
    pub content: &'k str,
    pub capture_other: bool,
}

impl<'k> AdjacentKey<'k> {
    #[must_use]
    pub const fn new(tag: &'k str, content: &'k str, capture_other: bool) -> Self {
        Self {
            tag,
            content,
            capture_other,
        }
    }
}

/// Result of deserializing a map key with [`AdjacentKey`].
#[derive(Debug)]
pub enum AdjacentKeyResult<'de> {
    /// The key matched the tag field name.
    Tag,
    /// The key matched the content field name.
    Content,
    /// The key matched neither field and `capture_other` was `true`.
    Other(Content<'de>),
    /// The key matched neither field and `capture_other` was `false`.
    Ignored,
}

impl<'de> DeserializeSeed<'de> for AdjacentKey<'_> {
    type Value = AdjacentKeyResult<'de>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(AdjacentKeyVisitor {
            tag: self.tag,
            content: self.content,
            capture_other: self.capture_other,
            _marker: PhantomData,
        })
    }
}

struct AdjacentKeyVisitor<'k, 'de> {
    tag: &'k str,
    content: &'k str,
    capture_other: bool,
    _marker: PhantomData<Content<'de>>,
}

impl<'de> Visitor<'de> for AdjacentKeyVisitor<'_, 'de> {
    type Value = AdjacentKeyResult<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map key")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::Bool(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::I8(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::I16(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::I32(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::I64(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::I128(Box::new(v)))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::U8(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::U16(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::U32(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::U64(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::U128(Box::new(v)))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::F32(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::F64(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_char<E>(self, v: char) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::Char(v))
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v == self.tag {
            Ok(AdjacentKeyResult::Tag)
        } else if v == self.content {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::String(Box::from(v))))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        if v == self.tag {
            Ok(AdjacentKeyResult::Tag)
        } else if v == self.content {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::Str(v)))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        if v == self.tag {
            Ok(AdjacentKeyResult::Tag)
        } else if v == self.content {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::String(v.into_boxed_str())))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v == self.tag.as_bytes() {
            Ok(AdjacentKeyResult::Tag)
        } else if v == self.content.as_bytes() {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::ByteBuf(Box::from(v))))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        if v == self.tag.as_bytes() {
            Ok(AdjacentKeyResult::Tag)
        } else if v == self.content.as_bytes() {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::Bytes(v)))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        if v.as_slice() == self.tag.as_bytes() {
            Ok(AdjacentKeyResult::Tag)
        } else if v.as_slice() == self.content.as_bytes() {
            Ok(AdjacentKeyResult::Content)
        } else if self.capture_other {
            Ok(AdjacentKeyResult::Other(Content::ByteBuf(v.into_boxed_slice())))
        } else {
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::None)
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.capture_other {
            Deserialize::deserialize(deserializer)
                .map(|v| AdjacentKeyResult::Other(Content::Some(Box::new(v))))
        } else {
            let _ = IgnoredAny::deserialize(deserializer)?;
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(if self.capture_other {
            AdjacentKeyResult::Other(Content::Unit)
        } else {
            AdjacentKeyResult::Ignored
        })
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.capture_other {
            Deserialize::deserialize(deserializer)
                .map(|v| AdjacentKeyResult::Other(Content::Newtype(Box::new(v))))
        } else {
            let _ = IgnoredAny::deserialize(deserializer)?;
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.capture_other {
            let mut vec = Vec::with_capacity(cautious_size_hint::<Content<'de>>(seq.size_hint()));
            while let Some(elem) = seq.next_element()? {
                vec.push(elem);
            }
            Ok(AdjacentKeyResult::Other(Content::Seq(vec.into_boxed_slice())))
        } else {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.capture_other {
            let mut vec = Vec::with_capacity(cautious_size_hint::<(Content<'de>, Content<'de>)>(
                map.size_hint(),
            ));
            while let Some(kv) = map.next_entry()? {
                vec.push(kv);
            }
            Ok(AdjacentKeyResult::Other(Content::Map(vec.into_boxed_slice())))
        } else {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(AdjacentKeyResult::Ignored)
        }
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        if self.capture_other {
            Err(de::Error::invalid_type(Unexpected::Enum, &self))
        } else {
            let _ = Visitor::visit_enum(IgnoredAny, data)?;
            Ok(AdjacentKeyResult::Ignored)
        }
    }
}

/// Dedicated single-key `DeserializeSeed` used by [`read_adjacent_content`] so
/// an empty-string key `""` in the input map never falsely matches a dummy tag.
struct ContentKey<'k>(&'k str);

impl<'de> DeserializeSeed<'de> for ContentKey<'_> {
    type Value = bool;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ContentKeyVisitor(self.0))
    }
}

struct ContentKeyVisitor<'k>(&'k str);

impl<'de> Visitor<'de> for ContentKeyVisitor<'_> {
    type Value = bool;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map key")
    }

    fn visit_bool<E>(self, _v: bool) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_i8<E>(self, _v: i8) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_i16<E>(self, _v: i16) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_i32<E>(self, _v: i32) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_i64<E>(self, _v: i64) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_i128<E>(self, _v: i128) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_u8<E>(self, _v: u8) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_u16<E>(self, _v: u16) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_u32<E>(self, _v: u32) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_u64<E>(self, _v: u64) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_u128<E>(self, _v: u128) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_f32<E>(self, _v: f32) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_f64<E>(self, _v: f64) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_char<E>(self, _v: char) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(v == self.0)
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(v == self.0)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(v == self.0)
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> {
        Ok(v == self.0.as_bytes())
    }

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        Ok(v == self.0.as_bytes())
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(v.as_slice() == self.0.as_bytes())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = IgnoredAny::deserialize(deserializer)?;
        Ok(false)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(false)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = IgnoredAny::deserialize(deserializer)?;
        Ok(false)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(false)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(false)
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        let _ = Visitor::visit_enum(IgnoredAny, data)?;
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Internal-tagging map deserializers
// ---------------------------------------------------------------------------

/// Wraps any [`MapAccess`] as a streaming [`Deserializer`].
///
/// Allows structs, maps, unit structs, newtypes, options, and externally-tagged
/// enums to be deserialized directly from the remaining entries of an
/// in-progress map.
pub struct RemainingMapDeserializer<'de, M> {
    map: M,
    _marker: PhantomData<&'de ()>,
}

impl<M> RemainingMapDeserializer<'_, M> {
    #[inline]
    #[must_use]
    pub const fn new(map: M) -> Self {
        Self {
            map,
            _marker: PhantomData,
        }
    }
}

impl<'de, M> Deserializer<'de> for RemainingMapDeserializer<'de, M>
where
    M: MapAccess<'de>,
{
    type Error = M::Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(FlatMapAccess(self.map))
    }

    #[inline]
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    #[inline]
    fn deserialize_unit<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_type(Unexpected::Map, &visitor));
        }
        visitor.visit_unit()
    }

    #[inline]
    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(FlatMapAccess(self.map))
    }

    #[inline]
    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(FlatMapAccess(self.map))
    }

    #[inline]
    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_enum(MapEnumAccess {
            map: FlatMapAccess(self.map),
            _marker: PhantomData,
        })
    }

    #[inline]
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Map, &visitor))
    }

    #[inline]
    fn deserialize_ignored_any<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        drain_map(&mut self.map)?;
        visitor.visit_unit()
    }
}

/// Zero-allocation [`MapAccess`] wrapper that routes key `deserialize_identifier`
/// calls through `deserialize_any`, matching Serde's `FlatMapDeserializer` so
/// formats like RON accept quoted map keys produced by `#[serde(flatten)]`.
struct FlatMapAccess<M>(M);

impl<'de, M> MapAccess<'de> for FlatMapAccess<M>
where
    M: MapAccess<'de>,
{
    type Error = M::Error;

    #[inline]
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        self.0.next_key_seed(FlatKeySeed(seed))
    }

    #[inline]
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        self.0.next_value_seed(seed)
    }

    #[inline]
    fn next_entry_seed<K, V>(
        &mut self,
        kseed: K,
        vseed: V,
    ) -> Result<Option<(K::Value, V::Value)>, Self::Error>
    where
        K: DeserializeSeed<'de>,
        V: DeserializeSeed<'de>,
    {
        self.0.next_entry_seed(FlatKeySeed(kseed), vseed)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct FlatKeySeed<K>(K);

impl<'de, K> DeserializeSeed<'de> for FlatKeySeed<K>
where
    K: DeserializeSeed<'de>,
{
    type Value = K::Value;

    #[inline]
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.0.deserialize(FlatKeyDeserializer(deserializer))
    }
}

struct FlatKeyDeserializer<D>(D);

impl<'de, D> Deserializer<'de> for FlatKeyDeserializer<D>
where
    D: Deserializer<'de>,
{
    type Error = D::Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_any(visitor)
    }

    #[inline]
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_bool(visitor)
    }

    #[inline]
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i8(visitor)
    }

    #[inline]
    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i16(visitor)
    }

    #[inline]
    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i32(visitor)
    }

    #[inline]
    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i64(visitor)
    }

    #[inline]
    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i128(visitor)
    }

    #[inline]
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u8(visitor)
    }

    #[inline]
    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u16(visitor)
    }

    #[inline]
    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u32(visitor)
    }

    #[inline]
    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u64(visitor)
    }

    #[inline]
    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u128(visitor)
    }

    #[inline]
    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_f32(visitor)
    }

    #[inline]
    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_f64(visitor)
    }

    #[inline]
    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_char(visitor)
    }

    #[inline]
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_string(visitor)
    }

    #[inline]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_bytes(visitor)
    }

    #[inline]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_byte_buf(visitor)
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_option(visitor)
    }

    #[inline]
    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_unit_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_unit_struct(name, visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_newtype_struct(name, visitor)
    }

    #[inline]
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_tuple(len, visitor)
    }

    #[inline]
    fn deserialize_tuple_struct<V>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_tuple_struct(name, len, visitor)
    }

    #[inline]
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_map(visitor)
    }

    #[inline]
    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_struct(name, fields, visitor)
    }

    #[inline]
    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_enum(name, variants, visitor)
    }

    #[inline]
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_any(visitor)
    }

    #[inline]
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_ignored_any(visitor)
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

struct MapEnumAccess<'de, M> {
    map: M,
    _marker: PhantomData<&'de ()>,
}

impl<'de, M> EnumAccess<'de> for MapEnumAccess<'de, M>
where
    M: MapAccess<'de>,
{
    type Error = M::Error;
    type Variant = MapVariantAccess<'de, M>;

    fn variant_seed<V>(mut self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        match self.map.next_key_seed(seed)? {
            Some(variant) => Ok((
                variant,
                MapVariantAccess {
                    map: self.map,
                    _marker: PhantomData,
                },
            )),
            None => Err(de::Error::invalid_value(
                Unexpected::Map,
                &"map with a single key",
            )),
        }
    }
}

struct MapVariantAccess<'de, M> {
    map: M,
    _marker: PhantomData<&'de ()>,
}

impl<'de, M> VariantAccess<'de> for MapVariantAccess<'de, M>
where
    M: MapAccess<'de>,
{
    type Error = M::Error;

    fn unit_variant(mut self) -> Result<(), Self::Error> {
        self.map.next_value::<()>()?;
        if self.map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_value(
                Unexpected::Map,
                &"map with a single key",
            ));
        }
        Ok(())
    }

    fn newtype_variant_seed<T>(mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let val = self.map.next_value_seed(seed)?;
        if self.map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_value(
                Unexpected::Map,
                &"map with a single key",
            ));
        }
        Ok(val)
    }

    fn tuple_variant<V>(mut self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        struct TupleSeed<V> {
            len: usize,
            visitor: V,
        }

        impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for TupleSeed<V> {
            type Value = V::Value;

            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_tuple(self.len, self.visitor)
            }
        }

        let val = self.map.next_value_seed(TupleSeed { len, visitor })?;
        if self.map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_value(
                Unexpected::Map,
                &"map with a single key",
            ));
        }
        Ok(val)
    }

    fn struct_variant<V>(
        mut self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        struct StructSeed<V> {
            fields: &'static [&'static str],
            visitor: V,
        }

        impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for StructSeed<V> {
            type Value = V::Value;

            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_struct("", self.fields, self.visitor)
            }
        }

        let val = self.map.next_value_seed(StructSeed { fields, visitor })?;
        if self.map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_value(
                Unexpected::Map,
                &"map with a single key",
            ));
        }
        Ok(val)
    }
}

/// [`MapAccess`] that first replays buffered `(key, value)` entries from a
/// prefix `Vec` and then streams all remaining entries directly from `rest`
/// without further buffering.
pub struct ChainedMapAccess<'de, M> {
    prefix: VecIntoIter<(Content<'de>, Content<'de>)>,
    pending_value: Option<Content<'de>>,
    rest: M,
}

impl<'de, M> ChainedMapAccess<'de, M> {
    #[inline]
    #[must_use]
    pub fn new(prefix: Vec<(Content<'de>, Content<'de>)>, rest: M) -> Self {
        Self {
            prefix: prefix.into_iter(),
            pending_value: None,
            rest,
        }
    }
}

impl<'de, M> MapAccess<'de> for ChainedMapAccess<'de, M>
where
    M: MapAccess<'de>,
{
    type Error = M::Error;

    #[inline]
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if let Some((key, value)) = self.prefix.next() {
            self.pending_value = Some(value);
            seed.deserialize(ContentDeserializer::new(key)).map(Some)
        } else {
            self.rest.next_key_seed(seed)
        }
    }

    #[inline]
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        if let Some(value) = self.pending_value.take() {
            seed.deserialize(ContentDeserializer::new(value))
        } else {
            self.rest.next_value_seed(seed)
        }
    }

    #[inline]
    fn next_entry_seed<K, V>(
        &mut self,
        kseed: K,
        vseed: V,
    ) -> Result<Option<(K::Value, V::Value)>, Self::Error>
    where
        K: DeserializeSeed<'de>,
        V: DeserializeSeed<'de>,
    {
        if let Some((key, value)) = self.prefix.next() {
            let k = kseed.deserialize(ContentDeserializer::new(key))?;
            let v = vseed.deserialize(ContentDeserializer::new(value))?;
            Ok(Some((k, v)))
        } else {
            self.rest.next_entry_seed(kseed, vseed)
        }
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        let prefix_len = self.prefix.len();
        self.rest
            .size_hint()
            .and_then(|rest_len| prefix_len.checked_add(rest_len))
    }
}

// ---------------------------------------------------------------------------
// Adjacent-tagging helpers
// ---------------------------------------------------------------------------

/// Consumes all remaining `(key, value)` entries in `map` via [`IgnoredAny`] so
/// streaming parsers (such as `serde_json`) reach the closing delimiter cleanly.
#[inline]
pub fn drain_map<'de, M: MapAccess<'de>>(map: &mut M) -> Result<(), M::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

/// Scans the remaining entries of `map` for `content_field`, deserializes its
/// value directly into `T`, and drains any trailing entries before returning.
pub fn read_adjacent_content<'de, M, T>(map: &mut M, content_field: &'static str) -> Result<T, M::Error>
where
    M: MapAccess<'de>,
    T: Deserialize<'de>,
{
    while let Some(is_content) = map.next_key_seed(ContentKey(content_field))? {
        if is_content {
            let val = map.next_value::<T>()?;
            drain_map(map)?;
            return Ok(val);
        }
        let _ = map.next_value::<IgnoredAny>()?;
    }
    Err(de::Error::missing_field(content_field))
}

/// Zero-allocation wrapper that forwards `deserialize_option` to `visit_some`
/// and `deserialize_newtype_struct` to `visit_newtype_struct`.
///
/// Wraps streaming deserializers such as `SeqAccessDeserializer` or
/// `EnumAccessDeserializer` so single-untagged `Option<Vec<T>>`,
/// `Option<MyEnum>`, and newtype wrappers work on self-describing formats like
/// JSON without buffering.
pub struct OptionForwardingDeserializer<D>(pub D);

impl<D> OptionForwardingDeserializer<D> {
    #[inline]
    #[must_use]
    pub const fn new(inner: D) -> Self {
        Self(inner)
    }
}

impl<'de, D> Deserializer<'de> for OptionForwardingDeserializer<D>
where
    D: Deserializer<'de>,
{
    type Error = D::Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_any(visitor)
    }

    #[inline]
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_bool(visitor)
    }

    #[inline]
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i8(visitor)
    }

    #[inline]
    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i16(visitor)
    }

    #[inline]
    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i32(visitor)
    }

    #[inline]
    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i64(visitor)
    }

    #[inline]
    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_i128(visitor)
    }

    #[inline]
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u8(visitor)
    }

    #[inline]
    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u16(visitor)
    }

    #[inline]
    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u32(visitor)
    }

    #[inline]
    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u64(visitor)
    }

    #[inline]
    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_u128(visitor)
    }

    #[inline]
    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_f32(visitor)
    }

    #[inline]
    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_f64(visitor)
    }

    #[inline]
    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_char(visitor)
    }

    #[inline]
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_string(visitor)
    }

    #[inline]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_bytes(visitor)
    }

    #[inline]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_byte_buf(visitor)
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    #[inline]
    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_unit_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_unit_struct(name, visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_tuple(len, visitor)
    }

    #[inline]
    fn deserialize_tuple_struct<V>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_tuple_struct(name, len, visitor)
    }

    #[inline]
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_map(visitor)
    }

    #[inline]
    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_struct(name, fields, visitor)
    }

    #[inline]
    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_enum(name, variants, visitor)
    }

    #[inline]
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_identifier(visitor)
    }

    #[inline]
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.0.deserialize_ignored_any(visitor)
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

// ---------------------------------------------------------------------------
// Shared untagged visitor dispatchers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::borrow::ToOwned as _;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString as _;
    use alloc::vec;
    use serde::Deserialize;
    use serde::de::value::{BorrowedBytesDeserializer, BytesDeserializer, Error as ValueError};

    // -- InternalKey tests --------------------------------------------------

    #[test]
    fn test_internal_key_matches_and_captures_strings() {
        let mut de = serde_json::Deserializer::from_str(r#""version""#);
        let res = InternalKey::new("version").deserialize(&mut de).unwrap();
        assert!(matches!(res, InternalKeyResult::Tag));

        let mut de = serde_json::Deserializer::from_str(r#""name""#);
        let res = InternalKey::new("version").deserialize(&mut de).unwrap();
        assert!(matches!(res, InternalKeyResult::Other(Content::Str("name"))));

        // Escaped string produces an owned String in serde_json.
        let mut de = serde_json::Deserializer::from_str(r#""ver\u0073ion""#);
        let res = InternalKey::new("version").deserialize(&mut de).unwrap();
        assert!(matches!(res, InternalKeyResult::Tag));

        let mut de = serde_json::Deserializer::from_str(r#""na\nme""#);
        let res = InternalKey::new("version").deserialize(&mut de).unwrap();
        assert!(matches!(
            res,
            InternalKeyResult::Other(Content::String(ref s)) if &**s == "na\nme"
        ));
    }

    #[test]
    fn test_internal_key_matches_bytes_and_primitives() {
        let de = BorrowedBytesDeserializer::<ValueError>::new(b"version");
        let res = InternalKey::new("version").deserialize(de).unwrap();
        assert!(matches!(res, InternalKeyResult::Tag));

        let de = BorrowedBytesDeserializer::<ValueError>::new(b"other");
        let res = InternalKey::new("version").deserialize(de).unwrap();
        assert!(matches!(res, InternalKeyResult::Other(Content::Bytes(b"other"))));

        let de = BytesDeserializer::<ValueError>::new(b"version");
        let res = InternalKey::new("version").deserialize(de).unwrap();
        assert!(matches!(res, InternalKeyResult::Tag));

        let de = BytesDeserializer::<ValueError>::new(b"other");
        let res = InternalKey::new("version").deserialize(de).unwrap();
        assert!(matches!(
            res,
            InternalKeyResult::Other(Content::ByteBuf(ref b)) if &**b == b"other"
        ));

        let res = InternalKey::new("version")
            .deserialize(ContentDeserializer::<ValueError>::new(Content::U64(42)))
            .unwrap();
        assert!(matches!(res, InternalKeyResult::Other(Content::U64(42))));

        let res = InternalKey::new("version")
            .deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true)))
            .unwrap();
        assert!(matches!(res, InternalKeyResult::Other(Content::Bool(true))));

        let res = InternalKey::new("version")
            .deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
                vec![Content::U8(1)].into_boxed_slice(),
            )))
            .unwrap();
        assert!(matches!(
            res,
            InternalKeyResult::Other(Content::Seq(ref v)) if v.len() == 1
        ));
    }

    // -- AdjacentKey tests --------------------------------------------------

    #[test]
    fn test_adjacent_key_without_capture() {
        let key = AdjacentKey::new("version", "data", false);

        let mut de = serde_json::Deserializer::from_str(r#""version""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Tag
        ));

        let mut de = serde_json::Deserializer::from_str(r#""data""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Content
        ));

        let mut de = serde_json::Deserializer::from_str(r#""extra""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Ignored
        ));

        // Nested container key is drained and ignored without error.
        let res = key
            .deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
                vec![Content::U64(1), Content::Str("a")].into_boxed_slice(),
            )))
            .unwrap();
        assert!(matches!(res, AdjacentKeyResult::Ignored));

        let res = key
            .deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
                vec![(Content::Str("k"), Content::U64(2))].into_boxed_slice(),
            )))
            .unwrap();
        assert!(matches!(res, AdjacentKeyResult::Ignored));
    }

    #[test]
    fn test_adjacent_key_with_capture() {
        let key = AdjacentKey::new("version", "data", true);

        let mut de = serde_json::Deserializer::from_str(r#""version""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Tag
        ));

        let mut de = serde_json::Deserializer::from_str(r#""data""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Content
        ));

        let mut de = serde_json::Deserializer::from_str(r#""extra""#);
        assert!(matches!(
            key.deserialize(&mut de).unwrap(),
            AdjacentKeyResult::Other(Content::Str("extra"))
        ));

        let de = BorrowedBytesDeserializer::<ValueError>::new(b"data");
        assert!(matches!(key.deserialize(de).unwrap(), AdjacentKeyResult::Content));

        let res = key
            .deserialize(ContentDeserializer::<ValueError>::new(Content::I32(-7)))
            .unwrap();
        assert!(matches!(res, AdjacentKeyResult::Other(Content::I32(-7))));
    }

    // -- RemainingMapDeserializer tests -------------------------------------

    /// Helper that strips a leading `"version"` key from a JSON object and
    /// deserializes `T` from the remaining map entries using `RemainingMapDeserializer`.
    fn deserialize_after_tag<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, serde_json::Error> {
        struct Helper<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for Helper<T> {
            type Value = T;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map starting with version")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let first = map.next_key_seed(InternalKey::new("version"))?;
                assert!(matches!(first, Some(InternalKeyResult::Tag)));
                let _version: u64 = map.next_value()?;
                T::deserialize(RemainingMapDeserializer::new(map))
            }
        }

        let mut de = serde_json::Deserializer::from_str(json);
        let val = de.deserialize_map(Helper::<T>(PhantomData))?;
        de.end()?;
        Ok(val)
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct SimpleStruct {
        x: i32,
        y: String,
    }

    #[test]
    fn test_remaining_map_deserializer_struct() {
        let s: SimpleStruct = deserialize_after_tag(r#"{"version": 1, "x": 10, "y": "hello"}"#).unwrap();
        assert_eq!(
            s,
            SimpleStruct {
                x: 10,
                y: "hello".to_owned(),
            }
        );
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct UnitStruct;

    #[test]
    fn test_remaining_map_deserializer_unit_struct() {
        let u: UnitStruct = deserialize_after_tag(r#"{"version": 1}"#).unwrap();
        assert_eq!(u, UnitStruct);

        let err = deserialize_after_tag::<UnitStruct>(r#"{"version": 1, "extra": 2}"#).unwrap_err();
        assert!(err.to_string().contains("invalid type"));
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct NewtypeStruct(SimpleStruct);

    #[test]
    fn test_remaining_map_deserializer_newtype_and_option() {
        let nt: NewtypeStruct = deserialize_after_tag(r#"{"version": 1, "x": 3, "y": "ok"}"#).unwrap();
        assert_eq!(
            nt,
            NewtypeStruct(SimpleStruct {
                x: 3,
                y: "ok".to_owned(),
            })
        );

        let opt: Option<SimpleStruct> =
            deserialize_after_tag(r#"{"version": 1, "x": 4, "y": "some"}"#).unwrap();
        assert_eq!(
            opt,
            Some(SimpleStruct {
                x: 4,
                y: "some".to_owned(),
            })
        );
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct FlattenedOuter {
        a: i32,
        #[serde(flatten)]
        rest: BTreeMap<String, i32>,
    }

    #[test]
    fn test_remaining_map_deserializer_flatten() {
        let v: FlattenedOuter = deserialize_after_tag(r#"{"version": 1, "a": 1, "b": 2, "c": 3}"#).unwrap();
        assert_eq!(v.a, 1);
        assert_eq!(v.rest.get("b"), Some(&2));
        assert_eq!(v.rest.get("c"), Some(&3));
        assert_eq!(v.rest.len(), 2);
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    enum ExternalEnum {
        Unit,
        Newtype(i32),
        Tuple(i32, String),
        Struct { flag: bool },
    }

    #[test]
    fn test_remaining_map_deserializer_enum() {
        let u: ExternalEnum = deserialize_after_tag(r#"{"version": 1, "Unit": null}"#).unwrap();
        assert_eq!(u, ExternalEnum::Unit);

        let n: ExternalEnum = deserialize_after_tag(r#"{"version": 1, "Newtype": 42}"#).unwrap();
        assert_eq!(n, ExternalEnum::Newtype(42));

        let t: ExternalEnum = deserialize_after_tag(r#"{"version": 1, "Tuple": [7, "hi"]}"#).unwrap();
        assert_eq!(t, ExternalEnum::Tuple(7, "hi".to_owned()));

        let s: ExternalEnum = deserialize_after_tag(r#"{"version": 1, "Struct": {"flag": true}}"#).unwrap();
        assert_eq!(s, ExternalEnum::Struct { flag: true });

        // Empty remaining map is rejected.
        let empty_err = deserialize_after_tag::<ExternalEnum>(r#"{"version": 1}"#).unwrap_err();
        assert!(empty_err.to_string().contains("single key"));

        // Map with >1 remaining entries is rejected.
        let multi_err = deserialize_after_tag::<ExternalEnum>(r#"{"version": 1, "Newtype": 42, "extra": 1}"#)
            .unwrap_err();
        assert!(multi_err.to_string().contains("single key"));
    }

    #[test]
    fn test_remaining_map_deserializer_ignored_any_and_primitives() {
        let _: IgnoredAny =
            deserialize_after_tag(r#"{"version": 1, "a": [1, 2, 3], "b": {"c": true}}"#).unwrap();

        let err = deserialize_after_tag::<i32>(r#"{"version": 1, "x": 1}"#).unwrap_err();
        assert!(err.to_string().contains("invalid type"));
    }

    // -- ChainedMapAccess tests ---------------------------------------------

    #[test]
    fn test_chained_map_access_replays_prefix_then_streams_rest() {
        struct OutOfOrderHelper;

        impl<'de> Visitor<'de> for OutOfOrderHelper {
            type Value = (u64, SimpleStruct);

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map containing version anywhere")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut prefix = Vec::new();
                while let Some(key) = map.next_key_seed(InternalKey::new("version"))? {
                    match key {
                        InternalKeyResult::Tag => {
                            let ver: u64 = map.next_value()?;
                            let chained = ChainedMapAccess::new(prefix, map);
                            let val = SimpleStruct::deserialize(RemainingMapDeserializer::new(chained))?;
                            return Ok((ver, val));
                        }
                        InternalKeyResult::Other(k) => {
                            let v: Content<'de> = map.next_value()?;
                            prefix.push((k, v));
                        }
                    }
                }
                Err(de::Error::missing_field("version"))
            }
        }

        // Tag appears in the middle: "x" is buffered in prefix, "y" streams directly from JSON.
        let json = r#"{"x": 99, "version": 2, "y": "streamed"}"#;
        let mut de = serde_json::Deserializer::from_str(json);
        let (ver, val) = de.deserialize_map(OutOfOrderHelper).unwrap();
        de.end().unwrap();

        assert_eq!(ver, 2);
        assert_eq!(
            val,
            SimpleStruct {
                x: 99,
                y: "streamed".to_owned(),
            }
        );
    }

    #[test]
    fn test_chained_map_access_size_hint() {
        struct FixedMap(usize);

        impl<'de> MapAccess<'de> for FixedMap {
            type Error = ValueError;

            fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, Self::Error>
            where
                K: DeserializeSeed<'de>,
            {
                Ok(None)
            }

            fn next_value_seed<V>(&mut self, _seed: V) -> Result<V::Value, Self::Error>
            where
                V: DeserializeSeed<'de>,
            {
                Err(de::Error::custom("empty"))
            }

            fn size_hint(&self) -> Option<usize> {
                Some(self.0)
            }
        }

        let prefix = vec![
            (Content::Str("a"), Content::U64(1)),
            (Content::Str("b"), Content::U64(2)),
        ];
        let mut chained = ChainedMapAccess::new(prefix, FixedMap(3));
        assert_eq!(chained.size_hint(), Some(5));

        let entry: Option<(String, u64)> = chained.next_entry().unwrap();
        assert_eq!(entry, Some(("a".to_owned(), 1)));
        assert_eq!(chained.size_hint(), Some(4));
    }

    // -- drain_map & read_adjacent_content tests ----------------------------

    #[test]
    fn test_drain_map_consumes_trailing_entries() {
        struct DrainHelper;

        impl<'de> Visitor<'de> for DrainHelper {
            type Value = u64;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let _k: String = map.next_key()?.unwrap();
                let v: u64 = map.next_value()?;
                drain_map(&mut map)?;
                Ok(v)
            }
        }

        let json = r#"{"first": 7, "second": [1, 2, 3], "third": {"nested": true}}"#;
        let mut de = serde_json::Deserializer::from_str(json);
        let first = de.deserialize_map(DrainHelper).unwrap();
        de.end().unwrap();
        assert_eq!(first, 7);
    }

    #[test]
    fn test_read_adjacent_content_streams_and_drains() {
        struct AdjHelper;

        impl<'de> Visitor<'de> for AdjHelper {
            type Value = (u64, SimpleStruct);

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an adjacently-tagged map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let key = map.next_key_seed(AdjacentKey::new("version", "data", false))?;
                assert!(matches!(key, Some(AdjacentKeyResult::Tag)));
                let ver: u64 = map.next_value()?;
                let val: SimpleStruct = read_adjacent_content(&mut map, "data")?;
                Ok((ver, val))
            }
        }

        // Includes an empty-string key "" before "data" and a trailing key after "data".
        let json = r#"{"version": 3, "": "empty-key", "data": {"x": 5, "y": "adj"}, "trailing": [1, 2]}"#;
        let mut de = serde_json::Deserializer::from_str(json);
        let (ver, val) = de.deserialize_map(AdjHelper).unwrap();
        de.end().unwrap();

        assert_eq!(ver, 3);
        assert_eq!(
            val,
            SimpleStruct {
                x: 5,
                y: "adj".to_owned(),
            }
        );

        // Missing content field returns a missing_field error.
        let missing_json = r#"{"version": 3, "other": 1}"#;
        let mut de = serde_json::Deserializer::from_str(missing_json);
        let err = de.deserialize_map(AdjHelper).unwrap_err();
        assert!(err.to_string().contains("missing field `data`"));
    }

    #[test]
    fn test_option_forwarding_deserializer_nested_option_and_newtype() {
        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct NewtypeVec(Vec<u32>);

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct NewtypeOptVec(Option<Vec<u32>>);

        let seq_de = serde::de::value::SeqDeserializer::<_, ValueError>::new([10_u32, 20, 30].into_iter());
        let opt_nt = Option::<NewtypeVec>::deserialize(OptionForwardingDeserializer::new(seq_de)).unwrap();
        assert_eq!(opt_nt, Some(NewtypeVec(vec![10, 20, 30])));

        let seq_de2 = serde::de::value::SeqDeserializer::<_, ValueError>::new([40_u32, 50].into_iter());
        let nt_opt = NewtypeOptVec::deserialize(OptionForwardingDeserializer::new(seq_de2)).unwrap();
        assert_eq!(nt_opt, NewtypeOptVec(Some(vec![40, 50])));
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct BorrowedPair<'a> {
        first: &'a str,
        second: &'a str,
    }

    fn assert_str_borrowed_from(needle: &str, haystack: &str) {
        let h_start = haystack.as_ptr() as usize;
        let h_end = h_start + haystack.len();
        let n_start = needle.as_ptr() as usize;
        let n_end = n_start + needle.len();
        assert!(
            n_start >= h_start && n_end <= h_end,
            "expected {needle:?} to borrow zero-copy from input buffer {haystack:?}"
        );
    }

    fn deserialize_after_leading_tag<'de, T: Deserialize<'de>>(
        json: &'de str,
    ) -> Result<T, serde_json::Error> {
        struct AfterTagVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for AfterTagVisitor<T> {
            type Value = T;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map starting with version")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let first = map.next_key_seed(InternalKey::new("version"))?;
                assert!(
                    matches!(first, Some(InternalKeyResult::Tag)),
                    "expected leading version tag"
                );
                let _ver: u64 = map.next_value()?;
                T::deserialize(RemainingMapDeserializer::new(map))
            }
        }

        let mut de = serde_json::Deserializer::from_str(json);
        let val = de.deserialize_map(AfterTagVisitor::<T>(PhantomData))?;
        de.end()?;
        Ok(val)
    }

    fn deserialize_with_chained_tag<'de, T: Deserialize<'de>>(
        json: &'de str,
    ) -> Result<(u64, T), serde_json::Error> {
        struct ChainedTagVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ChainedTagVisitor<T> {
            type Value = (u64, T);

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map containing version anywhere")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut prefix = Vec::new();
                while let Some(key) = map.next_key_seed(InternalKey::new("version"))? {
                    match key {
                        InternalKeyResult::Tag => {
                            let ver: u64 = map.next_value()?;
                            let chained = ChainedMapAccess::new(prefix, map);
                            let val = T::deserialize(RemainingMapDeserializer::new(chained))?;
                            return Ok((ver, val));
                        }
                        InternalKeyResult::Other(k) => {
                            let v: Content<'de> = map.next_value()?;
                            prefix.push((k, v));
                        }
                    }
                }
                Err(de::Error::missing_field("version"))
            }
        }

        let mut de = serde_json::Deserializer::from_str(json);
        let val = de.deserialize_map(ChainedTagVisitor::<T>(PhantomData))?;
        de.end()?;
        Ok(val)
    }

    #[test]
    fn test_zero_copy_borrowed_str_through_remaining_map_deserializer() {
        let input = String::from(r#"{"version": 1, "first": "zero_copy_a", "second": "zero_copy_b"}"#);
        let pair: BorrowedPair<'_> = deserialize_after_leading_tag(&input).unwrap();
        assert_eq!(
            pair,
            BorrowedPair {
                first: "zero_copy_a",
                second: "zero_copy_b",
            }
        );
        assert_str_borrowed_from(pair.first, &input);
        assert_str_borrowed_from(pair.second, &input);
    }

    #[test]
    fn test_zero_copy_borrowed_str_and_prefix_modes_through_chained_map_access() {
        // Non-empty prefix + non-empty remaining stream
        let input_mid = String::from(r#"{"first": "from_prefix", "version": 2, "second": "from_stream"}"#);
        let (ver, pair): (u64, BorrowedPair<'_>) = deserialize_with_chained_tag(&input_mid).unwrap();
        assert_eq!(ver, 2);
        assert_eq!(
            pair,
            BorrowedPair {
                first: "from_prefix",
                second: "from_stream",
            }
        );
        assert_str_borrowed_from(pair.first, &input_mid);
        assert_str_borrowed_from(pair.second, &input_mid);

        // Empty prefix + non-empty remaining stream
        let input_first = String::from(r#"{"version": 1, "first": "stream_1", "second": "stream_2"}"#);
        let (ver, pair): (u64, BorrowedPair<'_>) = deserialize_with_chained_tag(&input_first).unwrap();
        assert_eq!(ver, 1);
        assert_eq!(
            pair,
            BorrowedPair {
                first: "stream_1",
                second: "stream_2",
            }
        );
        assert_str_borrowed_from(pair.first, &input_first);
        assert_str_borrowed_from(pair.second, &input_first);

        // Non-empty prefix + empty remaining stream
        let input_last = String::from(r#"{"first": "prefix_1", "second": "prefix_2", "version": 3}"#);
        let (ver, pair): (u64, BorrowedPair<'_>) = deserialize_with_chained_tag(&input_last).unwrap();
        assert_eq!(ver, 3);
        assert_eq!(
            pair,
            BorrowedPair {
                first: "prefix_1",
                second: "prefix_2",
            }
        );
        assert_str_borrowed_from(pair.first, &input_last);
        assert_str_borrowed_from(pair.second, &input_last);

        // Empty prefix + empty remaining stream
        let input_empty = String::from(r#"{"version": 4}"#);
        let (ver, unit): (u64, UnitStruct) = deserialize_with_chained_tag(&input_empty).unwrap();
        assert_eq!(ver, 4);
        assert_eq!(unit, UnitStruct);

        let (ver, empty_map): (u64, BTreeMap<&str, &str>) =
            deserialize_with_chained_tag(&input_empty).unwrap();
        assert_eq!(ver, 4);
        assert!(empty_map.is_empty());

        // BTreeMap exercises `next_entry_seed` on both empty prefix and non-empty prefix
        let (ver, map_non_empty_prefix): (u64, BTreeMap<&str, &str>) =
            deserialize_with_chained_tag(&input_mid).unwrap();
        assert_eq!(ver, 2);
        assert_eq!(map_non_empty_prefix.len(), 2);
        for (k, v) in &map_non_empty_prefix {
            assert_str_borrowed_from(k, &input_mid);
            assert_str_borrowed_from(v, &input_mid);
        }

        let (ver, map_empty_prefix): (u64, BTreeMap<&str, &str>) =
            deserialize_with_chained_tag(&input_first).unwrap();
        assert_eq!(ver, 1);
        assert_eq!(map_empty_prefix.len(), 2);
        for (k, v) in &map_empty_prefix {
            assert_str_borrowed_from(k, &input_first);
            assert_str_borrowed_from(v, &input_first);
        }
    }

    #[test]
    fn test_remaining_map_deserializer_ignored_any_drains_cleanly_in_stream() {
        struct IgnoreAfterTag;

        impl<'de> Deserialize<'de> for IgnoreAfterTag {
            fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = IgnoreAfterTag;
                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("a versioned map")
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        let mut prefix = Vec::new();
                        while let Some(key) = map.next_key_seed(InternalKey::new("version"))? {
                            match key {
                                InternalKeyResult::Tag => {
                                    let _ver: u64 = map.next_value()?;
                                    if prefix.is_empty() {
                                        let _ = IgnoredAny::deserialize(RemainingMapDeserializer::new(map))?;
                                    } else {
                                        let chained = ChainedMapAccess::new(prefix, map);
                                        let _ =
                                            IgnoredAny::deserialize(RemainingMapDeserializer::new(chained))?;
                                    }
                                    return Ok(IgnoreAfterTag);
                                }
                                InternalKeyResult::Other(k) => {
                                    let v: Content<'de> = map.next_value()?;
                                    prefix.push((k, v));
                                }
                            }
                        }
                        Err(de::Error::missing_field("version"))
                    }
                }
                deserializer.deserialize_map(V)
            }
        }

        let json = r#"[
            {"version": 1, "a": [1, {"nested": [true, null]}], "b": "str"},
            {"version": 2},
            {"pre": {"k": [1, 2]}, "version": 3, "post": "val"},
            42
        ]"#;

        let mut de = serde_json::Deserializer::from_str(json);
        let trailing: u64 = {
            struct ArrVisitor;
            impl<'de> Visitor<'de> for ArrVisitor {
                type Value = u64;
                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("array of 3 maps and a u64")
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let _ = seq.next_element::<IgnoreAfterTag>()?.unwrap();
                    let _ = seq.next_element::<IgnoreAfterTag>()?.unwrap();
                    let _ = seq.next_element::<IgnoreAfterTag>()?.unwrap();
                    let tail: u64 = seq.next_element()?.unwrap();
                    assert!(seq.next_element::<IgnoredAny>()?.is_none());
                    Ok(tail)
                }
            }
            de.deserialize_seq(ArrVisitor).unwrap()
        };
        de.end().unwrap();
        assert_eq!(trailing, 42);
    }

    #[test]
    fn test_read_adjacent_content_empty_key_trailing_keys_and_zero_copy() {
        struct AdjItem<'de> {
            version: u64,
            payload: BorrowedPair<'de>,
        }

        impl<'de> Deserialize<'de> for AdjItem<'de> {
            fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = AdjItem<'de>;
                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("adjacently-tagged map")
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        let first = map.next_key_seed(AdjacentKey::new("version", "content", false))?;
                        assert!(matches!(first, Some(AdjacentKeyResult::Tag)));
                        let version: u64 = map.next_value()?;
                        let payload: BorrowedPair<'de> = read_adjacent_content(&mut map, "content")?;
                        Ok(AdjItem { version, payload })
                    }
                }
                deserializer.deserialize_map(V)
            }
        }

        let input = String::from(
            r#"[
                {
                    "version": 1,
                    "": {"ignored": [1, 2]},
                    "other": true,
                    "content": {"first": "adj_a", "second": "adj_b"},
                    "trail1": [3, 4],
                    "trail2": "end"
                },
                {
                    "version": 2,
                    "content": {"first": "only", "second": "one"}
                },
                123
            ]"#,
        );

        let mut de = serde_json::Deserializer::from_str(&input);
        let (item1, item2, tail): (AdjItem<'_>, AdjItem<'_>, u64) = {
            struct SeqV;
            impl<'de> Visitor<'de> for SeqV {
                type Value = (AdjItem<'de>, AdjItem<'de>, u64);
                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("array of two AdjItems and a u64")
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let a = seq.next_element::<AdjItem<'de>>()?.unwrap();
                    let b = seq.next_element::<AdjItem<'de>>()?.unwrap();
                    let c = seq.next_element::<u64>()?.unwrap();
                    assert!(seq.next_element::<IgnoredAny>()?.is_none());
                    Ok((a, b, c))
                }
            }
            de.deserialize_seq(SeqV).unwrap()
        };
        de.end().unwrap();

        assert_eq!(item1.version, 1);
        assert_eq!(
            item1.payload,
            BorrowedPair {
                first: "adj_a",
                second: "adj_b",
            }
        );
        assert_str_borrowed_from(item1.payload.first, &input);
        assert_str_borrowed_from(item1.payload.second, &input);

        assert_eq!(item2.version, 2);
        assert_eq!(
            item2.payload,
            BorrowedPair {
                first: "only",
                second: "one",
            }
        );
        assert_str_borrowed_from(item2.payload.first, &input);
        assert_str_borrowed_from(item2.payload.second, &input);
        assert_eq!(tail, 123);

        let missing_input = r#"{"version": 1, "": "not-content"}"#;
        let err = serde_json::from_str::<AdjItem<'_>>(missing_input).err().unwrap();
        assert!(err.to_string().contains("missing field `content`"));
    }

    #[test]
    fn test_chained_map_access_size_hint_none_and_overflow() {
        struct HintMap(Option<usize>);

        impl<'de> MapAccess<'de> for HintMap {
            type Error = ValueError;

            fn next_key_seed<K: de::DeserializeSeed<'de>>(
                &mut self,
                _seed: K,
            ) -> Result<Option<K::Value>, Self::Error> {
                Ok(None)
            }

            fn next_value_seed<V: de::DeserializeSeed<'de>>(
                &mut self,
                _seed: V,
            ) -> Result<V::Value, Self::Error> {
                Err(de::Error::custom("empty"))
            }

            fn size_hint(&self) -> Option<usize> {
                self.0
            }
        }

        let prefix = vec![(Content::Str("a"), Content::U8(1))];
        let chained_none = ChainedMapAccess::new(prefix, HintMap(None));
        assert_eq!(chained_none.size_hint(), None);

        let prefix = vec![(Content::Str("a"), Content::U8(1))];
        let chained_overflow = ChainedMapAccess::new(prefix, HintMap(Some(usize::MAX)));
        assert_eq!(chained_overflow.size_hint(), None);

        let chained_empty_prefix = ChainedMapAccess::new(Vec::new(), HintMap(Some(7)));
        assert_eq!(chained_empty_prefix.size_hint(), Some(7));
    }

    #[test]
    fn test_adjacent_key_enum_and_wrapped_draining() {
        struct EnumOnlyDe;

        impl<'de> de::Deserializer<'de> for EnumOnlyDe {
            type Error = ValueError;

            fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
                visitor.visit_enum(serde::de::value::MapAccessDeserializer::new(
                    serde::de::value::MapDeserializer::<_, ValueError>::new(
                        [(
                            "Var",
                            ContentDeserializer::<ValueError>::new(Content::Map(
                                vec![(Content::Str("k"), Content::U8(1))].into_boxed_slice(),
                            )),
                        )]
                        .into_iter(),
                    ),
                ))
            }

            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
                bytes byte_buf option unit unit_struct newtype_struct seq tuple
                tuple_struct map struct enum identifier ignored_any
            }
        }

        let ignore_key = AdjacentKey::new("version", "content", false);
        assert!(matches!(
            ignore_key.deserialize(EnumOnlyDe).unwrap(),
            AdjacentKeyResult::Ignored
        ));

        let capture_key = AdjacentKey::new("version", "content", true);
        let _ = capture_key.deserialize(EnumOnlyDe).unwrap_err();

        let some_content = Content::Some(Box::new(Content::Seq(
            vec![Content::U8(1), Content::U8(2)].into_boxed_slice(),
        )));
        assert!(matches!(
            ignore_key
                .deserialize(ContentDeserializer::<ValueError>::new(some_content))
                .unwrap(),
            AdjacentKeyResult::Ignored
        ));

        let newtype_content = Content::Newtype(Box::new(Content::Map(
            vec![(Content::Str("x"), Content::U8(9))].into_boxed_slice(),
        )));
        assert!(matches!(
            ignore_key
                .deserialize(ContentDeserializer::<ValueError>::new(newtype_content))
                .unwrap(),
            AdjacentKeyResult::Ignored
        ));
    }

    #[test]
    fn test_cautious_size_hint() {
        assert_eq!(cautious_size_hint::<u8>(None), 0);
        assert_eq!(cautious_size_hint::<u8>(Some(100)), 100);
        assert_eq!(cautious_size_hint::<u64>(Some(usize::MAX)), 1024 * 1024 / 8);
        assert_eq!(cautious_size_hint::<()>(Some(usize::MAX)), 1024 * 1024);
    }
}
