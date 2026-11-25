use backwards_compat::{BackwardsCompatDeserialize, Migrated, backwards_compat};
use serde::de::value::Error as ValueError;
use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize as DeriveDeserialize, Serialize as DeriveSerialize};

fn assert_borrowed_within(borrowed: &str, input: &str) {
    let input_start = input.as_ptr() as usize;
    let input_end = input_start + input.len();
    let borrowed_start = borrowed.as_ptr() as usize;
    let borrowed_end = borrowed_start + borrowed.len();
    assert!(
        borrowed_start >= input_start && borrowed_end <= input_end,
        "expected `{borrowed}` ({borrowed_start:#x}..{borrowed_end:#x}) to be zero-copy borrowed from input ({input_start:#x}..{input_end:#x})"
    );
}

// =========================================================================
// Custom Deserializers for fail-fast and positional visit_seq verification
// =========================================================================

#[derive(Clone, Copy)]
enum ScriptedVal<'de> {
    U64(u64),
    Str(&'de str),
    Bool(bool),
    Null,
}

impl<'de> Deserializer<'de> for ScriptedVal<'de> {
    type Error = ValueError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::U64(n) => visitor.visit_u64(n),
            Self::Str(s) => visitor.visit_borrowed_str(s),
            Self::Bool(b) => visitor.visit_bool(b),
            Self::Null => visitor.visit_unit(),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

enum ScriptedEntry<'de> {
    Pair(&'de str, ScriptedVal<'de>),
    PanicIfPolled(&'static str),
}

struct FailFastMapDeserializer<'de> {
    entries: &'de [ScriptedEntry<'de>],
}

impl<'de> FailFastMapDeserializer<'de> {
    const fn new(entries: &'de [ScriptedEntry<'de>]) -> Self {
        Self { entries }
    }
}

struct FailFastMapAccess<'de> {
    entries: &'de [ScriptedEntry<'de>],
    pos: usize,
    pending_val: Option<ScriptedVal<'de>>,
}

impl<'de> MapAccess<'de> for FailFastMapAccess<'de> {
    type Error = ValueError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        let Some(entry) = self.entries.get(self.pos) else {
            return Ok(None);
        };
        self.pos += 1;
        match *entry {
            ScriptedEntry::Pair(key, val) => {
                self.pending_val = Some(val);
                seed.deserialize(ScriptedVal::Str(key)).map(Some)
            }
            ScriptedEntry::PanicIfPolled(msg) => {
                panic!("deserializer did not fail fast: {msg}");
            }
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        let val = self
            .pending_val
            .take()
            .ok_or_else(|| de::Error::custom("next_value_seed called without key"))?;
        seed.deserialize(val)
    }
}

impl<'de> Deserializer<'de> for FailFastMapDeserializer<'de> {
    type Error = ValueError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(FailFastMapAccess {
            entries: self.entries,
            pos: 0,
            pending_val: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

enum SeqItem<'de> {
    Tag(ScriptedVal<'de>),
    JsonPayload(&'de str),
    PanicIfPolled(&'static str),
}

struct SeqEnvelopeDeserializer<'de> {
    items: &'de [SeqItem<'de>],
}

struct SeqEnvelopeAccess<'de> {
    items: &'de [SeqItem<'de>],
    pos: usize,
}

impl<'de> SeqAccess<'de> for SeqEnvelopeAccess<'de> {
    type Error = ValueError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        let Some(item) = self.items.get(self.pos) else {
            return Ok(None);
        };
        self.pos += 1;
        match *item {
            SeqItem::Tag(val) => seed.deserialize(val).map(Some),
            SeqItem::JsonPayload(json) => {
                let mut de = serde_json::Deserializer::from_str(json);
                seed.deserialize(&mut de)
                    .map(Some)
                    .map_err(|e| de::Error::custom(e.to_string()))
            }
            SeqItem::PanicIfPolled(msg) => {
                panic!("seq deserializer did not fail fast: {msg}");
            }
        }
    }
}

impl<'de> Deserializer<'de> for SeqEnvelopeDeserializer<'de> {
    type Error = ValueError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(SeqEnvelopeAccess {
            items: self.items,
            pos: 0,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

// =========================================================================
// 1. Internal tagging: tag-first, tag-middle, tag-last, and fail-fast
// =========================================================================

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct InternalDocV1 {
    pub title: String,
    pub count: u32,
}

#[backwards_compat(versions(1: InternalDocV1, 2: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct InternalDoc {
    pub title: String,
    pub count: u32,
    pub active: bool,
}

impl From<InternalDocV1> for InternalDoc {
    fn from(v: InternalDocV1) -> Self {
        Self {
            title: v.title,
            count: v.count,
            active: true,
        }
    }
}

#[test]
fn test_internal_tag_first_middle_last_and_migrated() {
    // Tag-first fast path (v2 current & v1 migrated)
    let v2_first: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"version":2,"title":"first","count":1,"active":false}"#).unwrap();
    assert_eq!(v2_first.source_version, Some(2));
    assert!(!v2_first.was_migrated(), "v2 should not be marked migrated");
    assert_eq!(
        v2_first.into_inner(),
        InternalDoc {
            title: "first".to_owned(),
            count: 1,
            active: false,
        }
    );

    let v1_first: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"version":1,"title":"old_first","count":10}"#).unwrap();
    assert_eq!(v1_first.source_version, Some(1));
    assert!(v1_first.was_migrated(), "v1 should be marked migrated");
    assert_eq!(
        v1_first.into_inner(),
        InternalDoc {
            title: "old_first".to_owned(),
            count: 10,
            active: true,
        }
    );

    // Tag-middle slow path (prefix replay + remaining stream)
    let v2_mid: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"title":"mid","version":2,"count":2,"active":true}"#).unwrap();
    assert_eq!(v2_mid.source_version, Some(2));
    assert_eq!(
        v2_mid.into_inner(),
        InternalDoc {
            title: "mid".to_owned(),
            count: 2,
            active: true,
        }
    );

    let v1_mid: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"title":"old_mid","version":1,"count":20}"#).unwrap();
    assert_eq!(v1_mid.source_version, Some(1));
    assert_eq!(
        v1_mid.into_inner(),
        InternalDoc {
            title: "old_mid".to_owned(),
            count: 20,
            active: true,
        }
    );

    // Tag-last slow path (full prefix replay)
    let v2_last: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"title":"last","count":3,"active":false,"version":2}"#).unwrap();
    assert_eq!(v2_last.source_version, Some(2));
    assert_eq!(
        v2_last.into_inner(),
        InternalDoc {
            title: "last".to_owned(),
            count: 3,
            active: false,
        }
    );

    let v1_last: Migrated<InternalDoc> =
        serde_json::from_str(r#"{"title":"old_last","count":30,"version":1}"#).unwrap();
    assert_eq!(v1_last.source_version, Some(1));
    assert_eq!(
        v1_last.into_inner(),
        InternalDoc {
            title: "old_last".to_owned(),
            count: 30,
            active: true,
        }
    );
}

#[test]
fn test_internal_tag_fail_fast_on_bad_tags() {
    let cases: &[(&str, ScriptedVal<'_>)] = &[
        ("future version", ScriptedVal::U64(3)),
        ("unknown version", ScriptedVal::U64(99)),
        ("wrong tag type string", ScriptedVal::Str("2")),
        ("non-tag bool", ScriptedVal::Bool(true)),
        ("non-tag null", ScriptedVal::Null),
    ];

    for &(label, bad_tag) in cases {
        // Tag first: must fail before requesting any subsequent map key
        let entries_first = [
            ScriptedEntry::Pair("version", bad_tag),
            ScriptedEntry::PanicIfPolled("internal tag-first polled key after bad tag"),
        ];
        let err = InternalDoc::deserialize(FailFastMapDeserializer::new(&entries_first))
            .expect_err("expected error on bad tag");
        assert!(!err.to_string().is_empty(), "case `{label}` produced empty error");

        // Tag after a prefix field: must still fail before requesting keys after the bad tag
        let entries_mid = [
            ScriptedEntry::Pair("title", ScriptedVal::Str("prefix")),
            ScriptedEntry::Pair("version", bad_tag),
            ScriptedEntry::PanicIfPolled("internal tag-middle polled key after bad tag"),
        ];
        let err_mid = InternalDoc::deserialize(FailFastMapDeserializer::new(&entries_mid))
            .expect_err("expected error on bad tag in middle");
        assert!(
            !err_mid.to_string().is_empty(),
            "case `{label}` (middle) produced empty error"
        );
    }
}

// =========================================================================
// 2. Adjacent tagging: tag-before-content, content-before-tag, extra keys,
//    missing content, fail-fast, and positional visit_seq
// =========================================================================

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct AdjacentPayloadV1 {
    pub label: String,
}

#[backwards_compat(content = "data", versions(1: AdjacentPayloadV1, 2: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct AdjacentEnvelope {
    pub label: String,
    pub score: u32,
}

impl From<AdjacentPayloadV1> for AdjacentEnvelope {
    fn from(v: AdjacentPayloadV1) -> Self {
        Self {
            label: v.label,
            score: 0,
        }
    }
}

#[test]
fn test_adjacent_tag_order_extra_keys_and_missing_content() {
    // Tag before content (direct streaming)
    let v2_direct: Migrated<AdjacentEnvelope> =
        serde_json::from_str(r#"{"version":2,"data":{"label":"direct","score":10}}"#).unwrap();
    assert_eq!(v2_direct.source_version, Some(2));
    assert_eq!(
        v2_direct.into_inner(),
        AdjacentEnvelope {
            label: "direct".to_owned(),
            score: 10,
        }
    );

    let v1_direct: Migrated<AdjacentEnvelope> =
        serde_json::from_str(r#"{"version":1,"data":{"label":"old_direct"}}"#).unwrap();
    assert_eq!(v1_direct.source_version, Some(1));
    assert_eq!(
        v1_direct.into_inner(),
        AdjacentEnvelope {
            label: "old_direct".to_owned(),
            score: 0,
        }
    );

    // Content before tag (buffered content + trailing key drain)
    let v2_rev: Migrated<AdjacentEnvelope> =
        serde_json::from_str(r#"{"data":{"label":"rev","score":20},"version":2}"#).unwrap();
    assert_eq!(v2_rev.source_version, Some(2));
    assert_eq!(
        v2_rev.into_inner(),
        AdjacentEnvelope {
            label: "rev".to_owned(),
            score: 20,
        }
    );

    let v1_rev: Migrated<AdjacentEnvelope> =
        serde_json::from_str(r#"{"data":{"label":"old_rev"},"version":1}"#).unwrap();
    assert_eq!(v1_rev.source_version, Some(1));
    assert_eq!(
        v1_rev.into_inner(),
        AdjacentEnvelope {
            label: "old_rev".to_owned(),
            score: 0,
        }
    );

    // Extra unknown keys ignored in both orders
    let with_extras_1: AdjacentEnvelope = serde_json::from_str(
        r#"{"extra_a":123,"version":2,"extra_b":[1,2,3],"data":{"label":"extras1","score":30},"extra_c":{"k":"v"}}"#,
    )
    .unwrap();
    assert_eq!(
        with_extras_1,
        AdjacentEnvelope {
            label: "extras1".to_owned(),
            score: 30,
        }
    );

    let with_extras_2: AdjacentEnvelope = serde_json::from_str(
        r#"{"extra_a":"hi","data":{"label":"extras2","score":40},"extra_b":null,"version":2,"extra_c":true}"#,
    )
    .unwrap();
    assert_eq!(
        with_extras_2,
        AdjacentEnvelope {
            label: "extras2".to_owned(),
            score: 40,
        }
    );

    // Missing content field error
    let missing_err = serde_json::from_str::<AdjacentEnvelope>(r#"{"version":2}"#)
        .expect_err("expected missing content field error");
    assert!(
        missing_err.to_string().contains("data"),
        "expected missing field `data` error, got: {missing_err}"
    );

    let missing_with_other = serde_json::from_str::<AdjacentEnvelope>(r#"{"version":2,"other":1}"#)
        .expect_err("expected missing content field error with extra keys");
    assert!(
        missing_with_other.to_string().contains("data"),
        "expected missing field `data` error, got: {missing_with_other}"
    );
}

#[test]
fn test_adjacent_fail_fast_before_content_is_read() {
    for bad_tag in [
        ScriptedVal::U64(99),
        ScriptedVal::Str("v2"),
        ScriptedVal::Bool(false),
    ] {
        let entries = [
            ScriptedEntry::Pair("version", bad_tag),
            ScriptedEntry::PanicIfPolled("adjacent deserializer polled content key after bad tag"),
        ];
        let err = AdjacentEnvelope::deserialize(FailFastMapDeserializer::new(&entries))
            .expect_err("expected error on bad adjacent tag");
        assert!(!err.to_string().is_empty(), "error message must be non-empty");
    }
}

#[test]
fn test_adjacent_positional_visit_seq() {
    // v2 positional seq
    let items_v2 = [
        SeqItem::Tag(ScriptedVal::U64(2)),
        SeqItem::JsonPayload(r#"{"label":"from_seq","score":77}"#),
    ];
    let v2 = AdjacentEnvelope::deserialize_migrated(SeqEnvelopeDeserializer { items: &items_v2 }).unwrap();
    assert_eq!(v2.source_version, Some(2));
    assert_eq!(
        v2.into_inner(),
        AdjacentEnvelope {
            label: "from_seq".to_owned(),
            score: 77,
        }
    );

    // v1 positional seq
    let items_v1 = [
        SeqItem::Tag(ScriptedVal::U64(1)),
        SeqItem::JsonPayload(r#"{"label":"from_seq_v1"}"#),
    ];
    let v1 = AdjacentEnvelope::deserialize_migrated(SeqEnvelopeDeserializer { items: &items_v1 }).unwrap();
    assert_eq!(v1.source_version, Some(1));
    assert_eq!(
        v1.into_inner(),
        AdjacentEnvelope {
            label: "from_seq_v1".to_owned(),
            score: 0,
        }
    );

    // Fail-fast on bad tag in element 0 before element 1 is polled
    let items_bad = [
        SeqItem::Tag(ScriptedVal::U64(99)),
        SeqItem::PanicIfPolled("positional seq polled content element after bad tag"),
    ];
    let err = AdjacentEnvelope::deserialize(SeqEnvelopeDeserializer { items: &items_bad })
        .expect_err("expected error on bad tag in positional seq");
    assert!(!err.to_string().is_empty(), "error message must be non-empty");
}

// =========================================================================
// 3. Untagged fallback: single _, multiple _, non-maps, adjacent + "data"
//    field collision, and #[cfg]-gated _ entries
// =========================================================================

#[backwards_compat(content = "data", versions(_: u64 => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedNum {
    pub value: u64,
}

impl From<u64> for UntaggedNum {
    fn from(value: u64) -> Self {
        Self { value }
    }
}

#[backwards_compat(content = "data", versions(_: Vec<u32> => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedList {
    pub items: Vec<u32>,
}

impl From<Vec<u32>> for UntaggedList {
    fn from(items: Vec<u32>) -> Self {
        Self { items }
    }
}

#[backwards_compat(content = "data", versions(_: Option<String> => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedOpt {
    pub label: Option<String>,
}

impl From<Option<String>> for UntaggedOpt {
    fn from(label: Option<String>) -> Self {
        Self { label }
    }
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize, DeriveSerialize)]
pub enum PlainEnum {
    Alpha,
    Beta(u32),
}

#[backwards_compat(content = "data", versions(_: PlainEnum => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedEnumModel {
    pub desc: String,
}

impl From<PlainEnum> for UntaggedEnumModel {
    fn from(v: PlainEnum) -> Self {
        match v {
            PlainEnum::Alpha => Self {
                desc: "alpha".to_owned(),
            },
            PlainEnum::Beta(n) => Self {
                desc: format!("beta:{n}"),
            },
        }
    }
}

#[backwards_compat(content = "data", versions(_: Option<Vec<u32>> => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedOptVec {
    pub items: Option<Vec<u32>>,
}

impl From<Option<Vec<u32>>> for UntaggedOptVec {
    fn from(items: Option<Vec<u32>>) -> Self {
        Self { items }
    }
}

#[backwards_compat(content = "data", versions(_: Option<PlainEnum> => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedOptEnum {
    pub kind: Option<PlainEnum>,
}

impl From<Option<PlainEnum>> for UntaggedOptEnum {
    fn from(kind: Option<PlainEnum>) -> Self {
        Self { kind }
    }
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct MapLegacySingle {
    pub key: String,
}

#[backwards_compat(versions(_: MapLegacySingle => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct SingleUntaggedMap {
    pub key: String,
    pub v: u8,
}

impl From<MapLegacySingle> for SingleUntaggedMap {
    fn from(m: MapLegacySingle) -> Self {
        Self { key: m.key, v: 0 }
    }
}

#[test]
fn test_single_untagged_map_and_non_map_inputs() {
    // Map input
    let map_val: Migrated<SingleUntaggedMap> = serde_json::from_str(r#"{"key":"hello"}"#).unwrap();
    assert_eq!(map_val.source_version, None);
    assert_eq!(
        map_val.into_inner(),
        SingleUntaggedMap {
            key: "hello".to_owned(),
            v: 0,
        }
    );

    // Primitive input
    let num: Migrated<UntaggedNum> = serde_json::from_str("42").unwrap();
    assert_eq!(num.source_version, None);
    assert_eq!(num.into_inner(), UntaggedNum { value: 42 });

    // Vec input
    let list: Migrated<UntaggedList> = serde_json::from_str("[1, 2, 3]").unwrap();
    assert_eq!(list.source_version, None);
    assert_eq!(list.into_inner(), UntaggedList { items: vec![1, 2, 3] });

    // Option inputs (null and string)
    let opt_none: Migrated<UntaggedOpt> = serde_json::from_str("null").unwrap();
    assert_eq!(opt_none.source_version, None);
    assert_eq!(opt_none.into_inner(), UntaggedOpt { label: None });

    let opt_some: Migrated<UntaggedOpt> = serde_json::from_str(r#""present""#).unwrap();
    assert_eq!(opt_some.source_version, None);
    assert_eq!(
        opt_some.into_inner(),
        UntaggedOpt {
            label: Some("present".to_owned()),
        }
    );

    // Enum inputs (unit variant string and newtype variant map)
    let enum_unit: Migrated<UntaggedEnumModel> = serde_json::from_str(r#""Alpha""#).unwrap();
    assert_eq!(enum_unit.source_version, None);
    assert_eq!(
        enum_unit.into_inner(),
        UntaggedEnumModel {
            desc: "alpha".to_owned(),
        }
    );

    let enum_newtype: Migrated<UntaggedEnumModel> = serde_json::from_str(r#"{"Beta":9}"#).unwrap();
    assert_eq!(enum_newtype.source_version, None);
    assert_eq!(
        enum_newtype.into_inner(),
        UntaggedEnumModel {
            desc: "beta:9".to_owned(),
        }
    );

    // Option<Vec<u32>> on JSON (exercises OptionForwardingDeserializer on visit_seq)
    let opt_vec_some: Migrated<UntaggedOptVec> = serde_json::from_str("[4, 5]").unwrap();
    assert_eq!(opt_vec_some.source_version, None);
    assert_eq!(
        opt_vec_some.into_inner(),
        UntaggedOptVec {
            items: Some(vec![4, 5]),
        }
    );
    let opt_vec_none: Migrated<UntaggedOptVec> = serde_json::from_str("null").unwrap();
    assert_eq!(opt_vec_none.source_version, None);
    assert_eq!(opt_vec_none.into_inner(), UntaggedOptVec { items: None });

    // Option<PlainEnum> on JSON (exercises ContentDeserializer / OptionForwardingDeserializer)
    let opt_enum_unit: Migrated<UntaggedOptEnum> = serde_json::from_str(r#""Alpha""#).unwrap();
    assert_eq!(opt_enum_unit.source_version, None);
    assert_eq!(
        opt_enum_unit.into_inner(),
        UntaggedOptEnum {
            kind: Some(PlainEnum::Alpha),
        }
    );
    let opt_enum_newtype: Migrated<UntaggedOptEnum> = serde_json::from_str(r#"{"Beta":3}"#).unwrap();
    assert_eq!(opt_enum_newtype.source_version, None);
    assert_eq!(
        opt_enum_newtype.into_inner(),
        UntaggedOptEnum {
            kind: Some(PlainEnum::Beta(3)),
        }
    );
    let opt_enum_none: Migrated<UntaggedOptEnum> = serde_json::from_str("null").unwrap();
    assert_eq!(opt_enum_none.source_version, None);
    assert_eq!(opt_enum_none.into_inner(), UntaggedOptEnum { kind: None });
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct MultiMapA {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct MultiMapB {
    pub host: String,
}

#[backwards_compat(versions(_: MultiMapA => 1, _: MultiMapB => 1, 1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct MultiUntaggedMap {
    pub host: String,
    pub port: u16,
    pub source: String,
}

impl From<MultiMapA> for MultiUntaggedMap {
    fn from(v: MultiMapA) -> Self {
        Self {
            host: v.host,
            port: v.port,
            source: "A".to_owned(),
        }
    }
}

impl From<MultiMapB> for MultiUntaggedMap {
    fn from(v: MultiMapB) -> Self {
        Self {
            host: v.host,
            port: 80,
            source: "B".to_owned(),
        }
    }
}

#[backwards_compat(
    content = "data",
    versions(
        _: u64 => 1,
        _: String => 1,
        _: Vec<u64> => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct MultiUntaggedNonMap {
    pub tag: String,
}

impl From<u64> for MultiUntaggedNonMap {
    fn from(n: u64) -> Self {
        Self {
            tag: format!("u64:{n}"),
        }
    }
}

impl From<String> for MultiUntaggedNonMap {
    fn from(s: String) -> Self {
        Self {
            tag: format!("str:{s}"),
        }
    }
}

impl From<Vec<u64>> for MultiUntaggedNonMap {
    fn from(v: Vec<u64>) -> Self {
        Self {
            tag: format!("vec:{}", v.len()),
        }
    }
}

#[test]
fn test_multiple_untagged_declaration_order_map_and_non_map() {
    // Map: first arm wins when both match
    let a: MultiUntaggedMap = serde_json::from_str(r#"{"host":"db","port":5432}"#).unwrap();
    assert_eq!(
        a,
        MultiUntaggedMap {
            host: "db".to_owned(),
            port: 5432,
            source: "A".to_owned(),
        }
    );

    // Map: falls through to second arm when `port` is absent
    let b: MultiUntaggedMap = serde_json::from_str(r#"{"host":"web"}"#).unwrap();
    assert_eq!(
        b,
        MultiUntaggedMap {
            host: "web".to_owned(),
            port: 80,
            source: "B".to_owned(),
        }
    );

    // Non-map inputs in declaration order
    let from_num: MultiUntaggedNonMap = serde_json::from_str("7").unwrap();
    assert_eq!(from_num.tag, "u64:7");

    let from_str: MultiUntaggedNonMap = serde_json::from_str(r#""abc""#).unwrap();
    assert_eq!(from_str.tag, "str:abc");

    let from_vec: MultiUntaggedNonMap = serde_json::from_str("[10, 20, 30]").unwrap();
    assert_eq!(from_vec.tag, "vec:3");

    // Non-matching non-map fails cleanly
    let _ = serde_json::from_str::<MultiUntaggedNonMap>("true")
        .expect_err("bool should not match any untagged arm");
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct LegacyWithDataField {
    pub data: String,
    pub meta: u32,
}

#[backwards_compat(
    content = "data",
    versions(
        _: LegacyWithDataField => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct AdjacentWithDataCollision {
    pub payload: String,
    pub meta: u32,
}

impl From<LegacyWithDataField> for AdjacentWithDataCollision {
    fn from(v: LegacyWithDataField) -> Self {
        Self {
            payload: v.data,
            meta: v.meta,
        }
    }
}

#[test]
fn test_adjacent_untagged_map_containing_content_field_name_preserves_all_keys() {
    // `"data"` appears first, no `"version"` key at all
    let data_first: Migrated<AdjacentWithDataCollision> =
        serde_json::from_str(r#"{"data":"keep_me","meta":11}"#).unwrap();
    assert_eq!(data_first.source_version, None);
    assert_eq!(
        data_first.into_inner(),
        AdjacentWithDataCollision {
            payload: "keep_me".to_owned(),
            meta: 11,
        }
    );

    // `"data"` appears second, no `"version"` key at all
    let data_second: Migrated<AdjacentWithDataCollision> =
        serde_json::from_str(r#"{"meta":22,"data":"also_keep_me"}"#).unwrap();
    assert_eq!(data_second.source_version, None);
    assert_eq!(
        data_second.into_inner(),
        AdjacentWithDataCollision {
            payload: "also_keep_me".to_owned(),
            meta: 22,
        }
    );

    // Tagged adjacent payload still works normally
    let tagged: Migrated<AdjacentWithDataCollision> =
        serde_json::from_str(r#"{"version":1,"data":{"payload":"tagged","meta":33}}"#).unwrap();
    assert_eq!(tagged.source_version, Some(1));
    assert_eq!(
        tagged.into_inner(),
        AdjacentWithDataCollision {
            payload: "tagged".to_owned(),
            meta: 33,
        }
    );
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct DisabledUntaggedV0 {
    pub dead: String,
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct ActiveUntaggedV0 {
    pub live: String,
}

#[backwards_compat(
    versions(
        #[cfg(any())]
        _: DisabledUntaggedV0 => 1,
        #[cfg(all())]
        _: ActiveUntaggedV0 => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct CfgGatedUntaggedModel {
    pub text: String,
}

#[cfg(any())]
impl From<DisabledUntaggedV0> for CfgGatedUntaggedModel {
    fn from(v: DisabledUntaggedV0) -> Self {
        Self { text: v.dead }
    }
}

impl From<ActiveUntaggedV0> for CfgGatedUntaggedModel {
    fn from(v: ActiveUntaggedV0) -> Self {
        Self { text: v.live }
    }
}

#[backwards_compat(
    versions(
        #[cfg(any())]
        _: DisabledUntaggedV0 => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct OnlyDisabledUntaggedModel {
    pub text: String,
}

#[cfg(any())]
impl From<DisabledUntaggedV0> for OnlyDisabledUntaggedModel {
    fn from(v: DisabledUntaggedV0) -> Self {
        Self { text: v.dead }
    }
}

#[test]
fn test_cfg_gated_untagged_entries() {
    let active: Migrated<CfgGatedUntaggedModel> = serde_json::from_str(r#"{"live":"works"}"#).unwrap();
    assert_eq!(active.source_version, None);
    assert_eq!(
        active.into_inner(),
        CfgGatedUntaggedModel {
            text: "works".to_owned(),
        }
    );

    // Disabled `_` arm must not match
    let _ = serde_json::from_str::<CfgGatedUntaggedModel>(r#"{"dead":"nope"}"#)
        .expect_err("disabled cfg untagged arm must not match");

    // Model whose only `_` arm is cfg-disabled must reject untagged input
    let _ = serde_json::from_str::<OnlyDisabledUntaggedModel>(r#"{"dead":"nope"}"#)
        .expect_err("only-disabled untagged model must reject untagged payload");
    let tagged: OnlyDisabledUntaggedModel = serde_json::from_str(r#"{"version":1,"text":"ok"}"#).unwrap();
    assert_eq!(
        tagged,
        OnlyDisabledUntaggedModel {
            text: "ok".to_owned(),
        }
    );
}

// =========================================================================
// 4. Zero-copy &'de str borrowing across all streaming and buffered paths
// =========================================================================

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct BorrowedInternalV0<'a> {
    pub legacy: &'a str,
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct BorrowedInternalV1<'a> {
    pub text: &'a str,
}

#[backwards_compat(
    versions(
        _: BorrowedInternalV0<'a> => 1,
        1: BorrowedInternalV1<'a>,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct BorrowedInternal<'a> {
    pub text: &'a str,
    pub rank: u32,
}

impl<'a> From<BorrowedInternalV0<'a>> for BorrowedInternalV1<'a> {
    fn from(v: BorrowedInternalV0<'a>) -> Self {
        Self { text: v.legacy }
    }
}

impl<'a> From<BorrowedInternalV1<'a>> for BorrowedInternal<'a> {
    fn from(v: BorrowedInternalV1<'a>) -> Self {
        Self {
            text: v.text,
            rank: 1,
        }
    }
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct BorrowedAdjA<'a> {
    pub primary: &'a str,
    pub extra: u8,
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct BorrowedAdjB<'a> {
    pub fallback: &'a str,
}

#[backwards_compat(
    content = "data",
    versions(
        _: BorrowedAdjA<'a> => 1,
        _: BorrowedAdjB<'a> => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct BorrowedAdjacent<'a> {
    pub text: &'a str,
}

impl<'a> From<BorrowedAdjA<'a>> for BorrowedAdjacent<'a> {
    fn from(v: BorrowedAdjA<'a>) -> Self {
        Self { text: v.primary }
    }
}

impl<'a> From<BorrowedAdjB<'a>> for BorrowedAdjacent<'a> {
    fn from(v: BorrowedAdjB<'a>) -> Self {
        Self { text: v.fallback }
    }
}

#[test]
fn test_zero_copy_borrowed_str_on_all_paths() {
    // Internal tag-first (v2 & v1)
    let input_i_v2_first = r#"{"version":2,"text":"zero_copy_v2_first","rank":5}"#.to_owned();
    let i_v2_first: BorrowedInternal<'_> = serde_json::from_str(&input_i_v2_first).unwrap();
    assert_eq!(i_v2_first.text, "zero_copy_v2_first");
    assert_borrowed_within(i_v2_first.text, &input_i_v2_first);

    let input_i_v1_first = r#"{"version":1,"text":"zero_copy_v1_first"}"#.to_owned();
    let i_v1_first: BorrowedInternal<'_> = serde_json::from_str(&input_i_v1_first).unwrap();
    assert_eq!(i_v1_first.text, "zero_copy_v1_first");
    assert_borrowed_within(i_v1_first.text, &input_i_v1_first);

    // Internal tag-later (prefix replay + remaining stream)
    let input_i_v2_later = r#"{"text":"zero_copy_v2_later","version":2,"rank":6}"#.to_owned();
    let i_v2_later: BorrowedInternal<'_> = serde_json::from_str(&input_i_v2_later).unwrap();
    assert_eq!(i_v2_later.text, "zero_copy_v2_later");
    assert_borrowed_within(i_v2_later.text, &input_i_v2_later);

    let input_i_v1_last = r#"{"text":"zero_copy_v1_last","version":1}"#.to_owned();
    let i_v1_last: BorrowedInternal<'_> = serde_json::from_str(&input_i_v1_last).unwrap();
    assert_eq!(i_v1_last.text, "zero_copy_v1_last");
    assert_borrowed_within(i_v1_last.text, &input_i_v1_last);

    // Internal single `_` untagged
    let input_i_untagged = r#"{"legacy":"zero_copy_untagged_single"}"#.to_owned();
    let i_untagged: BorrowedInternal<'_> = serde_json::from_str(&input_i_untagged).unwrap();
    assert_eq!(i_untagged.text, "zero_copy_untagged_single");
    assert_borrowed_within(i_untagged.text, &input_i_untagged);

    // Adjacent tag-before-content
    let input_adj_direct = r#"{"version":1,"data":{"text":"zero_copy_adj_direct"}}"#.to_owned();
    let adj_direct: BorrowedAdjacent<'_> = serde_json::from_str(&input_adj_direct).unwrap();
    assert_eq!(adj_direct.text, "zero_copy_adj_direct");
    assert_borrowed_within(adj_direct.text, &input_adj_direct);

    // Adjacent content-before-tag
    let input_adj_rev = r#"{"data":{"text":"zero_copy_adj_rev"},"version":1}"#.to_owned();
    let adj_rev: BorrowedAdjacent<'_> = serde_json::from_str(&input_adj_rev).unwrap();
    assert_eq!(adj_rev.text, "zero_copy_adj_rev");
    assert_borrowed_within(adj_rev.text, &input_adj_rev);

    // Adjacent multiple `_` untagged (arm 1 and arm 2 via ContentRefDeserializer)
    let input_adj_u1 = r#"{"primary":"zero_copy_multi_a","extra":1}"#.to_owned();
    let adj_u1: BorrowedAdjacent<'_> = serde_json::from_str(&input_adj_u1).unwrap();
    assert_eq!(adj_u1.text, "zero_copy_multi_a");
    assert_borrowed_within(adj_u1.text, &input_adj_u1);

    let input_adj_u2 = r#"{"fallback":"zero_copy_multi_b"}"#.to_owned();
    let adj_u2: BorrowedAdjacent<'_> = serde_json::from_str(&input_adj_u2).unwrap();
    assert_eq!(adj_u2.text, "zero_copy_multi_b");
    assert_borrowed_within(adj_u2.text, &input_adj_u2);
}

// =========================================================================
// 5. Generic structs & enums with #[cfg]-gated versions (__BcArm_i path)
// =========================================================================

#[derive(Debug, PartialEq, Eq, DeriveDeserialize)]
pub struct GenericBoxV1<T> {
    pub inner: T,
}

#[backwards_compat(
    versions(
        #[cfg(all())]
        1: GenericBoxV1<T>,
        #[cfg(any())]
        2: GenericBoxV1<T>,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericBox<T> {
    pub inner: T,
    pub note: String,
}

impl<T> From<GenericBoxV1<T>> for GenericBox<T> {
    fn from(v: GenericBoxV1<T>) -> Self {
        Self {
            inner: v.inner,
            note: "from_v1".to_owned(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, DeriveDeserialize, DeriveSerialize)]
pub enum GenericEventV1<'a, T> {
    Item(&'a str, T),
}

#[backwards_compat(
    content = "data",
    versions(
        #[cfg(all())]
        1: GenericEventV1<'a, T>,
        #[cfg(any())]
        2: GenericEventV1<'a, T>,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum GenericEvent<'a, T> {
    Item { key: &'a str, value: T },
    Empty,
}

impl<'a, T> From<GenericEventV1<'a, T>> for GenericEvent<'a, T> {
    fn from(v: GenericEventV1<'a, T>) -> Self {
        match v {
            GenericEventV1::Item(key, value) => Self::Item { key, value },
        }
    }
}

#[test]
fn test_generic_struct_and_enum_with_cfg_gated_versions() {
    // Generic struct: active cfg v1, current v3, disabled cfg v2
    let s_v1: Migrated<GenericBox<u32>> = serde_json::from_str(r#"{"version":1,"inner":99}"#).unwrap();
    assert_eq!(s_v1.source_version, Some(1));
    assert_eq!(
        s_v1.into_inner(),
        GenericBox {
            inner: 99,
            note: "from_v1".to_owned(),
        }
    );

    let s_v3: Migrated<GenericBox<String>> =
        serde_json::from_str(r#"{"inner":"ok","version":3,"note":"direct"}"#).unwrap();
    assert_eq!(s_v3.source_version, Some(3));
    assert_eq!(
        s_v3.into_inner(),
        GenericBox {
            inner: "ok".to_owned(),
            note: "direct".to_owned(),
        }
    );

    let s_v2_err = serde_json::from_str::<GenericBox<u32>>(r#"{"version":2,"inner":1}"#)
        .expect_err("cfg-disabled version 2 should fail");
    assert!(
        s_v2_err.to_string().contains("feature"),
        "expected feature-gate hint for disabled v2, got: {s_v2_err}"
    );

    // Generic lifetime enum: active cfg v1 (zero-copy!), current v3, disabled cfg v2
    let e_v1_input = r#"{"version":1,"data":{"Item":["zero_key",42]}}"#.to_owned();
    let e_v1: Migrated<GenericEvent<'_, u32>> = serde_json::from_str(&e_v1_input).unwrap();
    assert_eq!(e_v1.source_version, Some(1));
    let GenericEvent::Item { key, value } = e_v1.into_inner() else {
        panic!("expected GenericEvent::Item");
    };
    assert_eq!(key, "zero_key");
    assert_eq!(value, 42);
    assert_borrowed_within(key, &e_v1_input);

    let e_v3_input = r#"{"data":{"Item":{"key":"v3_key","value":true}},"version":3}"#.to_owned();
    let e_v3: Migrated<GenericEvent<'_, bool>> = serde_json::from_str(&e_v3_input).unwrap();
    assert_eq!(e_v3.source_version, Some(3));
    let GenericEvent::Item { key: k3, value: v3 } = e_v3.into_inner() else {
        panic!("expected GenericEvent::Item");
    };
    assert_eq!(k3, "v3_key");
    assert!(v3, "expected true");
    assert_borrowed_within(k3, &e_v3_input);

    let e_v2_err = serde_json::from_str::<GenericEvent<'_, u32>>(r#"{"version":2,"data":"Empty"}"#)
        .expect_err("cfg-disabled enum version 2 should fail");
    assert!(
        e_v2_err.to_string().contains("feature"),
        "expected feature-gate hint for disabled enum v2, got: {e_v2_err}"
    );
}
