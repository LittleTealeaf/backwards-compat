#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "unit tests"
)]
use super::*;
use alloc::borrow::ToOwned as _;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use serde::de::IntoDeserializer as _;
use serde::de::value::Error as ValueError;

#[test]
fn test_content_ref_deserializer_primitives_and_containers() {
    struct BytesVisitor;
    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Vec<u8>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("bytes")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.as_bytes().to_vec())
        }
        fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            Ok(v.to_vec())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(b) = seq.next_element::<u8>()? {
                out.push(b);
            }
            Ok(out)
        }
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct UnitS;

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Nt(u16);

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Pair(u8, u8);

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct Pt {
        x: u8,
        y: u8,
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    #[serde(field_identifier, rename_all = "lowercase")]
    enum Field {
        Alpha,
    }

    // Bool
    assert!(bool::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Bool(true))).unwrap());
    let _ = bool::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    // Integers across all signed/unsigned types
    assert_eq!(
        i8::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I8(-8))).unwrap(),
        -8
    );
    assert_eq!(
        i16::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I16(-16))).unwrap(),
        -16
    );
    assert_eq!(
        i32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I32(-32))).unwrap(),
        -32
    );
    assert_eq!(
        i64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I64(-64))).unwrap(),
        -64
    );
    assert_eq!(
        i128::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I128(
            Box::new(-128)
        )))
        .unwrap(),
        -128
    );
    assert_eq!(
        u8::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(8))).unwrap(),
        8
    );
    assert_eq!(
        u16::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U16(16))).unwrap(),
        16
    );
    assert_eq!(
        u32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U32(32))).unwrap(),
        32
    );
    assert_eq!(
        u64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U64(64))).unwrap(),
        64
    );
    assert_eq!(
        u128::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U128(
            Box::new(128)
        )))
        .unwrap(),
        128
    );
    let _ = i32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("nope"))).unwrap_err();

    // Floats including int-to-float coercion
    assert!(
        (f32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::F32(1.5))).unwrap() - 1.5)
            .abs()
            < f32::EPSILON
    );
    assert!(
        (f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::F64(2.5))).unwrap() - 2.5)
            .abs()
            < f64::EPSILON
    );
    for (c, expected) in [
        (Content::U8(1), 1.0_f64),
        (Content::U16(2), 2.0),
        (Content::U32(3), 3.0),
        (Content::U64(4), 4.0),
        (Content::I8(-1), -1.0),
        (Content::I16(-2), -2.0),
        (Content::I32(-3), -3.0),
        (Content::I64(-4), -4.0),
    ] {
        let val = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&c)).unwrap();
        assert!((val - expected).abs() < f64::EPSILON);
    }
    let _ = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Bool(false))).unwrap_err();

    // Char
    assert_eq!(
        char::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Char('a'))).unwrap(),
        'a'
    );
    assert_eq!(
        char::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::String(
            "b".to_owned().into_boxed_str()
        )))
        .unwrap(),
        'b'
    );
    assert_eq!(
        char::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("c"))).unwrap(),
        'c'
    );
    let _ = char::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    // Str & String (including 'de borrowing)
    let borrowed_str: &'static str =
        <&'static str>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("s"))).unwrap();
    assert_eq!(borrowed_str, "s");
    assert_eq!(
        String::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::String(
            "o".to_owned().into_boxed_str()
        )))
        .unwrap(),
        "o"
    );
    assert_eq!(
        String::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::ByteBuf(
            b"bb".to_vec().into_boxed_slice()
        )))
        .unwrap(),
        "bb"
    );
    let borrowed_from_bytes: &'static str =
        <&'static str>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Bytes(b"bs")))
            .unwrap();
    assert_eq!(borrowed_from_bytes, "bs");
    let _ = String::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    // Bytes & ByteBuf
    assert_eq!(
        ContentRefDeserializer::<ValueError>::new(&Content::String("ab".to_owned().into_boxed_str()))
            .deserialize_bytes(BytesVisitor)
            .unwrap(),
        b"ab"
    );
    assert_eq!(
        ContentRefDeserializer::<ValueError>::new(&Content::Str("cd"))
            .deserialize_byte_buf(BytesVisitor)
            .unwrap(),
        b"cd"
    );
    assert_eq!(
        ContentRefDeserializer::<ValueError>::new(&Content::ByteBuf(vec![1, 2].into_boxed_slice()))
            .deserialize_bytes(BytesVisitor)
            .unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        ContentRefDeserializer::<ValueError>::new(&Content::Bytes(&[3, 4]))
            .deserialize_byte_buf(BytesVisitor)
            .unwrap(),
        vec![3, 4]
    );
    assert_eq!(
        ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![Content::U8(5), Content::U8(6)].into_boxed_slice()
        ))
        .deserialize_byte_buf(BytesVisitor)
        .unwrap(),
        vec![5, 6]
    );
    let _ = ContentRefDeserializer::<ValueError>::new(&Content::Bool(true))
        .deserialize_byte_buf(BytesVisitor)
        .unwrap_err();

    // Option
    assert_eq!(
        Option::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::None)).unwrap(),
        None
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Unit)).unwrap(),
        None
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Some(
            Box::new(Content::U8(9))
        )))
        .unwrap(),
        Some(9)
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(10))).unwrap(),
        Some(10)
    );

    // Unit & unit struct
    <()>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Unit)).unwrap();
    <()>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![].into_boxed_slice(),
    )))
    .unwrap();
    let _ = <()>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    assert_eq!(
        UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![].into_boxed_slice()
        )))
        .unwrap(),
        UnitS
    );
    assert_eq!(
        UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![].into_boxed_slice()
        )))
        .unwrap(),
        UnitS
    );
    assert_eq!(
        UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Unit)).unwrap(),
        UnitS
    );
    let _ = UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    // Newtype struct
    assert_eq!(
        Nt::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Newtype(
            Box::new(Content::U16(3))
        )))
        .unwrap(),
        Nt(3)
    );
    assert_eq!(
        Nt::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U16(4))).unwrap(),
        Nt(4)
    );

    // Seq, tuple, tuple struct, map, struct
    assert_eq!(
        Vec::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![Content::U8(1)].into_boxed_slice()
        )))
        .unwrap(),
        vec![1]
    );
    let _ = Vec::<u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Unit)).unwrap_err();
    assert_eq!(
        <(u8, bool)>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![Content::U8(2), Content::Bool(true)].into_boxed_slice()
        )))
        .unwrap(),
        (2, true)
    );
    assert_eq!(
        Pair::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![Content::U8(3), Content::U8(4)].into_boxed_slice()
        )))
        .unwrap(),
        Pair(3, 4)
    );

    assert_eq!(
        BTreeMap::<String, u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(Content::Str("k"), Content::U8(5))].into_boxed_slice()
        )))
        .unwrap()
        .get("k"),
        Some(&5)
    );
    let _ = BTreeMap::<String, u8>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Unit))
        .unwrap_err();

    assert_eq!(
        Pt::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
            vec![Content::U8(1), Content::U8(2)].into_boxed_slice()
        )))
        .unwrap(),
        Pt { x: 1, y: 2 }
    );
    assert_eq!(
        Pt::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![
                (Content::Str("x"), Content::U8(3)),
                (Content::Str("y"), Content::U8(4)),
            ]
            .into_boxed_slice()
        )))
        .unwrap(),
        Pt { x: 3, y: 4 }
    );
    let _ = Pt::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();

    // Identifier
    for id_content in [
        Content::String("alpha".to_owned().into_boxed_str()),
        Content::Str("alpha"),
        Content::ByteBuf(b"alpha".to_vec().into_boxed_slice()),
        Content::Bytes(b"alpha"),
        Content::U8(0),
        Content::U64(0),
    ] {
        assert_eq!(
            Field::deserialize(ContentRefDeserializer::<ValueError>::new(&id_content)).unwrap(),
            Field::Alpha
        );
    }
    let _ = Field::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Bool(true))).unwrap_err();

    // IgnoredAny & IntoDeserializer
    serde::de::IgnoredAny::deserialize(
        ContentRefDeserializer::<ValueError>::new(&Content::U64(123)).into_deserializer(),
    )
    .unwrap();

    // deserialize_any roundtrip across variants
    for sample in [
        Content::Bool(false),
        Content::U8(1),
        Content::U16(2),
        Content::U32(3),
        Content::U64(4),
        Content::U128(Box::new(5)),
        Content::I8(-1),
        Content::I16(-2),
        Content::I32(-3),
        Content::I64(-4),
        Content::I128(Box::new(-5)),
        Content::F32(1.0),
        Content::F64(2.0),
        Content::Char('a'),
        Content::String("s".to_owned().into_boxed_str()),
        Content::Str("b"),
        Content::ByteBuf(vec![1].into_boxed_slice()),
        Content::Bytes(&[2]),
        Content::Unit,
        Content::None,
        Content::Some(Box::new(Content::U8(1))),
        Content::Newtype(Box::new(Content::U8(2))),
        Content::Seq(vec![Content::U8(3)].into_boxed_slice()),
        Content::Map(vec![(Content::Str("k"), Content::U8(4))].into_boxed_slice()),
    ] {
        let _ = Content::deserialize(ContentRefDeserializer::<ValueError>::new(&sample)).unwrap();
    }
}

#[test]
fn test_content_ref_deserializer_multi_retry_and_enums() {
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct SchemaA<'a> {
        a: &'a str,
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct SchemaB<'a> {
        b: &'a str,
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    enum Sample {
        Unit,
        Newtype(u8),
        Tuple(u8, u8),
        Struct { a: u8 },
    }

    // Reuse the same Content across multiple retries while preserving 'de borrow
    let input = String::from("{\"b\":\"hello\"}");
    let buffered: Content<'_> = serde_json::from_str(&input).unwrap();
    let _ = SchemaA::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap_err();
    let decoded = SchemaB::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap();
    assert_eq!(decoded, SchemaB { b: "hello" });

    assert_eq!(
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("Unit"))).unwrap(),
        Sample::Unit
    );
    assert_eq!(
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(Content::Str("Newtype"), Content::U8(5))].into_boxed_slice()
        )))
        .unwrap(),
        Sample::Newtype(5)
    );
    assert_eq!(
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(
                Content::Str("Tuple"),
                Content::Seq(vec![Content::U8(1), Content::U8(2)].into_boxed_slice()),
            )]
            .into_boxed_slice()
        )))
        .unwrap(),
        Sample::Tuple(1, 2)
    );
    assert_eq!(
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(
                Content::Str("Struct"),
                Content::Seq(vec![Content::U8(7)].into_boxed_slice()),
            )]
            .into_boxed_slice()
        )))
        .unwrap(),
        Sample::Struct { a: 7 }
    );
    assert_eq!(
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(
                Content::Str("Struct"),
                Content::Map(vec![(Content::Str("a"), Content::U8(9))].into_boxed_slice()),
            )]
            .into_boxed_slice()
        )))
        .unwrap(),
        Sample::Struct { a: 9 }
    );

    // Enum errors
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![
            (Content::Str("Unit"), Content::Unit),
            (Content::Str("Newtype"), Content::U8(1)),
        ]
        .into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U8(1))).unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![(Content::Str("Unit"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str(
        "Newtype",
    )))
    .unwrap_err();
    let _ =
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("Tuple"))).unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![(Content::Str("Tuple"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ =
        Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Str("Struct"))).unwrap_err();
    let _ = Sample::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![(Content::Str("Struct"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
}

#[test]
fn test_content_ref_deserializer_borrows_outlive_content_buffer() {
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct FailFirst<'a> {
        missing: &'a str,
    }

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct SucceedSecond<'a> {
        name: &'a str,
    }

    let input = String::from("{\"name\":\"zero-copy\"}");
    let decoded: SucceedSecond<'_>;
    {
        let buffered: Content<'_> = serde_json::from_str(&input).unwrap();
        let _ = FailFirst::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap_err();
        decoded = SucceedSecond::deserialize(ContentRefDeserializer::<ValueError>::new(&buffered)).unwrap();
    }
    assert_eq!(decoded, SucceedSecond { name: "zero-copy" });

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
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    struct UnitS;

    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
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

    let val = f32::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I32(-42))).unwrap();
    assert!((val - (-42.0_f32)).abs() < f32::EPSILON);
    let _ = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::U128(
        Box::new(1),
    )))
    .unwrap_err();
    let _ = f64::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::I128(
        Box::new(-1),
    )))
    .unwrap_err();

    let _ = <()>::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![(Content::Str("k"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
        vec![(Content::Str("k"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = UnitS::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Seq(
        vec![Content::U8(1)].into_boxed_slice(),
    )))
    .unwrap_err();

    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::String(
            "Unit".to_owned().into_boxed_str()
        )))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(Content::Str("Unit"), Content::Unit)].into_boxed_slice()
        )))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(Content::Str("Unit"), Content::Map(vec![].into_boxed_slice()))].into_boxed_slice()
        )))
        .unwrap(),
        E::Unit
    );
    assert_eq!(
        E::deserialize(ContentRefDeserializer::<ValueError>::new(&Content::Map(
            vec![(
                Content::String("Newtype".to_owned().into_boxed_str()),
                Content::U8(7),
            )]
            .into_boxed_slice()
        )))
        .unwrap(),
        E::Newtype(7)
    );

    let seq_content = Content::Seq(vec![Content::U8(1), Content::U8(2), Content::U8(3)].into_boxed_slice());
    let (seq_hint, seq_count) = ContentRefDeserializer::<ValueError>::new(&seq_content)
        .deserialize_seq(SizeHintSeqVisitor)
        .unwrap();
    assert_eq!(seq_hint, Some(3));
    assert_eq!(seq_count, 3);

    let map_content = Content::Map(
        vec![
            (Content::Str("a"), Content::U8(1)),
            (Content::Str("b"), Content::U8(2)),
        ]
        .into_boxed_slice(),
    );
    let (map_hint, map_count) = ContentRefDeserializer::<ValueError>::new(&map_content)
        .deserialize_map(SizeHintMapVisitor)
        .unwrap();
    assert_eq!(map_hint, Some(2));
    assert_eq!(map_count, 2);
}
