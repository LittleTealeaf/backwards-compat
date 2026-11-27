use backwards_compat::{BackwardsCompat, BackwardsCompatSerialize as _, Migrated, Tagging, backwards_compat};
use serde::{Deserialize, Serialize};

// =========================================================================
// Helper leaf types: one Deserialize-only, one Serialize-only
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct OnlyDe(String);

#[derive(Debug, PartialEq, Eq, Serialize)]
struct OnlySer(String);

// =========================================================================
// (a) Deserialize-only (`serialize = false`)
// =========================================================================

#[derive(Debug, Deserialize)]
struct DeOnlyNamedV1 {
    raw: String,
}

#[backwards_compat(
    serialize = false,
    versions(
        1: DeOnlyNamedV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct DeOnlyNamed {
    inner: OnlyDe,
    migrated: bool,
}

impl From<DeOnlyNamedV1> for DeOnlyNamed {
    fn from(v: DeOnlyNamedV1) -> Self {
        Self {
            inner: OnlyDe(v.raw),
            migrated: true,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DeOnlyTupleV1(String);

#[backwards_compat(
    serialize = false,
    content = "data",
    versions(
        1: DeOnlyTupleV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct DeOnlyTuple(OnlyDe);

impl From<DeOnlyTupleV1> for DeOnlyTuple {
    fn from(v: DeOnlyTupleV1) -> Self {
        Self(OnlyDe(v.0))
    }
}

#[backwards_compat(
    serialize = false,
    content = "data",
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct DeOnlyUnit;

#[derive(Debug, Deserialize)]
enum DeOnlyEnumV1 {
    Legacy(String),
}

#[backwards_compat(
    serialize = false,
    tag = "v",
    content = "d",
    versions(
        1: DeOnlyEnumV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
enum DeOnlyEnum {
    Unit,
    Tuple(OnlyDe),
    Named { payload: OnlyDe },
}

impl From<DeOnlyEnumV1> for DeOnlyEnum {
    fn from(v: DeOnlyEnumV1) -> Self {
        match v {
            DeOnlyEnumV1::Legacy(s) => Self::Tuple(OnlyDe(s)),
        }
    }
}

#[test]
fn test_serialize_false_named_tuple_unit_enum() {
    // Named struct: v1 -> v2, v2 direct, Migrated<T>, BackwardsCompat constants
    assert_eq!(DeOnlyNamed::CURRENT_VERSION, 2);
    assert_eq!(DeOnlyNamed::SUPPORTED_VERSIONS, &[1, 2]);
    const {
        assert!(!DeOnlyNamed::ACCEPTS_UNTAGGED);
    }
    assert_eq!(DeOnlyNamed::TAGGING, Tagging::Internal { tag: "version" });

    let m1: Migrated<DeOnlyNamed> = serde_json::from_str(r#"{"version":1,"raw":"hello"}"#).unwrap();
    assert_eq!(m1.source_version, Some(1));
    assert_eq!(
        m1.value,
        DeOnlyNamed {
            inner: OnlyDe("hello".to_owned()),
            migrated: true,
        }
    );

    let v2: DeOnlyNamed = serde_json::from_str(r#"{"version":2,"inner":"world","migrated":false}"#).unwrap();
    assert_eq!(
        v2,
        DeOnlyNamed {
            inner: OnlyDe("world".to_owned()),
            migrated: false,
        }
    );

    // Tuple struct
    let t1: DeOnlyTuple = serde_json::from_str(r#"{"version":1,"data":"tup"}"#).unwrap();
    assert_eq!(t1, DeOnlyTuple(OnlyDe("tup".to_owned())));
    let t2: DeOnlyTuple = serde_json::from_str(r#"{"version":2,"data":"tup2"}"#).unwrap();
    assert_eq!(t2, DeOnlyTuple(OnlyDe("tup2".to_owned())));

    // Unit struct
    let u: DeOnlyUnit = serde_json::from_str(r#"{"version":1,"data":null}"#).unwrap();
    assert_eq!(u, DeOnlyUnit);

    // Enum
    let e1: DeOnlyEnum = serde_json::from_str(r#"{"v":1,"d":{"Legacy":"old"}}"#).unwrap();
    assert_eq!(e1, DeOnlyEnum::Tuple(OnlyDe("old".to_owned())));

    let e_unit: DeOnlyEnum = serde_json::from_str(r#"{"v":2,"d":"Unit"}"#).unwrap();
    assert_eq!(e_unit, DeOnlyEnum::Unit);

    let e_named: DeOnlyEnum = serde_json::from_str(r#"{"v":2,"d":{"Named":{"payload":"ok"}}}"#).unwrap();
    assert_eq!(
        e_named,
        DeOnlyEnum::Named {
            payload: OnlyDe("ok".to_owned()),
        }
    );
}

#[derive(Debug, Deserialize)]
struct CustomSerV1 {
    n: u32,
}

#[backwards_compat(
    serialize = false,
    versions(
        1: CustomSerV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct DeOnlyManualSer {
    count: u64,
}

impl From<CustomSerV1> for DeOnlyManualSer {
    fn from(v: CustomSerV1) -> Self {
        Self {
            count: u64::from(v.n),
        }
    }
}

impl Serialize for DeOnlyManualSer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.count * 10)
    }
}

#[backwards_compat(
    serialize = false,
    versions(
        1: CustomSerV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeOnlyDerivedSer {
    #[serde(rename = "totalCount")]
    count: u64,
}

impl From<CustomSerV1> for DeOnlyDerivedSer {
    fn from(v: CustomSerV1) -> Self {
        Self {
            count: u64::from(v.n),
        }
    }
}

#[test]
fn test_serialize_false_with_manual_and_derived_serialize() {
    // Manual impl Serialize coexists with #[backwards_compat(serialize = false)]
    let manual: DeOnlyManualSer = serde_json::from_str(r#"{"version":1,"n":7}"#).unwrap();
    assert_eq!(manual, DeOnlyManualSer { count: 7 });
    assert_eq!(serde_json::to_string(&manual).unwrap(), "70");

    // #[derive(Serialize)] + #[serde(rename = ...)] preserved on domain struct
    let derived_v1: DeOnlyDerivedSer = serde_json::from_str(r#"{"version":1,"n":42}"#).unwrap();
    assert_eq!(derived_v1, DeOnlyDerivedSer { count: 42 });
    assert_eq!(
        serde_json::to_string(&derived_v1).unwrap(),
        r#"{"totalCount":42}"#
    );

    // Shadow wire also forwarded #[serde(rename = "totalCount")] for v2 deserialization
    let derived_v2: DeOnlyDerivedSer = serde_json::from_str(r#"{"version":2,"totalCount":99}"#).unwrap();
    assert_eq!(derived_v2, DeOnlyDerivedSer { count: 99 });
}

// =========================================================================
// (b) Serialize-only (`deserialize = false`)
// =========================================================================

// Historical type with neither Deserialize nor From impl
#[allow(dead_code, reason = "historical schema marker for deserialize = false")]
struct SerOnlyHistorical;

#[backwards_compat(
    deserialize = false,
    versions(
        1: SerOnlyHistorical,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerOnlyNamed {
    inner: OnlySer,
}

#[backwards_compat(
    deserialize = false,
    content = "data",
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerOnlyTuple(OnlySer);

#[backwards_compat(
    deserialize = false,
    content = "data",
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerOnlyUnit;

#[backwards_compat(
    deserialize = false,
    external,
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
enum SerOnlyEnum {
    Unit,
    Tuple(OnlySer),
    Named { payload: OnlySer },
}

#[derive(Serialize)]
struct SerOnlyV1 {
    legacy: String,
}

#[derive(Serialize)]
struct SerOnlyV2 {
    mid: String,
}

fn to_ser_v1(val: &SerOnlyMulti) -> SerOnlyV1 {
    SerOnlyV1 {
        legacy: val.value.0.clone(),
    }
}

fn to_ser_v2(val: &SerOnlyMulti) -> SerOnlyV2 {
    SerOnlyV2 {
        mid: val.value.0.clone(),
    }
}

#[backwards_compat(
    deserialize = false,
    versions(
        #[serializable(with = to_ser_v1)]
        1: SerOnlyV1,
        #[serialize(with = to_ser_v2)]
        2: SerOnlyV2,
        #[current]
        3: Self,
    )
)]
struct SerOnlyMulti {
    value: OnlySer,
}

#[test]
fn test_deserialize_false_named_tuple_unit_enum_and_rolling_serialize() {
    // Internal tagging
    assert_eq!(SerOnlyNamed::CURRENT_VERSION, 2);
    assert_eq!(SerOnlyNamed::SUPPORTED_VERSIONS, &[1, 2]);
    const {
        assert!(!SerOnlyNamed::ACCEPTS_UNTAGGED);
    }

    let named = SerOnlyNamed {
        inner: OnlySer("abc".to_owned()),
    };
    let named_val: serde_json::Value = serde_json::to_value(&named).unwrap();
    assert_eq!(named_val, serde_json::json!({"version": 2, "inner": "abc"}));

    // Adjacent tagging (tuple & unit)
    let tuple = SerOnlyTuple(OnlySer("xyz".to_owned()));
    let tuple_val: serde_json::Value = serde_json::to_value(&tuple).unwrap();
    assert_eq!(tuple_val, serde_json::json!({"version": 1, "data": "xyz"}));

    let unit_val: serde_json::Value = serde_json::to_value(&SerOnlyUnit).unwrap();
    assert_eq!(unit_val, serde_json::json!({"version": 1, "data": null}));

    // External tagging (enum)
    assert_eq!(SerOnlyEnum::TAGGING, Tagging::External);
    assert_eq!(
        serde_json::to_value(&SerOnlyEnum::Unit).unwrap(),
        serde_json::json!({"1": "Unit"})
    );
    assert_eq!(
        serde_json::to_value(SerOnlyEnum::Tuple(OnlySer("t".to_owned()))).unwrap(),
        serde_json::json!({"1": {"Tuple": "t"}})
    );
    assert_eq!(
        serde_json::to_value(SerOnlyEnum::Named {
            payload: OnlySer("p".to_owned()),
        })
        .unwrap(),
        serde_json::json!({"1": {"Named": {"payload": "p"}}})
    );

    // Rolling deployment #[serialize(with = ...)] + #[serializable(with = ...)] + as_version
    assert_eq!(SerOnlyMulti::CURRENT_VERSION, 3);
    assert_eq!(SerOnlyMulti::SERIALIZE_VERSION, 2);
    assert_eq!(SerOnlyMulti::SERIALIZABLE_VERSIONS, &[1, 2, 3]);
    const {
        assert!(!SerOnlyMulti::ACCEPTS_UNTAGGED);
    }

    let multi = SerOnlyMulti {
        value: OnlySer("hello".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(&multi).unwrap(),
        serde_json::json!({"version": 2, "mid": "hello"})
    );
    assert_eq!(
        serde_json::to_value(multi.as_version(1)).unwrap(),
        serde_json::json!({"version": 1, "legacy": "hello"})
    );
    assert_eq!(
        serde_json::to_value(multi.as_version(3)).unwrap(),
        serde_json::json!({"version": 3, "value": "hello"})
    );
}

#[backwards_compat(
    deserialize = false,
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerOnlyManualDe {
    count: u64,
}

impl<'de> Deserialize<'de> for SerOnlyManualDe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u64::deserialize(deserializer)?;
        Ok(Self { count: raw + 1 })
    }
}

#[backwards_compat(
    deserialize = false,
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct SerOnlyDerivedDe {
    #[serde(rename = "totalCount")]
    count: u64,
}

#[test]
fn test_deserialize_false_with_manual_and_derived_deserialize() {
    let manual = SerOnlyManualDe { count: 5 };
    assert_eq!(
        serde_json::to_value(&manual).unwrap(),
        serde_json::json!({"version": 1, "count": 5})
    );
    let parsed_manual: SerOnlyManualDe = serde_json::from_str("4").unwrap();
    assert_eq!(parsed_manual, SerOnlyManualDe { count: 5 });

    let derived = SerOnlyDerivedDe { count: 12 };
    assert_eq!(
        serde_json::to_value(&derived).unwrap(),
        serde_json::json!({"version": 1, "totalCount": 12})
    );
    let parsed_derived: SerOnlyDerivedDe = serde_json::from_str(r#"{"totalCount":12}"#).unwrap();
    assert_eq!(parsed_derived, SerOnlyDerivedDe { count: 12 });
}

// =========================================================================
// (c) Generics, lifetimes, and const generics
// =========================================================================

#[derive(Debug, PartialEq, Eq)]
struct ConstTag<const N: usize>;

impl<const N: usize> Serialize for ConstTag<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(N as u64)
    }
}

impl<'de, const N: usize> Deserialize<'de> for ConstTag<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = usize::deserialize(deserializer)?;
        if v == N {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom("mismatched const N"))
        }
    }
}

#[backwards_compat(
    serialize = false,
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct GenDeOnly<'a, T, const N: usize> {
    tag: &'a str,
    item: T,
    marker: ConstTag<N>,
}

#[backwards_compat(
    deserialize = false,
    content = "data",
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
enum GenSerOnly<'a, T, const N: usize> {
    Borrowed(&'a str),
    Payload { item: T, marker: ConstTag<N> },
}

#[test]
fn test_generics_lifetimes_and_const_generics() {
    let json = r#"{"version":1,"tag":"hello","item":"de-only","marker":3}"#;
    let decoded: GenDeOnly<'_, OnlyDe, 3> = serde_json::from_str(json).unwrap();
    assert_eq!(
        decoded,
        GenDeOnly {
            tag: "hello",
            item: OnlyDe("de-only".to_owned()),
            marker: ConstTag::<3>,
        }
    );

    let borrowed: GenSerOnly<'_, OnlySer, 2> = GenSerOnly::Borrowed("hi");
    assert_eq!(
        serde_json::to_value(&borrowed).unwrap(),
        serde_json::json!({"version": 1, "data": {"Borrowed": "hi"}})
    );

    let payload = GenSerOnly::Payload {
        item: OnlySer("ser-only".to_owned()),
        marker: ConstTag::<2>,
    };
    assert_eq!(
        serde_json::to_value(&payload).unwrap(),
        serde_json::json!({
            "version": 1,
            "data": {"Payload": {"item": "ser-only", "marker": 2}}
        })
    );
}

// =========================================================================
// (d) Proxy redirection (`via`)
// =========================================================================

#[derive(Debug, Deserialize)]
struct ViaProxyDeV1 {
    raw: String,
}

#[backwards_compat(
    serialize = false,
    versions(
        1: ViaProxyDeV1,
        2: Self,
    )
)]
#[derive(Debug)]
struct ViaProxyDe {
    text: OnlyDe,
}

impl From<ViaProxyDeV1> for ViaProxyDe {
    fn from(v: ViaProxyDeV1) -> Self {
        Self { text: OnlyDe(v.raw) }
    }
}

#[backwards_compat(via = ViaProxyDe, serialize = false)]
#[derive(Debug, PartialEq, Eq)]
struct ViaDomainDeOnly {
    text: OnlyDe,
}

impl From<ViaProxyDe> for ViaDomainDeOnly {
    fn from(p: ViaProxyDe) -> Self {
        Self { text: p.text }
    }
}

#[backwards_compat(
    deserialize = false,
    versions(
        1: Self,
    )
)]
struct ViaProxySer {
    text: OnlySer,
}

#[backwards_compat(via = ViaProxySer, deserialize = false)]
struct ViaDomainSerOnly {
    text: String,
}

impl From<&ViaDomainSerOnly> for ViaProxySer {
    fn from(d: &ViaDomainSerOnly) -> Self {
        Self {
            text: OnlySer(d.text.clone()),
        }
    }
}

#[test]
fn test_via_proxy_serialize_and_deserialize_toggles() {
    let m: Migrated<ViaDomainDeOnly> = serde_json::from_str(r#"{"version":1,"raw":"via-de"}"#).unwrap();
    assert_eq!(m.source_version, Some(1));
    assert_eq!(
        m.value,
        ViaDomainDeOnly {
            text: OnlyDe("via-de".to_owned()),
        }
    );

    let d = ViaDomainSerOnly {
        text: "via-ser".to_owned(),
    };
    assert_eq!(
        serde_json::to_value(&d).unwrap(),
        serde_json::json!({"version": 1, "text": "via-ser"})
    );
    assert_eq!(
        serde_json::to_value(d.as_version(1)).unwrap(),
        serde_json::json!({"version": 1, "text": "via-ser"})
    );
}

// =========================================================================
// (e) Combined container + field-level versioning
// =========================================================================

#[derive(Debug, Deserialize)]
struct OnlyDeFieldV1 {
    legacy: String,
}

impl From<OnlyDeFieldV1> for OnlyDe {
    fn from(v: OnlyDeFieldV1) -> Self {
        Self(v.legacy)
    }
}

#[backwards_compat(
    serialize = false,
    versions(
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct CombinedDeOnly {
    #[backwards_compat(versions(1: OnlyDeFieldV1, 2: Self))]
    leaf: OnlyDe,
}

#[allow(dead_code, reason = "historical marker for field-level deserialize = false")]
struct OnlySerFieldV1;

#[backwards_compat(
    deserialize = false,
    versions(
        1: Self,
    )
)]
struct CombinedSerOnly {
    #[backwards_compat(versions(1: OnlySerFieldV1, 2: Self))]
    leaf: OnlySer,
}

#[test]
fn test_combined_container_and_field_level_toggles() {
    let de_v1: CombinedDeOnly =
        serde_json::from_str(r#"{"version":1,"leaf":{"version":1,"data":{"legacy":"migrated-leaf"}}}"#)
            .unwrap();
    assert_eq!(
        de_v1,
        CombinedDeOnly {
            leaf: OnlyDe("migrated-leaf".to_owned()),
        }
    );

    let de_v2: CombinedDeOnly =
        serde_json::from_str(r#"{"version":1,"leaf":{"version":2,"data":"direct-leaf"}}"#).unwrap();
    assert_eq!(
        de_v2,
        CombinedDeOnly {
            leaf: OnlyDe("direct-leaf".to_owned()),
        }
    );

    let ser = CombinedSerOnly {
        leaf: OnlySer("ser-leaf".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(&ser).unwrap(),
        serde_json::json!({
            "version": 1,
            "leaf": {"version": 2, "data": "ser-leaf"}
        })
    );
}

// =========================================================================
// (f) `schemars` feature integration
// =========================================================================

#[cfg(feature = "schemars")]
mod schemars_toggle_tests {
    use backwards_compat::backwards_compat;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, JsonSchema)]
    struct SchemaV1 {
        old: String,
    }

    #[backwards_compat(
        serialize = false,
        versions(
            1: SchemaV1,
            2: Self,
        )
    )]
    #[derive(Debug, PartialEq, Eq, JsonSchema)]
    struct SchemaDeOnly {
        name: String,
    }

    impl From<SchemaV1> for SchemaDeOnly {
        fn from(v: SchemaV1) -> Self {
            Self { name: v.old }
        }
    }

    #[derive(Debug, Serialize, JsonSchema)]
    struct SchemaSerV1 {
        old: String,
    }

    #[backwards_compat(
        deserialize = false,
        versions(
            1: SchemaSerV1,
            2: Self,
        )
    )]
    #[derive(Debug, PartialEq, Eq, JsonSchema)]
    struct SchemaSerOnly {
        name: String,
    }

    #[backwards_compat(via = SchemaDeOnly, serialize = false)]
    #[derive(Debug, PartialEq, Eq, JsonSchema)]
    struct ViaSchemaDeOnly {
        name: String,
    }

    impl From<SchemaDeOnly> for ViaSchemaDeOnly {
        fn from(v: SchemaDeOnly) -> Self {
            Self { name: v.name }
        }
    }

    #[backwards_compat(via = SchemaSerOnly, deserialize = false)]
    #[derive(Debug, PartialEq, Eq, JsonSchema)]
    struct ViaSchemaSerOnly {
        name: String,
    }

    impl From<&ViaSchemaSerOnly> for SchemaSerOnly {
        fn from(v: &ViaSchemaSerOnly) -> Self {
            Self { name: v.name.clone() }
        }
    }

    #[test]
    fn test_schemars_with_serialize_and_deserialize_toggles() {
        let de_schema = serde_json::to_value(schemars::schema_for!(SchemaDeOnly)).unwrap();
        let de_str = de_schema.to_string();
        assert!(de_str.contains("\"old\""));
        assert!(de_str.contains("\"name\""));

        let ser_schema = serde_json::to_value(schemars::schema_for!(SchemaSerOnly)).unwrap();
        let ser_str = ser_schema.to_string();
        assert!(ser_str.contains("\"old\""));
        assert!(ser_str.contains("\"name\""));

        let via_de_schema = serde_json::to_value(schemars::schema_for!(ViaSchemaDeOnly)).unwrap();
        assert_eq!(via_de_schema, de_schema);

        let via_ser_schema = serde_json::to_value(schemars::schema_for!(ViaSchemaSerOnly)).unwrap();
        assert_eq!(via_ser_schema, ser_schema);
    }
}
