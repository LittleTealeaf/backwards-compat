use super::content::Content;
use alloc::borrow::ToOwned as _;
use alloc::format;
use alloc::string::String;
use core::fmt;
use core::marker::PhantomData;
use serde::de::{self, Deserialize, Deserializer, Visitor};

/// Zero-copy string version tag helper that deserializes via `deserialize_str`
/// (avoiding `deserialize_any` and avoiding `Cow<'de, str>`'s `deserialize_string`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrTag<'de> {
    /// Borrowed directly from the input buffer without allocation.
    Borrowed(&'de str),
    /// Owned string when the deserializer had to unescape or allocate.
    Owned(String),
}

impl StrTag<'_> {
    /// Returns the string slice for this tag.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(s) => s.as_str(),
        }
    }
}

impl<'de> Deserialize<'de> for StrTag<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(StrTagVisitor(PhantomData))
    }
}

struct StrTagVisitor<'de>(PhantomData<StrTag<'de>>);

impl<'de> Visitor<'de> for StrTagVisitor<'de> {
    type Value = StrTag<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string version tag")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(StrTag::Borrowed(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrTag::Owned(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(StrTag::Owned(v))
    }
}

/// Result of reading a version tag field on an integer-tagged type that accepts untagged (`_`) fallback.
#[derive(Debug)]
pub enum IntTagOrContent<'de> {
    /// Valid non-negative integer version tag.
    Tag(u64),
    /// Mismatched value type buffered as `Content` for untagged (`_`) fallback.
    Other(Content<'de>),
}

impl<'de> Deserialize<'de> for IntTagOrContent<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Content::deserialize(deserializer)? {
            Content::U8(n) => Ok(Self::Tag(u64::from(n))),
            Content::U16(n) => Ok(Self::Tag(u64::from(n))),
            Content::U32(n) => Ok(Self::Tag(u64::from(n))),
            Content::U64(n) => Ok(Self::Tag(n)),
            Content::U128(n) => {
                Ok(u64::try_from(*n).map_or_else(|_| Self::Other(Content::U128(n)), Self::Tag))
            }
            Content::I8(n) => Ok(u64::try_from(n).map_or(Self::Other(Content::I8(n)), Self::Tag)),
            Content::I16(n) => Ok(u64::try_from(n).map_or(Self::Other(Content::I16(n)), Self::Tag)),
            Content::I32(n) => Ok(u64::try_from(n).map_or(Self::Other(Content::I32(n)), Self::Tag)),
            Content::I64(n) => Ok(u64::try_from(n).map_or(Self::Other(Content::I64(n)), Self::Tag)),
            Content::I128(n) => {
                Ok(u64::try_from(*n).map_or_else(|_| Self::Other(Content::I128(n)), Self::Tag))
            }
            other => Ok(Self::Other(other)),
        }
    }
}

/// Result of reading a version tag field on a string-tagged type that accepts untagged (`_`) fallback.
#[derive(Debug)]
pub enum StrTagOrContent<'de> {
    /// Valid string version tag (borrowed zero-copy when possible).
    Tag(StrTag<'de>),
    /// Mismatched value type buffered as `Content` for untagged (`_`) fallback.
    Other(Content<'de>),
}

impl<'de> Deserialize<'de> for StrTagOrContent<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Content::deserialize(deserializer)? {
            Content::Str(s) => Ok(Self::Tag(StrTag::Borrowed(s))),
            Content::String(s) => Ok(Self::Tag(StrTag::Owned(String::from(s)))),
            other => Ok(Self::Other(other)),
        }
    }
}

/// Formats a cold-path error for an unrecognized integer version tag.
#[cold]
#[must_use]
pub fn unknown_int_version_error<E: de::Error>(
    found: u64,
    current_int: u64,
    supported: &[&str],
    has_disabled_versions: bool,
) -> E {
    let list = supported.join(", ");
    if found > current_int {
        return de::Error::custom(format!(
            "version {found} is newer than supported version {current_int} (supported versions: {list})"
        ));
    }

    let gated_hint = if has_disabled_versions {
        "; some versions may be disabled by feature flags"
    } else {
        ""
    };
    de::Error::custom(format!(
        "unknown version {found} (supported versions: {list}{gated_hint})"
    ))
}

/// Formats a cold-path error for an unrecognized string version tag.
#[cold]
#[must_use]
pub fn unknown_str_version_error<E: de::Error>(
    found: &str,
    supported: &[&str],
    has_disabled_versions: bool,
) -> E {
    let list = supported.join(", ");
    let gated_hint = if has_disabled_versions {
        "; some versions may be disabled by feature flags"
    } else {
        ""
    };
    de::Error::custom(format!(
        "unknown version \"{found}\" (supported versions: {list}{gated_hint})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::de::content::{Content, ContentDeserializer};
    use alloc::boxed::Box;
    use alloc::string::ToString as _;
    use serde::de::value::Error as ValueError;

    #[test]
    fn test_str_tag_borrowed_owned_and_as_str() {
        let borrowed =
            StrTag::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("v1"))).unwrap();
        assert_eq!(borrowed, StrTag::Borrowed("v1"));
        assert_eq!(borrowed.as_str(), "v1");

        let owned = StrTag::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
            Box::from("v2"),
        )))
        .unwrap();
        assert_eq!(owned, StrTag::Owned("v2".to_owned()));
        assert_eq!(owned.as_str(), "v2");

        let via_visit_str = StrTagVisitor(PhantomData).visit_str::<ValueError>("v3").unwrap();
        assert_eq!(via_visit_str, StrTag::Owned("v3".to_owned()));
        assert_eq!(via_visit_str.as_str(), "v3");
    }

    #[test]
    fn test_str_tag_json_borrowed_escaped_and_non_string_error() {
        let raw_borrowed = String::from("\"v1.2\"");
        let tag_borrowed: StrTag<'_> = serde_json::from_str(&raw_borrowed).unwrap();
        assert_eq!(tag_borrowed, StrTag::Borrowed("v1.2"));

        let raw_escaped = String::from("\"v1\\n2\"");
        let tag_escaped: StrTag<'_> = serde_json::from_str(&raw_escaped).unwrap();
        assert_eq!(tag_escaped, StrTag::Owned("v1\n2".to_owned()));

        let err = serde_json::from_str::<StrTag<'_>>("42").unwrap_err();
        assert!(err.to_string().contains("expected a string version tag"));
    }

    #[test]
    fn test_unknown_int_and_str_version_errors() {
        let err_future: ValueError = unknown_int_version_error(9, 7, &["5", "6", "7"], false);
        assert_eq!(
            err_future.to_string(),
            "version 9 is newer than supported version 7 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int: ValueError = unknown_int_version_error(3, 7, &["5", "6", "7"], false);
        assert_eq!(
            err_unknown_int.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int_disabled: ValueError = unknown_int_version_error(3, 7, &["5", "6", "7"], true);
        assert_eq!(
            err_unknown_int_disabled.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7; some versions may be disabled by feature flags)"
        );

        let err_unknown_str: ValueError = unknown_str_version_error("v0", &["\"v1\"", "\"v2\""], false);
        assert_eq!(
            err_unknown_str.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\")"
        );

        let err_unknown_str_disabled: ValueError =
            unknown_str_version_error("v0", &["\"v1\"", "\"v2\""], true);
        assert_eq!(
            err_unknown_str_disabled.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\"; some versions may be disabled by feature flags)"
        );
    }

    #[test]
    fn test_int_tag_or_content_variants() {
        for (content, expected) in [
            (Content::U8(1), 1_u64),
            (Content::U16(2), 2_u64),
            (Content::U32(3), 3_u64),
            (Content::U64(4), 4_u64),
            (Content::I8(5), 5_u64),
            (Content::I16(6), 6_u64),
            (Content::I32(7), 7_u64),
            (Content::I64(8), 8_u64),
            (Content::U128(Box::new(9)), 9_u64),
            (Content::I128(Box::new(10)), 10_u64),
        ] {
            let parsed =
                IntTagOrContent::deserialize(ContentDeserializer::<ValueError>::new(content)).unwrap();
            assert!(matches!(parsed, IntTagOrContent::Tag(v) if v == expected));
        }

        for mismatched in [
            Content::I8(-1),
            Content::I64(-42),
            Content::U128(Box::new(u128::from(u64::MAX) + 1)),
            Content::I128(Box::new(-1)),
            Content::Str("v1"),
            Content::String(Box::from("v2")),
            Content::Bool(true),
            Content::None,
        ] {
            let parsed =
                IntTagOrContent::deserialize(ContentDeserializer::<ValueError>::new(mismatched)).unwrap();
            assert!(matches!(parsed, IntTagOrContent::Other(_)));
        }

        let json_str: IntTagOrContent<'_> = serde_json::from_str("\"v1\"").unwrap();
        assert!(matches!(json_str, IntTagOrContent::Other(Content::Str("v1"))));
    }

    #[test]
    fn test_str_tag_or_content_variants() {
        let borrowed =
            StrTagOrContent::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("v1"))).unwrap();
        assert!(matches!(borrowed, StrTagOrContent::Tag(StrTag::Borrowed("v1"))));

        let owned = StrTagOrContent::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
            Box::from("v2"),
        )))
        .unwrap();
        assert!(matches!(owned, StrTagOrContent::Tag(StrTag::Owned(ref s)) if s == "v2"));

        let escaped = String::from("\"v1\\n2\"");
        let json_escaped: StrTagOrContent<'_> = serde_json::from_str(&escaped).unwrap();
        assert!(matches!(
            json_escaped,
            StrTagOrContent::Tag(StrTag::Owned(ref s)) if s == "v1\n2"
        ));

        for mismatched in [Content::U64(1), Content::I64(-1), Content::Bool(false)] {
            let parsed =
                StrTagOrContent::deserialize(ContentDeserializer::<ValueError>::new(mismatched)).unwrap();
            assert!(matches!(parsed, StrTagOrContent::Other(_)));
        }
    }
}
