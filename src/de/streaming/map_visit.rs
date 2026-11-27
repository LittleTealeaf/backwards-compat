use super::adjacent::drain_map;
use super::keys::{AdjacentKey, AdjacentKeyResult, InternalKey, InternalKeyResult};
use super::map_de::{ChainedMapAccess, RemainingMapDeserializer};
use crate::de::content::{Content, ContentDeserializer};
use crate::de::tag::{IntTagOrContent, StrTag, StrTagOrContent};
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Unexpected, Visitor};

/// Strategy trait abstracting integer (`u64`) vs string (`&str`) version tags
/// for shared map-scanning helpers.
pub trait TagMode<'de> {
    /// Borrowed tag argument passed to dispatch functions (`u64` or `&str`).
    type TagArg<'a>;
    /// Deserialized tag value (`u64` or [`StrTag`]).
    type OwnedTag: Deserialize<'de>;
    /// Tag-or-content value used when untagged fallback is enabled.
    type TagOrContent: Deserialize<'de>;

    /// Borrows a [`Self::TagArg`] from an owned tag.
    fn as_arg(tag: &Self::OwnedTag) -> Self::TagArg<'_>;

    /// Extracts a valid tag or falls back to buffered [`Content`] on type mismatch.
    fn into_tag_or_other(val: Self::TagOrContent) -> Result<Self::OwnedTag, Content<'de>>;
}

/// [`TagMode`] for native integer (`u64`) version tags.
pub struct IntTagMode;

impl<'de> TagMode<'de> for IntTagMode {
    type TagArg<'a> = u64;
    type OwnedTag = u64;
    type TagOrContent = IntTagOrContent<'de>;

    #[inline]
    fn as_arg(tag: &Self::OwnedTag) -> Self::TagArg<'_> {
        *tag
    }

    #[inline]
    fn into_tag_or_other(val: Self::TagOrContent) -> Result<Self::OwnedTag, Content<'de>> {
        match val {
            IntTagOrContent::Tag(t) => Ok(t),
            IntTagOrContent::Other(c) => Err(c),
        }
    }
}

/// [`TagMode`] for native string (`&str`) version tags.
pub struct StrTagMode;

impl<'de> TagMode<'de> for StrTagMode {
    type TagArg<'a> = &'a str;
    type OwnedTag = StrTag<'de>;
    type TagOrContent = StrTagOrContent<'de>;

    #[inline]
    fn as_arg(tag: &Self::OwnedTag) -> Self::TagArg<'_> {
        tag.as_str()
    }

    #[inline]
    fn into_tag_or_other(val: Self::TagOrContent) -> Result<Self::OwnedTag, Content<'de>> {
        match val {
            StrTagOrContent::Tag(t) => Ok(t),
            StrTagOrContent::Other(c) => Err(c),
        }
    }
}

/// Dispatch target for tagged deserialization from a `Deserializer`.
pub trait InternalMapDispatch<'de, TM: TagMode<'de>> {
    /// Final domain (or [`crate::Migrated`]) output type.
    type Value;

    /// Dispatches deserialization for a matched version `tag`.
    fn dispatch_tagged<D>(tag: TM::TagArg<'_>, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>;
}

/// Dispatch target for adjacently-tagged streaming deserialization when the tag
/// appears before the content field in a map.
pub trait AdjacentMapDispatch<'de, TM: TagMode<'de>>: InternalMapDispatch<'de, TM> {
    /// Streams the remaining map entries directly into the versioned content type.
    fn dispatch_adjacent_streaming<A>(tag: TM::TagArg<'_>, map: &mut A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>;
}

/// Scans an internally-tagged map (without untagged fallback), buffering only
/// prefix entries seen before `tag_field` and streaming the rest.
#[inline]
pub fn visit_internal_map<'de, TM, H, A>(mut map: A, tag_field: &'static str) -> Result<H::Value, A::Error>
where
    TM: TagMode<'de>,
    H: InternalMapDispatch<'de, TM>,
    A: MapAccess<'de>,
{
    let mut prefix: Vec<(Content<'de>, Content<'de>)> = Vec::new();
    while let Some(key) = map.next_key_seed(InternalKey::new(tag_field))? {
        match key {
            InternalKeyResult::Tag => {
                let tag = map.next_value::<TM::OwnedTag>()?;
                return H::dispatch_tagged(
                    TM::as_arg(&tag),
                    RemainingMapDeserializer::new(ChainedMapAccess::new(prefix, map)),
                );
            }
            InternalKeyResult::Other(k) => {
                let v = map.next_value::<Content<'de>>()?;
                prefix.push((k, v));
            }
        }
    }
    Err(de::Error::missing_field(tag_field))
}

/// Scans an internally-tagged map with untagged fallback, invoking `fallback`
/// with the buffered [`Content::Map`] if no valid version tag is encountered.
#[inline]
pub fn visit_internal_map_untagged<'de, TM, H, A, F>(
    mut map: A,
    tag_field: &'static str,
    fallback: F,
) -> Result<H::Value, A::Error>
where
    TM: TagMode<'de>,
    H: InternalMapDispatch<'de, TM>,
    A: MapAccess<'de>,
    F: FnOnce(Content<'de>) -> Result<H::Value, A::Error>,
{
    let mut prefix: Vec<(Content<'de>, Content<'de>)> = Vec::new();
    while let Some(key) = map.next_key_seed(InternalKey::new(tag_field))? {
        match key {
            InternalKeyResult::Tag => match TM::into_tag_or_other(map.next_value::<TM::TagOrContent>()?) {
                Ok(tag) => {
                    return H::dispatch_tagged(
                        TM::as_arg(&tag),
                        RemainingMapDeserializer::new(ChainedMapAccess::new(prefix, map)),
                    );
                }
                Err(other) => {
                    prefix.push((Content::Str(tag_field), other));
                }
            },
            InternalKeyResult::Other(k) => {
                let v = map.next_value::<Content<'de>>()?;
                prefix.push((k, v));
            }
        }
    }
    fallback(Content::Map(prefix.into_boxed_slice()))
}

/// Scans an adjacently-tagged map (without untagged fallback), streaming
/// content directly when `tag_field` precedes `content_field`.
#[inline]
pub fn visit_adjacent_map<'de, TM, H, A>(
    mut map: A,
    tag_field: &'static str,
    content_field: &'static str,
) -> Result<H::Value, A::Error>
where
    TM: TagMode<'de>,
    H: AdjacentMapDispatch<'de, TM>,
    A: MapAccess<'de>,
{
    let mut buffered_content: Option<Content<'de>> = None;
    while let Some(key) = map.next_key_seed(AdjacentKey::new(tag_field, content_field, false))? {
        match key {
            AdjacentKeyResult::Tag => {
                let tag = map.next_value::<TM::OwnedTag>()?;
                if let Some(content) = buffered_content {
                    let res = H::dispatch_tagged(TM::as_arg(&tag), ContentDeserializer::new(content))?;
                    drain_map(&mut map)?;
                    return Ok(res);
                }
                return H::dispatch_adjacent_streaming(TM::as_arg(&tag), &mut map);
            }
            AdjacentKeyResult::Content => {
                if buffered_content.is_some() {
                    return Err(de::Error::duplicate_field(content_field));
                }
                buffered_content = Some(map.next_value::<Content<'de>>()?);
            }
            AdjacentKeyResult::Ignored | AdjacentKeyResult::Other(_) => {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }
    }
    Err(de::Error::missing_field(tag_field))
}

/// Scans an adjacently-tagged map with untagged fallback, invoking `fallback`
/// with the buffered [`Content::Map`] if no valid version tag is encountered.
#[inline]
pub fn visit_adjacent_map_untagged<'de, TM, H, A, F>(
    mut map: A,
    tag_field: &'static str,
    content_field: &'static str,
    fallback: F,
) -> Result<H::Value, A::Error>
where
    TM: TagMode<'de>,
    H: AdjacentMapDispatch<'de, TM>,
    A: MapAccess<'de>,
    F: FnOnce(Content<'de>) -> Result<H::Value, A::Error>,
{
    let mut prefix: Vec<(Content<'de>, Content<'de>)> = Vec::new();
    let mut buffered_content_idx: Option<usize> = None;

    while let Some(key) = map.next_key_seed(AdjacentKey::new(tag_field, content_field, true))? {
        match key {
            AdjacentKeyResult::Tag => match TM::into_tag_or_other(map.next_value::<TM::TagOrContent>()?) {
                Ok(tag) => {
                    if let Some(idx) = buffered_content_idx {
                        let (_, content) = prefix.swap_remove(idx);
                        let res = H::dispatch_tagged(TM::as_arg(&tag), ContentDeserializer::new(content))?;
                        drain_map(&mut map)?;
                        return Ok(res);
                    }
                    return H::dispatch_adjacent_streaming(TM::as_arg(&tag), &mut map);
                }
                Err(other) => {
                    prefix.push((Content::Str(tag_field), other));
                }
            },
            AdjacentKeyResult::Content => {
                let v = map.next_value::<Content<'de>>()?;
                if buffered_content_idx.is_none() {
                    buffered_content_idx = Some(prefix.len());
                }
                prefix.push((Content::Str(content_field), v));
            }
            AdjacentKeyResult::Other(k) => {
                let v = map.next_value::<Content<'de>>()?;
                prefix.push((k, v));
            }
            AdjacentKeyResult::Ignored => {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }
    }

    fallback(Content::Map(prefix.into_boxed_slice()))
}

// ---------------------------------------------------------------------------
// Shared external-tagging variant identifier deserializer
// ---------------------------------------------------------------------------

/// Dispatch trait for externally-tagged version variant identifier enums.
pub trait ExtField: Sized {
    /// Resolves a positional `u64` variant index.
    fn from_u64<E: de::Error>(v: u64, expected: &dyn de::Expected) -> Result<Self, E>;

    /// Resolves a string variant identifier.
    fn from_str<E: de::Error>(v: &str) -> Result<Self, E>;
}

/// Wrapper implementing [`Deserialize`] for any [`ExtField`] via a shared
/// [`Visitor`] (handling `expecting`, `visit_u64`, `visit_str`, and `visit_bytes`).
pub struct ExtFieldDeserialize<F>(pub F);

struct ExtFieldVisitor<F>(PhantomData<F>);

impl<F: ExtField> Visitor<'_> for ExtFieldVisitor<F> {
    type Value = F;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("variant identifier")
    }

    #[inline]
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        F::from_u64(v, &self)
    }

    #[inline]
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        F::from_str(v)
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        core::str::from_utf8(v).map_or_else(
            |_| Err(de::Error::invalid_value(Unexpected::Bytes(v), &self)),
            F::from_str,
        )
    }
}

impl<'de, F: ExtField> Deserialize<'de> for ExtFieldDeserialize<F> {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer
            .deserialize_identifier(ExtFieldVisitor(PhantomData))
            .map(Self)
    }
}
