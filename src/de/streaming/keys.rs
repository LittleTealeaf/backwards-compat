use super::cautious_size_hint;
use crate::de::content::Content;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::de::{
    self, Deserialize, DeserializeSeed, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess,
    Unexpected, Visitor,
};

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
pub(super) struct ContentKey<'k>(pub(super) &'k str);

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
