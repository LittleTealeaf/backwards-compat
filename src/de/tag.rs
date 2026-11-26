use alloc::borrow::ToOwned as _;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::de::{
    self, Deserialize, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagValue<'de> {
    Int(u64),
    NegativeInt(i64),
    Str(&'de str),
    String(String),
    Other,
}

impl TagValue<'_> {
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match *self {
            Self::Int(n) => Some(n),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for TagValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::NegativeInt(n) => write!(f, "{n}"),
            Self::Str(s) => write!(f, "\"{s}\""),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Other => f.write_str("<non-version tag>"),
        }
    }
}

impl<'de> Deserialize<'de> for TagValue<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(TagValueVisitor(PhantomData))
    }
}

struct TagValueVisitor<'de>(PhantomData<TagValue<'de>>);

impl<'de> Visitor<'de> for TagValueVisitor<'de> {
    type Value = TagValue<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a version tag value")
    }

    fn visit_bool<E>(self, _v: bool) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or_else(|_| TagValue::NegativeInt(i64::from(v)), TagValue::Int))
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or_else(|_| TagValue::NegativeInt(i64::from(v)), TagValue::Int))
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or_else(|_| TagValue::NegativeInt(i64::from(v)), TagValue::Int))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or(TagValue::NegativeInt(v), TagValue::Int))
    }

    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or_else(
            |_| i64::try_from(v).map_or(TagValue::Other, TagValue::NegativeInt),
            TagValue::Int,
        ))
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E> {
        Ok(TagValue::Int(u64::from(v)))
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E> {
        Ok(TagValue::Int(u64::from(v)))
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E> {
        Ok(TagValue::Int(u64::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(TagValue::Int(v))
    }

    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).map_or(TagValue::Other, TagValue::Int))
    }

    fn visit_f32<E>(self, _v: f32) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_f64<E>(self, _v: f64) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_char<E>(self, _v: char) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(TagValue::String(v.to_owned()))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(TagValue::Str(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(TagValue::String(v))
    }

    fn visit_bytes<E>(self, _v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(TagValue::Other)
    }

    fn visit_borrowed_bytes<E>(self, _v: &'de [u8]) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_byte_buf<E>(self, _v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = IgnoredAny::deserialize(deserializer)?;
        Ok(TagValue::Other)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(TagValue::Other)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = IgnoredAny::deserialize(deserializer)?;
        Ok(TagValue::Other)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(TagValue::Other)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(TagValue::Other)
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        let _ = Visitor::visit_enum(IgnoredAny, data)?;
        Ok(TagValue::Other)
    }
}

#[cold]
#[must_use]
pub fn unknown_version_error<E: de::Error>(
    found: &TagValue<'_>,
    current_int: Option<u64>,
    supported: &[&str],
    has_disabled_versions: bool,
) -> E {
    if matches!(found, TagValue::Other) {
        return de::Error::invalid_type(
            Unexpected::Other("non-version tag"),
            &"an integer or string version tag",
        );
    }

    let list = supported.join(", ");
    if let TagValue::Int(n) = *found
        && let Some(max) = current_int
        && n > max
    {
        return de::Error::custom(format!(
            "version {n} is newer than supported version {max} (supported versions: {list})"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::de::content::{Content, ContentDeserializer};
    use alloc::boxed::Box;
    use alloc::string::ToString as _;
    use alloc::vec;
    use serde::de::value::{Error as ValueError, MapAccessDeserializer, MapDeserializer};

    #[test]
    fn test_tag_value_accessors_and_display() {
        let int_tag = TagValue::Int(5);
        assert_eq!(int_tag.as_u64(), Some(5));
        assert_eq!(int_tag.as_str(), None);
        assert_eq!(int_tag.to_string(), "5");

        let neg_tag = TagValue::NegativeInt(-3);
        assert_eq!(neg_tag.as_u64(), None);
        assert_eq!(neg_tag.as_str(), None);
        assert_eq!(neg_tag.to_string(), "-3");

        let str_tag = TagValue::Str("v1");
        assert_eq!(str_tag.as_u64(), None);
        assert_eq!(str_tag.as_str(), Some("v1"));
        assert_eq!(str_tag.to_string(), "\"v1\"");

        let string_tag = TagValue::String("v2".to_owned());
        assert_eq!(string_tag.as_u64(), None);
        assert_eq!(string_tag.as_str(), Some("v2"));
        assert_eq!(string_tag.to_string(), "\"v2\"");

        let other_tag = TagValue::Other;
        assert_eq!(other_tag.as_u64(), None);
        assert_eq!(other_tag.as_str(), None);
        assert_eq!(other_tag.to_string(), "<non-version tag>");
    }

    #[test]
    fn test_tag_value_direct_deserialize() {
        // Unsigned integers
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap(),
            TagValue::Int(1)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U16(2))).unwrap(),
            TagValue::Int(2)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U32(3))).unwrap(),
            TagValue::Int(3)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U64(u64::MAX))).unwrap(),
            TagValue::Int(u64::MAX)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(Box::new(
                u128::from(u64::MAX)
            ))))
            .unwrap(),
            TagValue::Int(u64::MAX)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(Box::new(
                u128::from(u64::MAX) + 1
            ))))
            .unwrap(),
            TagValue::Other
        );

        // Signed integers
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I8(7))).unwrap(),
            TagValue::Int(7)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I8(-5))).unwrap(),
            TagValue::NegativeInt(-5)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I16(0))).unwrap(),
            TagValue::Int(0)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I16(-10))).unwrap(),
            TagValue::NegativeInt(-10)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I32(42))).unwrap(),
            TagValue::Int(42)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I32(-42))).unwrap(),
            TagValue::NegativeInt(-42)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I64(99))).unwrap(),
            TagValue::Int(99)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I64(i64::MIN))).unwrap(),
            TagValue::NegativeInt(i64::MIN)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::from(u64::MAX)
            ))))
            .unwrap(),
            TagValue::Int(u64::MAX)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::from(u64::MAX) + 1
            ))))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::from(i64::MIN)
            ))))
            .unwrap(),
            TagValue::NegativeInt(i64::MIN)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::from(i64::MIN) - 1
            ))))
            .unwrap(),
            TagValue::Other
        );

        // Strings (borrowed and owned)
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("v1"))).unwrap(),
            TagValue::Str("v1")
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
                Box::from("v2")
            )))
            .unwrap(),
            TagValue::String("v2".to_owned())
        );
        assert_eq!(
            TagValueVisitor(PhantomData)
                .visit_str::<ValueError>("v3")
                .unwrap(),
            TagValue::String("v3".to_owned())
        );

        // Non-tag values (drained without allocating Content)
        for non_tag in [
            Content::Bool(true),
            Content::F32(1.5),
            Content::F64(2.5),
            Content::Char('x'),
            Content::Bytes(b"raw"),
            Content::ByteBuf(vec![1, 2].into_boxed_slice()),
            Content::None,
            Content::Some(Box::new(Content::U8(1))),
            Content::Unit,
            Content::Newtype(Box::new(Content::U8(1))),
            Content::Seq(vec![Content::U8(1), Content::Str("a")].into_boxed_slice()),
            Content::Map(vec![(Content::Str("k"), Content::U8(1))].into_boxed_slice()),
        ] {
            assert_eq!(
                TagValue::deserialize(ContentDeserializer::<ValueError>::new(non_tag)).unwrap(),
                TagValue::Other
            );
        }

        // Direct visit_bytes / visit_enum / JSON round-trip
        assert_eq!(
            TagValueVisitor(PhantomData)
                .visit_bytes::<ValueError>(b"abc")
                .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValueVisitor(PhantomData)
                .visit_enum(MapAccessDeserializer::new(MapDeserializer::<_, ValueError>::new(
                    [("V", 1u8)].into_iter()
                )))
                .unwrap(),
            TagValue::Other
        );
        let json_str: TagValue<'_> = serde_json::from_str("\"v1\"").unwrap();
        assert_eq!(json_str, TagValue::Str("v1"));
        let json_int: TagValue<'_> = serde_json::from_str("42").unwrap();
        assert_eq!(json_int, TagValue::Int(42));
        let json_obj: TagValue<'_> = serde_json::from_str("{\"nested\":[1,2,3]}").unwrap();
        assert_eq!(json_obj, TagValue::Other);
    }

    #[test]
    fn test_unknown_version_error() {
        let err_other: ValueError = unknown_version_error(&TagValue::Other, Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_other.to_string(),
            "invalid type: non-version tag, expected an integer or string version tag"
        );

        let err_future: ValueError =
            unknown_version_error(&TagValue::Int(9), Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_future.to_string(),
            "version 9 is newer than supported version 7 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int: ValueError =
            unknown_version_error(&TagValue::Int(3), Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_unknown_int.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int_disabled: ValueError =
            unknown_version_error(&TagValue::Int(3), Some(7), &["5", "6", "7"], true);
        assert_eq!(
            err_unknown_int_disabled.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7; some versions may be disabled by feature flags)"
        );

        let err_unknown_str: ValueError =
            unknown_version_error(&TagValue::Str("v0"), None, &["\"v1\"", "\"v2\""], false);
        assert_eq!(
            err_unknown_str.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\")"
        );

        let err_unknown_str_disabled: ValueError =
            unknown_version_error(&TagValue::Str("v0"), None, &["\"v1\"", "\"v2\""], true);
        assert_eq!(
            err_unknown_str_disabled.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\"; some versions may be disabled by feature flags)"
        );
    }

    #[test]
    fn test_tag_value_deserialize_numeric_boundaries_and_strings() {
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(Box::new(
                u128::MAX
            ))))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(Box::new(
                u128::from(u64::MAX)
            ))))
            .unwrap(),
            TagValue::Int(u64::MAX)
        );

        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::MIN
            ))))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                i128::MAX
            ))))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(0))))
                .unwrap(),
            TagValue::Int(0)
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
                -1
            ))))
            .unwrap(),
            TagValue::NegativeInt(-1)
        );

        let raw_borrowed = String::from("\"v1.2\"");
        let tag_borrowed: TagValue<'_> = serde_json::from_str(&raw_borrowed).unwrap();
        assert_eq!(tag_borrowed, TagValue::Str("v1.2"));

        let raw_escaped = String::from("\"v1\\n2\"");
        let tag_escaped: TagValue<'_> = serde_json::from_str(&raw_escaped).unwrap();
        assert_eq!(tag_escaped, TagValue::String("v1\n2".to_owned()));

        let tag_neg: TagValue<'_> = serde_json::from_str("-42").unwrap();
        assert_eq!(tag_neg, TagValue::NegativeInt(-42));
    }

    #[test]
    fn test_tag_value_deserialize_drains_non_tag_payloads_in_stream() {
        struct Envelope<'de> {
            v: TagValue<'de>,
            after: u32,
        }

        impl<'de> Deserialize<'de> for Envelope<'de> {
            fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct EnvelopeVisitor;
                impl<'de> Visitor<'de> for EnvelopeVisitor {
                    type Value = Envelope<'de>;
                    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        formatter.write_str("envelope map")
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        let mut v = None;
                        let mut after = None;
                        while let Some(key) = map.next_key::<&str>()? {
                            match key {
                                "v" => v = Some(map.next_value::<TagValue<'de>>()?),
                                "after" => after = Some(map.next_value::<u32>()?),
                                _ => {
                                    let _ = map.next_value::<de::IgnoredAny>()?;
                                }
                            }
                        }
                        Ok(Envelope {
                            v: v.ok_or_else(|| de::Error::missing_field("v"))?,
                            after: after.ok_or_else(|| de::Error::missing_field("after"))?,
                        })
                    }
                }
                deserializer.deserialize_map(EnvelopeVisitor)
            }
        }

        struct EnumOnlyDe;
        impl<'de> de::Deserializer<'de> for EnumOnlyDe {
            type Error = ValueError;
            fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
                visitor.visit_enum(MapAccessDeserializer::new(MapDeserializer::<_, ValueError>::new(
                    [(
                        "Var",
                        ContentDeserializer::<ValueError>::new(Content::Map(
                            vec![(Content::Str("a"), Content::U8(1))].into_boxed_slice(),
                        )),
                    )]
                    .into_iter(),
                )))
            }
            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
                bytes byte_buf option unit unit_struct newtype_struct seq tuple
                tuple_struct map struct enum identifier ignored_any
            }
        }

        for non_tag_json in [
            "true",
            "false",
            "3.14",
            "null",
            "[1, [2, 3], {\"k\": \"val\"}]",
            "{\"nested\": {\"arr\": [true, null, 1.5]}}",
        ] {
            let payload = format!("{{\"v\": {non_tag_json}, \"after\": 99}}");
            let env: Envelope<'_> = serde_json::from_str(&payload)
                .unwrap_or_else(|e| panic!("failed to drain {non_tag_json}: {e}"));
            assert_eq!(env.v, TagValue::Other);
            assert_eq!(env.after, 99);
        }

        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::Newtype(
                Box::new(Content::Seq(
                    vec![
                        Content::U8(1),
                        Content::Map(vec![(Content::Str("a"), Content::Bool(true))].into_boxed_slice()),
                    ]
                    .into_boxed_slice()
                ))
            )))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(
            TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::Some(Box::new(
                Content::Map(vec![(Content::Str("x"), Content::U32(7))].into_boxed_slice())
            ))))
            .unwrap(),
            TagValue::Other
        );
        assert_eq!(TagValue::deserialize(EnumOnlyDe).unwrap(), TagValue::Other);
    }
}
