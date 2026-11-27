#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "unit tests"
)]
use super::*;
use alloc::borrow::ToOwned as _;
use alloc::collections::BTreeMap;
use alloc::string::ToString as _;
use alloc::vec;
use serde::de::IntoDeserializer as _;
use serde::de::value::Error as ValueError;

#[test]
fn test_content_size() {
    assert!(
        core::mem::size_of::<Content<'_>>() <= 24,
        "Content<'_> exceeded 24 bytes: {}",
        core::mem::size_of::<Content<'_>>()
    );
}

#[test]
fn test_content_unexpected_all_variants() {
    let non_bool_cases: Vec<(Content<'_>, &str)> = vec![
        (Content::U8(1), "integer `1`"),
        (Content::U16(2), "integer `2`"),
        (Content::U32(3), "integer `3`"),
        (Content::U64(4), "integer `4`"),
        (Content::U128(Box::new(5)), "128-bit unsigned integer"),
        (Content::I8(-1), "integer `-1`"),
        (Content::I16(-2), "integer `-2`"),
        (Content::I32(-3), "integer `-3`"),
        (Content::I64(-4), "integer `-4`"),
        (Content::I128(Box::new(-5)), "128-bit signed integer"),
        (Content::F32(1.5), "floating point"),
        (Content::F64(2.5), "floating point"),
        (Content::Char('x'), "character `x`"),
        (
            Content::String("owned".to_owned().into_boxed_str()),
            "string \"owned\"",
        ),
        (Content::Str("borrowed"), "string \"borrowed\""),
        (Content::ByteBuf(vec![1, 2].into_boxed_slice()), "byte array"),
        (Content::Bytes(b"ab"), "byte array"),
        (Content::None, "Option value"),
        (Content::Some(Box::new(Content::U8(1))), "Option value"),
        (Content::Unit, "unit value"),
        (Content::Newtype(Box::new(Content::U8(1))), "newtype struct"),
        (Content::Seq(vec![].into_boxed_slice()), "sequence"),
        (Content::Map(vec![].into_boxed_slice()), "map"),
    ];

    for (content, expected_substr) in non_bool_cases {
        let err = bool::deserialize(ContentDeserializer::<ValueError>::new(content))
            .expect_err("expected bool deserialization error");
        assert!(
            err.to_string().contains(expected_substr),
            "expected `{expected_substr}` in `{err}`"
        );
    }

    let bool_err = u8::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true)))
        .expect_err("expected u8 deserialization error for Bool");
    assert!(bool_err.to_string().contains("boolean `true`"));
}

#[test]
fn test_content_visitor_all_methods() {
    let visitor = || ContentVisitor { value: PhantomData };

    assert!(matches!(
        visitor().visit_bool::<ValueError>(true),
        Ok(Content::Bool(true))
    ));
    assert!(matches!(
        visitor().visit_i8::<ValueError>(-1),
        Ok(Content::I8(-1))
    ));
    assert!(matches!(
        visitor().visit_i16::<ValueError>(-2),
        Ok(Content::I16(-2))
    ));
    assert!(matches!(
        visitor().visit_i32::<ValueError>(-3),
        Ok(Content::I32(-3))
    ));
    assert!(matches!(
        visitor().visit_i64::<ValueError>(-4),
        Ok(Content::I64(-4))
    ));
    assert!(matches!(
        visitor().visit_i128::<ValueError>(-5),
        Ok(Content::I128(v)) if *v == -5
    ));
    assert!(matches!(visitor().visit_u8::<ValueError>(1), Ok(Content::U8(1))));
    assert!(matches!(
        visitor().visit_u16::<ValueError>(2),
        Ok(Content::U16(2))
    ));
    assert!(matches!(
        visitor().visit_u32::<ValueError>(3),
        Ok(Content::U32(3))
    ));
    assert!(matches!(
        visitor().visit_u64::<ValueError>(4),
        Ok(Content::U64(4))
    ));
    assert!(matches!(
        visitor().visit_u128::<ValueError>(5),
        Ok(Content::U128(v)) if *v == 5
    ));
    assert!(
        matches!(visitor().visit_f32::<ValueError>(1.25), Ok(Content::F32(v)) if (v - 1.25).abs() < f32::EPSILON)
    );
    assert!(
        matches!(visitor().visit_f64::<ValueError>(2.5), Ok(Content::F64(v)) if (v - 2.5).abs() < f64::EPSILON)
    );
    assert!(matches!(
        visitor().visit_char::<ValueError>('z'),
        Ok(Content::Char('z'))
    ));
    assert!(matches!(visitor().visit_str::<ValueError>("hi"), Ok(Content::String(s)) if &*s == "hi"));
    assert!(matches!(
        visitor().visit_borrowed_str::<ValueError>("hi"),
        Ok(Content::Str("hi"))
    ));
    assert!(
        matches!(visitor().visit_string::<ValueError>("hi".to_owned()), Ok(Content::String(s)) if &*s == "hi")
    );
    assert!(matches!(visitor().visit_bytes::<ValueError>(b"ok"), Ok(Content::ByteBuf(b)) if &*b == b"ok"));
    assert!(matches!(
        visitor().visit_borrowed_bytes::<ValueError>(b"ok"),
        Ok(Content::Bytes(b"ok"))
    ));
    assert!(matches!(visitor().visit_byte_buf::<ValueError>(vec![9]), Ok(Content::ByteBuf(b)) if *b == [9]));
    assert!(matches!(visitor().visit_unit::<ValueError>(), Ok(Content::Unit)));
    assert!(matches!(visitor().visit_none::<ValueError>(), Ok(Content::None)));
    assert!(matches!(
        visitor().visit_some(ContentDeserializer::<ValueError>::new(Content::U8(7))),
        Ok(Content::Some(inner)) if matches!(*inner, Content::U8(7))
    ));
    assert!(matches!(
        visitor().visit_newtype_struct(ContentDeserializer::<ValueError>::new(Content::U8(8))),
        Ok(Content::Newtype(inner)) if matches!(*inner, Content::U8(8))
    ));

    // Expecting formatter
    let err: ValueError = de::Error::invalid_type(Unexpected::Enum, &visitor());
    assert!(err.to_string().contains("any value"));
}

#[test]
fn test_content_deserializer_primitives_and_containers() {
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
    assert!(bool::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true))).unwrap());

    // Integers across all signed/unsigned types
    assert_eq!(
        i8::deserialize(ContentDeserializer::<ValueError>::new(Content::I8(-8))).unwrap(),
        -8
    );
    assert_eq!(
        i16::deserialize(ContentDeserializer::<ValueError>::new(Content::I16(-16))).unwrap(),
        -16
    );
    assert_eq!(
        i32::deserialize(ContentDeserializer::<ValueError>::new(Content::I32(-32))).unwrap(),
        -32
    );
    assert_eq!(
        i64::deserialize(ContentDeserializer::<ValueError>::new(Content::I64(-64))).unwrap(),
        -64
    );
    assert_eq!(
        i128::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(Box::new(
            -128
        ))))
        .unwrap(),
        -128
    );
    assert_eq!(
        u8::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(8))).unwrap(),
        8
    );
    assert_eq!(
        u16::deserialize(ContentDeserializer::<ValueError>::new(Content::U16(16))).unwrap(),
        16
    );
    assert_eq!(
        u32::deserialize(ContentDeserializer::<ValueError>::new(Content::U32(32))).unwrap(),
        32
    );
    assert_eq!(
        u64::deserialize(ContentDeserializer::<ValueError>::new(Content::U64(64))).unwrap(),
        64
    );
    assert_eq!(
        u128::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(Box::new(
            128
        ))))
        .unwrap(),
        128
    );
    let _ = i32::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("nope"))).unwrap_err();

    // Floats including int-to-float coercion
    assert!(
        (f32::deserialize(ContentDeserializer::<ValueError>::new(Content::F32(1.5))).unwrap() - 1.5).abs()
            < f32::EPSILON
    );
    assert!(
        (f64::deserialize(ContentDeserializer::<ValueError>::new(Content::F64(2.5))).unwrap() - 2.5).abs()
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
        let val = f64::deserialize(ContentDeserializer::<ValueError>::new(c)).unwrap();
        assert!((val - expected).abs() < f64::EPSILON);
    }
    let _ = f64::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(false))).unwrap_err();

    // Char
    assert_eq!(
        char::deserialize(ContentDeserializer::<ValueError>::new(Content::Char('a'))).unwrap(),
        'a'
    );
    assert_eq!(
        char::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
            "b".to_owned().into_boxed_str()
        )))
        .unwrap(),
        'b'
    );
    assert_eq!(
        char::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("c"))).unwrap(),
        'c'
    );
    let _ = char::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    // Str & String
    assert_eq!(
        <&str>::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("s"))).unwrap(),
        "s"
    );
    assert_eq!(
        String::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
            "o".to_owned().into_boxed_str()
        )))
        .unwrap(),
        "o"
    );
    assert_eq!(
        String::deserialize(ContentDeserializer::<ValueError>::new(Content::ByteBuf(
            b"bb".to_vec().into_boxed_slice()
        )))
        .unwrap(),
        "bb"
    );
    assert_eq!(
        <&str>::deserialize(ContentDeserializer::<ValueError>::new(Content::Bytes(b"bs"))).unwrap(),
        "bs"
    );
    let _ = String::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    // Bytes & ByteBuf
    assert_eq!(
        ContentDeserializer::<ValueError>::new(Content::String("ab".to_owned().into_boxed_str()))
            .deserialize_bytes(BytesVisitor)
            .unwrap(),
        b"ab"
    );
    assert_eq!(
        ContentDeserializer::<ValueError>::new(Content::Str("cd"))
            .deserialize_byte_buf(BytesVisitor)
            .unwrap(),
        b"cd"
    );
    assert_eq!(
        ContentDeserializer::<ValueError>::new(Content::ByteBuf(vec![1, 2].into_boxed_slice()))
            .deserialize_bytes(BytesVisitor)
            .unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        ContentDeserializer::<ValueError>::new(Content::Bytes(&[3, 4]))
            .deserialize_byte_buf(BytesVisitor)
            .unwrap(),
        vec![3, 4]
    );
    assert_eq!(
        ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![Content::U8(5), Content::U8(6)].into_boxed_slice()
        ))
        .deserialize_byte_buf(BytesVisitor)
        .unwrap(),
        vec![5, 6]
    );
    let _ = ContentDeserializer::<ValueError>::new(Content::Bool(true))
        .deserialize_byte_buf(BytesVisitor)
        .unwrap_err();

    // Option
    assert_eq!(
        Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::None)).unwrap(),
        None
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap(),
        None
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Some(Box::new(
            Content::U8(9)
        ))))
        .unwrap(),
        Some(9)
    );
    assert_eq!(
        Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(10))).unwrap(),
        Some(10)
    );

    // Unit & unit struct
    <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap();
    <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![].into_boxed_slice(),
    )))
    .unwrap();
    let _ = <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    assert_eq!(
        UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
            vec![].into_boxed_slice()
        )))
        .unwrap(),
        UnitS
    );
    assert_eq!(
        UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![].into_boxed_slice()
        )))
        .unwrap(),
        UnitS
    );
    assert_eq!(
        UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap(),
        UnitS
    );
    let _ = UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    // Newtype struct
    assert_eq!(
        Nt::deserialize(ContentDeserializer::<ValueError>::new(Content::Newtype(
            Box::new(Content::U16(3))
        )))
        .unwrap(),
        Nt(3)
    );
    assert_eq!(
        Nt::deserialize(ContentDeserializer::<ValueError>::new(Content::U16(4))).unwrap(),
        Nt(4)
    );

    // Seq, tuple, tuple struct, map, struct
    assert_eq!(
        Vec::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![Content::U8(1)].into_boxed_slice()
        )))
        .unwrap(),
        vec![1]
    );
    let _ = Vec::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap_err();
    assert_eq!(
        <(u8, bool)>::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![Content::U8(2), Content::Bool(true)].into_boxed_slice()
        )))
        .unwrap(),
        (2, true)
    );

    assert_eq!(
        Pair::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![Content::U8(3), Content::U8(4)].into_boxed_slice()
        )))
        .unwrap(),
        Pair(3, 4)
    );

    assert_eq!(
        BTreeMap::<String, u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
            vec![(Content::Str("k"), Content::U8(5))].into_boxed_slice()
        )))
        .unwrap()
        .get("k"),
        Some(&5)
    );
    let _ = BTreeMap::<String, u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit))
        .unwrap_err();

    assert_eq!(
        Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(
            vec![Content::U8(1), Content::U8(2)].into_boxed_slice()
        )))
        .unwrap(),
        Pt { x: 1, y: 2 }
    );
    assert_eq!(
        Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
            vec![
                (Content::Str("x"), Content::U8(3)),
                (Content::Str("y"), Content::U8(4)),
            ]
            .into_boxed_slice()
        )))
        .unwrap(),
        Pt { x: 3, y: 4 }
    );
    let _ = Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    // Identifier (String, Str, ByteBuf, Bytes, U8, U64, invalid)
    for id_content in [
        Content::String("alpha".to_owned().into_boxed_str()),
        Content::Str("alpha"),
        Content::ByteBuf(b"alpha".to_vec().into_boxed_slice()),
        Content::Bytes(b"alpha"),
        Content::U8(0),
        Content::U64(0),
    ] {
        assert_eq!(
            Field::deserialize(ContentDeserializer::<ValueError>::new(id_content)).unwrap(),
            Field::Alpha
        );
    }
    let _ = Field::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true))).unwrap_err();

    // IgnoredAny & IntoDeserializer
    serde::de::IgnoredAny::deserialize(
        ContentDeserializer::<ValueError>::new(Content::U64(123)).into_deserializer(),
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
        let _ = Content::deserialize(ContentDeserializer::<ValueError>::new(sample)).unwrap();
    }
}

#[test]
fn test_enum_variant_errors_and_adjacent_newtype() {
    #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
    enum Sample {
        Unit,
        Newtype(u8),
        Tuple(u8, u8),
        Struct { a: u8 },
    }

    // Valid enum forms
    assert_eq!(
        Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Unit"))).unwrap(),
        Sample::Unit
    );
    assert_eq!(
        Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
            vec![(
                Content::Str("Struct"),
                Content::Seq(vec![Content::U8(7)].into_boxed_slice()),
            )]
            .into_boxed_slice()
        )))
        .unwrap(),
        Sample::Struct { a: 7 }
    );

    // deserialize_enum errors: empty map, multi-key map, non-string/non-map
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![
            (Content::Str("Unit"), Content::Unit),
            (Content::Str("Newtype"), Content::U8(1)),
        ]
        .into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

    // VariantDeserializer error arms
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![(Content::Str("Unit"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Newtype"))).unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Tuple"))).unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![(Content::Str("Tuple"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Struct"))).unwrap_err();
    let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(
        vec![(Content::Str("Struct"), Content::U8(1))].into_boxed_slice(),
    )))
    .unwrap_err();
}
