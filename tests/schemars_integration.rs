#![cfg(feature = "schemars")]

use backwards_compat::{Migrated, backwards_compat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// =========================================================================
// 1. Internally-tagged struct with multiple versions (`1: V1, 2: Self`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct UserV1 {
    pub name: String,
}

#[backwards_compat(
    versions(
        1: UserV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct SchemaUser {
    pub first_name: String,
    pub active: bool,
}

impl From<UserV1> for SchemaUser {
    fn from(v: UserV1) -> Self {
        Self {
            first_name: v.name,
            active: true,
        }
    }
}

// =========================================================================
// 2. Adjacently-tagged struct and enum (`tag = "type", content = "data"`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct AdjStructV1 {
    pub old_val: u32,
}

#[backwards_compat(
    tag = "type",
    content = "data",
    versions(
        1: AdjStructV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct AdjStruct {
    pub new_val: u64,
    pub label: String,
}

impl From<AdjStructV1> for AdjStruct {
    fn from(v: AdjStructV1) -> Self {
        Self {
            new_val: u64::from(v.old_val),
            label: "migrated".to_owned(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum AdjEnumV1 {
    Ping,
    Val(i32),
}

#[backwards_compat(
    tag = "type",
    content = "data",
    versions(
        "v1": AdjEnumV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub enum AdjEnum {
    Ping,
    Val { amount: i64 },
}

impl From<AdjEnumV1> for AdjEnum {
    fn from(v: AdjEnumV1) -> Self {
        match v {
            AdjEnumV1::Ping => Self::Ping,
            AdjEnumV1::Val(n) => Self::Val { amount: i64::from(n) },
        }
    }
}

// =========================================================================
// 3. Type with untagged `_` entry
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct LegacySettingV0 {
    pub legacy_key: String,
}

#[backwards_compat(
    versions(
        _: LegacySettingV0 => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct SettingWithUntagged {
    pub key: String,
}

impl From<LegacySettingV0> for SettingWithUntagged {
    fn from(v: LegacySettingV0) -> Self {
        Self { key: v.legacy_key }
    }
}

// =========================================================================
// 4. Proxy redirection (`#[backwards_compat(via = ...)]`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct ProxyV1 {
    pub title: String,
}

#[backwards_compat(
    versions(
        1: ProxyV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct DomainProxy {
    pub title: String,
    pub priority: u8,
}

impl From<ProxyV1> for DomainProxy {
    fn from(v: ProxyV1) -> Self {
        Self {
            title: v.title,
            priority: 1,
        }
    }
}

#[backwards_compat(via = DomainProxy)]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct DomainModel {
    pub title: String,
    pub priority: u8,
}

impl From<DomainProxy> for DomainModel {
    fn from(p: DomainProxy) -> Self {
        Self {
            title: p.title,
            priority: p.priority,
        }
    }
}

impl From<&DomainModel> for DomainProxy {
    fn from(m: &DomainModel) -> Self {
        Self {
            title: m.title.clone(),
            priority: m.priority,
        }
    }
}

// =========================================================================
// 5. Field-level `#[backwards_compat]` container
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct EndpointV1 {
    pub host: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl From<EndpointV1> for Endpoint {
    fn from(v: EndpointV1) -> Self {
        Self {
            host: v.host,
            port: 443,
        }
    }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, JsonSchema)]
pub struct ContainerWithScalarField {
    pub service_name: String,

    #[backwards_compat(versions(
        1: u32,
        2: Self,
    ))]
    pub timeout_ms: u64,

    #[backwards_compat(
        tag = "v",
        content = "cfg",
        versions(
            1: EndpointV1,
            2: Self,
        )
    )]
    pub endpoint: Endpoint,
}

fn as_required_strs(val: &Value) -> Vec<&str> {
    val.get("required")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn resolve_ref<'a>(root: &'a Value, entry: &'a Value) -> &'a Value {
    if let Some(ref_str) = entry.get("$ref").and_then(Value::as_str)
        && let Some(name) = ref_str.strip_prefix("#/$defs/")
        && let Some(target) = root.get("$defs").and_then(|d| d.get(name))
    {
        return target;
    }
    entry
}

#[test]
fn test_schemars_internally_tagged_multi_version() {
    let schema = schemars::schema_for!(SchemaUser);
    let val = serde_json::to_value(&schema).unwrap();

    let one_of = val
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("internally-tagged multi-version schema should have oneOf");
    assert_eq!(one_of.len(), 2, "expected 2 version variants in oneOf");

    let v1 = one_of.first().expect("v1 entry");
    assert_eq!(v1.get("type").and_then(Value::as_str), Some("object"));
    assert_eq!(
        v1.pointer("/properties/version/type").and_then(Value::as_str),
        Some("integer")
    );
    assert_eq!(
        v1.pointer("/properties/version/const").and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        v1.pointer("/properties/name").is_some(),
        "v1 schema should contain `name` property"
    );
    let v1_req = as_required_strs(v1);
    assert!(v1_req.contains(&"version"), "v1 required should contain version");
    assert!(v1_req.contains(&"name"), "v1 required should contain name");

    let v2 = one_of.get(1).expect("v2 entry");
    assert_eq!(v2.get("type").and_then(Value::as_str), Some("object"));
    assert_eq!(
        v2.pointer("/properties/version/const").and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        v2.pointer("/properties/first_name").is_some(),
        "v2 schema should contain `first_name` property"
    );
    assert!(
        v2.pointer("/properties/active").is_some(),
        "v2 schema should contain `active` property"
    );
    let v2_req = as_required_strs(v2);
    assert!(v2_req.contains(&"version"), "v2 required should contain version");
    assert!(
        v2_req.contains(&"first_name"),
        "v2 required should contain first_name"
    );
    assert!(v2_req.contains(&"active"), "v2 required should contain active");
}

#[test]
fn test_schemars_adjacently_tagged_struct_and_enum() {
    // Struct with tag = "type", content = "data"
    let struct_val = serde_json::to_value(schemars::schema_for!(AdjStruct)).unwrap();
    let struct_one_of = struct_val
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("adjacently-tagged struct should have oneOf");
    assert_eq!(struct_one_of.len(), 2, "expected 2 variants for AdjStruct");

    for (idx, expected_tag) in [1_u64, 2_u64].into_iter().enumerate() {
        let entry = struct_one_of.get(idx).expect("variant entry");
        assert_eq!(entry.get("type").and_then(Value::as_str), Some("object"));
        assert_eq!(
            entry.pointer("/properties/type/type").and_then(Value::as_str),
            Some("integer")
        );
        assert_eq!(
            entry.pointer("/properties/type/const").and_then(Value::as_u64),
            Some(expected_tag)
        );
        assert!(
            entry.pointer("/properties/data").is_some(),
            "adjacently-tagged entry should have `data` property"
        );
        let req = as_required_strs(entry);
        assert_eq!(
            req,
            vec!["type", "data"],
            "adjacent wrapper should require `type` and `data`"
        );
    }

    // Enum with string tags ("v1", "v2") and tag = "type", content = "data"
    let enum_val = serde_json::to_value(schemars::schema_for!(AdjEnum)).unwrap();
    let enum_one_of = enum_val
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("adjacently-tagged enum should have oneOf");
    assert_eq!(enum_one_of.len(), 2, "expected 2 variants for AdjEnum");

    for (idx, expected_tag) in ["v1", "v2"].into_iter().enumerate() {
        let entry = enum_one_of.get(idx).expect("enum variant entry");
        assert_eq!(entry.get("type").and_then(Value::as_str), Some("object"));
        assert_eq!(
            entry.pointer("/properties/type/type").and_then(Value::as_str),
            Some("string")
        );
        assert_eq!(
            entry.pointer("/properties/type/const").and_then(Value::as_str),
            Some(expected_tag)
        );
        assert!(
            entry.pointer("/properties/data").is_some(),
            "adjacent enum entry should have `data` property"
        );
        let req = as_required_strs(entry);
        assert_eq!(
            req,
            vec!["type", "data"],
            "adjacent enum wrapper should require `type` and `data`"
        );
    }
}

#[test]
fn test_schemars_untagged_fallback_entry() {
    let val = serde_json::to_value(schemars::schema_for!(SettingWithUntagged)).unwrap();
    let one_of = val
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("model with untagged `_` should produce oneOf");
    assert_eq!(
        one_of.len(),
        2,
        "expected tagged v1 + untagged LegacySettingV0 in oneOf"
    );

    // Tagged v1 entry
    let tagged = one_of.first().expect("tagged entry");
    assert_eq!(
        tagged
            .pointer("/properties/version/const")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        tagged.pointer("/properties/key").is_some(),
        "tagged v1 should have `key` property"
    );

    // Untagged `_` entry (resolved from $defs or inline) has `legacy_key` and no `version` tag
    let untagged_raw = one_of.get(1).expect("untagged entry");
    let untagged = resolve_ref(&val, untagged_raw);
    assert!(
        untagged.pointer("/properties/legacy_key").is_some(),
        "untagged entry should resolve to LegacySettingV0 with `legacy_key`"
    );
    assert!(
        untagged.pointer("/properties/version").is_none(),
        "untagged entry must not require a `version` tag property"
    );
    assert!(
        as_required_strs(untagged).contains(&"legacy_key"),
        "untagged entry should require `legacy_key`"
    );
}

#[test]
fn test_schemars_via_proxy_and_migrated_delegation() {
    let proxy_val = serde_json::to_value(schemars::schema_for!(DomainProxy)).unwrap();
    let domain_val = serde_json::to_value(schemars::schema_for!(DomainModel)).unwrap();
    assert_eq!(
        domain_val, proxy_val,
        "`#[backwards_compat(via = DomainProxy)]` should delegate JsonSchema identically to DomainProxy"
    );

    let user_val = serde_json::to_value(schemars::schema_for!(SchemaUser)).unwrap();
    let migrated_val = serde_json::to_value(schemars::schema_for!(Migrated<SchemaUser>)).unwrap();
    assert_eq!(
        migrated_val, user_val,
        "`Migrated<T>` should delegate JsonSchema identically to `T`"
    );
}

#[test]
fn test_schemars_field_level_container() {
    let val = serde_json::to_value(schemars::schema_for!(ContainerWithScalarField)).unwrap();
    assert_eq!(val.get("type").and_then(Value::as_str), Some("object"));
    let req = as_required_strs(&val);
    assert!(
        req.contains(&"timeout_ms"),
        "container should require `timeout_ms`"
    );
    assert!(req.contains(&"endpoint"), "container should require `endpoint`");

    // `timeout_ms` field schema (default tag = "version", content = "data")
    let timeout_prop = val
        .pointer("/properties/timeout_ms")
        .expect("timeout_ms property");
    let timeout_schema = resolve_ref(&val, timeout_prop);
    let timeout_one_of = timeout_schema
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("timeout_ms schema should have oneOf");
    assert_eq!(timeout_one_of.len(), 2, "timeout_ms should have 2 versions");
    assert_eq!(
        timeout_one_of
            .first()
            .and_then(|e| e.pointer("/properties/version/const"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        timeout_one_of
            .get(1)
            .and_then(|e| e.pointer("/properties/version/const"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        timeout_one_of.first().map(as_required_strs),
        Some(vec!["version", "data"])
    );

    // `endpoint` field schema (custom tag = "v", content = "cfg")
    let endpoint_prop = val.pointer("/properties/endpoint").expect("endpoint property");
    let endpoint_schema = resolve_ref(&val, endpoint_prop);
    let endpoint_one_of = endpoint_schema
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("endpoint schema should have oneOf");
    assert_eq!(endpoint_one_of.len(), 2, "endpoint should have 2 versions");

    let ep_v1 = endpoint_one_of.first().expect("endpoint v1");
    assert_eq!(
        ep_v1.pointer("/properties/v/const").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        as_required_strs(ep_v1),
        vec!["v", "cfg"],
        "endpoint v1 should require custom tag `v` and content `cfg`"
    );
    let ep_v1_cfg = resolve_ref(&val, ep_v1.pointer("/properties/cfg").expect("cfg prop"));
    assert!(
        ep_v1_cfg.pointer("/properties/host").is_some(),
        "endpoint v1 cfg should have `host`"
    );

    let ep_v2 = endpoint_one_of.get(1).expect("endpoint v2");
    assert_eq!(
        ep_v2.pointer("/properties/v/const").and_then(Value::as_u64),
        Some(2)
    );
    let ep_v2_cfg = resolve_ref(&val, ep_v2.pointer("/properties/cfg").expect("cfg prop"));
    assert!(
        ep_v2_cfg.pointer("/properties/host").is_some(),
        "endpoint v2 cfg should have `host`"
    );
    assert!(
        ep_v2_cfg.pointer("/properties/port").is_some(),
        "endpoint v2 cfg should have `port`"
    );
}
