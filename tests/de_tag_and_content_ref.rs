use backwards_compat::__private::{Content, ContentDeserializer, ContentRefDeserializer, TagValue};
use serde::Deserialize;
use serde::de::value::Error as ValueError;
use serde::de::{self, Deserializer as _, MapAccess, SeqAccess, Visitor};
use std::fmt;

#[test]
fn test_tag_value_deserialize_numeric_boundaries_and_strings() {
    // u128::MAX -> Other, u64::MAX -> Int
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(u128::MAX))).unwrap(),
        TagValue::Other
    );
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(u128::from(
            u64::MAX
        ))))
        .unwrap(),
        TagValue::Int(u64::MAX)
    );

    // i128::MIN / i128::MAX -> Other
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(i128::MIN))).unwrap(),
        TagValue::Other
    );
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(i128::MAX))).unwrap(),
        TagValue::Other
    );
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(0))).unwrap(),
        TagValue::Int(0)
    );
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(-1))).unwrap(),
        TagValue::NegativeInt(-1)
    );

    // JSON borrowed &'de str vs escaped owned String
    let raw_borrowed = String::from("\"v1.2\"");
    let tag_borrowed: TagValue<'_> = serde_json::from_str(&raw_borrowed).unwrap();
    assert_eq!(tag_borrowed, TagValue::Str("v1.2"));

    let raw_escaped = String::from("\"v1\\n2\"");
    let tag_escaped: TagValue<'_> = serde_json::from_str(&raw_escaped).unwrap();
    assert_eq!(tag_escaped, TagValue::String("v1\n2".to_owned()));

    // JSON negative integer
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
                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            visitor.visit_enum(serde::de::value::MapAccessDeserializer::new(
                serde::de::value::MapDeserializer::<_, ValueError>::new(
                    [(
                        "Var",
                        ContentDeserializer::<ValueError>::new(Content::Map(vec![(
                            Content::Str("a"),
                            Content::U8(1),
                        )])),
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

    for non_tag_json in [
        "true",
        "false",
        "3.14",
        "null",
        "[1, [2, 3], {\"k\": \"val\"}]",
        "{\"nested\": {\"arr\": [true, null, 1.5]}}",
    ] {
        let payload = format!("{{\"v\": {non_tag_json}, \"after\": 99}}");
        let env: Envelope<'_> =
            serde_json::from_str(&payload).unwrap_or_else(|e| panic!("failed to drain {non_tag_json}: {e}"));
        assert_eq!(env.v, TagValue::Other);
        assert_eq!(env.after, 99);
    }

    // Nested newtype, Some, and enum draining
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::Newtype(
            Box::new(Content::Seq(vec![
                Content::U8(1),
                Content::Map(vec![(Content::Str("a"), Content::Bool(true))]),
            ]))
        )))
        .unwrap(),
        TagValue::Other
    );
    assert_eq!(
        TagValue::deserialize(ContentDeserializer::<ValueError>::new(Content::Some(Box::new(
            Content::Map(vec![(Content::Str("x"), Content::U32(7))])
        ))))
        .unwrap(),
        TagValue::Other
    );
    assert_eq!(TagValue::deserialize(EnumOnlyDe).unwrap(), TagValue::Other);
}

#[test]
fn test_content_ref_deserializer_borrows_outlive_content_buffer() {
    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct FailFirst<'a> {
        missing: &'a str,
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct SucceedSecond<'a> {
        name: &'a str,
    }

    let input = String::from("{\"name\":\"zero-copy\"}");
    let decoded: SucceedSecond<'_>;
    {
        let buffered: Content<'_> = serde_json::from_str(&input).unwrap();
        let _ = FailFirst::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap_err();
        decoded = SucceedSecond::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap();
        // `buffered` is dropped here; `decoded` must still validly borrow from `input` ('de).
    }
    assert_eq!(decoded, SucceedSecond { name: "zero-copy" });

    // Same for borrowed &'de [u8]
    let raw_bytes: Vec<u8> = vec![10, 20, 30];
    let borrowed_slice: &[u8];
    {
        let buffered = Content::Bytes(&raw_bytes);
        borrowed_slice = <&[u8]>::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap();
    }
    assert_eq!(borrowed_slice, &[10, 20, 30]);
}

#[test]
fn test_content_ref_deserializer_edge_cases() {
    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct UnitS;

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    enum E {
        Unit,
        Newtype(u8),
    }

    struct SizeHintSeqVisitor;
    impl<'de> Visitor<'de> for SizeHintSeqVisitor {
        type Value = (Option<usize>, usize);
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("seq")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let hint = seq.size_hint();
            let mut count = 0;
            while seq.next_element::<u8>()?.is_some() {
                count += 1;
            }
            Ok((hint, count))
        }
    }

    struct SizeHintMapVisitor;
    impl<'de> Visitor<'de> for SizeHintMapVisitor {
        type Value = (Option<usize>, usize);
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("map")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let hint = map.size_hint();
            let mut count = 0;
            while map.next_entry::<String, u8>()?.is_some() {
                count += 1;
            }
            Ok((hint, count))
        }
    }

    // f32 int-to-float coercion and 128-bit rejection
    let val = f32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I32(-42))).unwrap();
    assert!((val - (-42.0_f32)).abs() < f32::EPSILON);
    let _ = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U128(1))).unwrap_err();
    let _ = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I128(-1))).unwrap_err();

    // Unit & unit_struct reject non-empty map/seq
    let _ = <()>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(vec![(
        Content::Str("k"),
        Content::U8(1),
    )])))
    .unwrap_err();
    let _ = UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(vec![(
        Content::Str("k"),
        Content::U8(1),
    )])))
    .unwrap_err();
    let _ = UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(vec![
        Content::U8(1),
    ])))
    .unwrap_err();

    // Enum unit variant as map with Unit or empty Map, and owned String variant name
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::String(
            "Unit".to_owned()
        )))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(vec![(
            Content::Str("Unit"),
            Content::Unit,
        )])))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(vec![(
            Content::Str("Unit"),
            Content::Map(vec![]),
        )])))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(vec![(
            Content::String("Newtype".to_owned()),
            Content::U8(7),
        )])))
        .unwrap(),
        E::Newtype(7)
    );

    // SeqRefDeserializer and MapRefDeserializer size_hint verification
    let seq_content = Content::Seq(vec![Content::U8(1), Content::U8(2), Content::U8(3)]);
    let (seq_hint, seq_count) = ContentRefDeserializer::<ValueError>::new(&seq_content)
        .deserialize_seq(SizeHintSeqVisitor)
        .unwrap();
    assert_eq!(seq_hint, Some(3));
    assert_eq!(seq_count, 3);

    let map_content = Content::Map(vec![
        (Content::Str("a"), Content::U8(1)),
        (Content::Str("b"), Content::U8(2)),
    ]);
    let (map_hint, map_count) = ContentRefDeserializer::<ValueError>::new(&map_content)
        .deserialize_map(SizeHintMapVisitor)
        .unwrap();
    assert_eq!(map_hint, Some(2));
    assert_eq!(map_count, 2);
}
