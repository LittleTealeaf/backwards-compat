#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "unit tests"
)]
use super::*;
use crate::de::content::{Content, ContentDeserializer};
use alloc::borrow::ToOwned as _;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString as _;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::Deserialize;
use serde::de::value::{BorrowedBytesDeserializer, BytesDeserializer, Error as ValueError};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

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

    let opt: Option<SimpleStruct> = deserialize_after_tag(r#"{"version": 1, "x": 4, "y": "some"}"#).unwrap();
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
    let multi_err =
        deserialize_after_tag::<ExternalEnum>(r#"{"version": 1, "Newtype": 42, "extra": 1}"#).unwrap_err();
    assert!(multi_err.to_string().contains("single key"));
}

#[test]
fn test_remaining_map_deserializer_ignored_any_and_primitives() {
    let _: IgnoredAny = deserialize_after_tag(r#"{"version": 1, "a": [1, 2, 3], "b": {"c": true}}"#).unwrap();

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

fn deserialize_after_leading_tag<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, serde_json::Error> {
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

    let (ver, empty_map): (u64, BTreeMap<&str, &str>) = deserialize_with_chained_tag(&input_empty).unwrap();
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
                                    let _ = IgnoredAny::deserialize(RemainingMapDeserializer::new(chained))?;
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
