use backwards_compat::__private::{
    AdjacentKey, AdjacentKeyResult, ChainedMapAccess, Content, ContentDeserializer, InternalKey,
    InternalKeyResult, RemainingMapDeserializer, read_adjacent_content,
};
use core::fmt;
use core::marker::PhantomData;
use serde::Deserialize;
use serde::de::value::Error as ValueError;
use serde::de::{self, DeserializeSeed as _, Deserializer as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct BorrowedPair<'a> {
    first: &'a str,
    second: &'a str,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct UnitStruct;

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

    // Mix of:
    // 1. Tag first with nested remaining entries (RemainingMapDeserializer directly)
    // 2. Tag first with no remaining entries (empty RemainingMapDeserializer)
    // 3. Tag in middle with prefix + remaining entries (ChainedMapAccess + RemainingMapDeserializer)
    // 4. Trailing scalar value proving the stream is aligned with zero trailing-token errors.
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

    // When only an empty-string key "" follows the tag, read_adjacent_content must not falsely match
    // and must return missing_field("content").
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
                        ContentDeserializer::<ValueError>::new(Content::Map(vec![(
                            Content::Str("k"),
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

    // capture_other = false drains enum cleanly
    let ignore_key = AdjacentKey::new("version", "content", false);
    assert!(matches!(
        ignore_key.deserialize(EnumOnlyDe).unwrap(),
        AdjacentKeyResult::Ignored
    ));

    // capture_other = true rejects enum (Content has no Enum variant)
    let capture_key = AdjacentKey::new("version", "content", true);
    let _ = capture_key.deserialize(EnumOnlyDe).unwrap_err();

    // capture_other = false drains Some and Newtype cleanly
    let some_content = Content::Some(Box::new(Content::Seq(vec![Content::U8(1), Content::U8(2)])));
    assert!(matches!(
        ignore_key
            .deserialize(ContentDeserializer::<ValueError>::new(some_content))
            .unwrap(),
        AdjacentKeyResult::Ignored
    ));

    let newtype_content = Content::Newtype(Box::new(Content::Map(vec![(Content::Str("x"), Content::U8(9))])));
    assert!(matches!(
        ignore_key
            .deserialize(ContentDeserializer::<ValueError>::new(newtype_content))
            .unwrap(),
        AdjacentKeyResult::Ignored
    ));
}
