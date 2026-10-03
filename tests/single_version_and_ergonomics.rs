#![deny(missing_debug_implementations)]

use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// =========================================================================
// Test 1: Single Version / Standalone Initial Model (version = 1, no versions(...))
// =========================================================================

#[backwards_compat(tag = "v", version = 1)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleVersionConfig {
    pub host: String,
    pub port: u16,
}

#[test]
fn test_single_version_implicit_wire() {
    let cfg = SingleVersionConfig {
        host: "localhost".into(),
        port: 8080,
    };

    // Should serialize with tag v = 1
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains(r#""v":"1""#));
    assert!(json.contains(r#""host":"localhost""#));
    assert!(json.contains(r#""port":8080"#));

    // Should deserialize back to SingleVersionConfig
    let deserialized: SingleVersionConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, cfg);
}

#[backwards_compat]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultEmptyAttrConfig {
    pub name: String,
}

#[test]
fn test_default_empty_attr_implicit_wire() {
    let cfg = DefaultEmptyAttrConfig {
        name: "test-service".into(),
    };

    // Defaults to tag = "version", version = 1
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains(r#""version":"1""#));
    assert!(json.contains(r#""name":"test-service""#));

    let deserialized: DefaultEmptyAttrConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, cfg);
}

// =========================================================================
// Test 2: Missing Debug Implementations Lint on Public Model
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PublicModelV1 {
    pub name: String,
}

#[backwards_compat(tag = "v", version = 2, versions(1: PublicModelV1))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicModel {
    pub name: String,
    pub count: usize,
}

impl From<PublicModelV1> for PublicModel {
    fn from(v1: PublicModelV1) -> Self {
        Self {
            name: v1.name,
            count: 0,
        }
    }
}

#[test]
fn test_public_model_debug_implemented() {
    let m = PublicModel {
        name: "test".into(),
        count: 5,
    };
    assert_eq!(format!("{m:?}"), "PublicModel { name: \"test\", count: 5 }");
}

// =========================================================================
// Test 3: Serde Field Attribute Forwarding with Implicit Target
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AliasV1 {
    pub old_key: String,
}

const fn default_rate() -> f64 {
    1.0
}

#[backwards_compat(tag = "v", version = 2, versions(1: AliasV1))]
#[derive(Debug, Clone, PartialEq)]
pub struct AliasTarget {
    #[serde(alias = "legacy_name", rename = "current_name")]
    pub name: String,
    #[serde(default = "default_rate")]
    pub rate: f64,
}

impl From<AliasV1> for AliasTarget {
    fn from(v1: AliasV1) -> Self {
        Self {
            name: v1.old_key,
            rate: 1.0,
        }
    }
}

#[test]
fn test_serde_alias_and_rename_on_shadow_wire() {
    // 1. Deserializing using alias 'legacy_name' for v2 payload
    let json_alias = r#"{"v": "2", "legacy_name": "alpha"}"#;
    let target: AliasTarget = serde_json::from_str(json_alias).unwrap();
    assert_eq!(target.name, "alpha");
    assert_eq!(target.rate, 1.0);

    // 2. Serializing uses rename 'current_name'
    let json_out = serde_json::to_string(&target).unwrap();
    assert!(json_out.contains(r#""current_name":"alpha""#));
}

// =========================================================================
// Test 4: Existing Serde Derives on Decorated Struct
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictingV1 {
    pub data: String,
}

#[backwards_compat(tag = "v", version = 2, versions(1: ConflictingV1))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictingModel {
    pub data: String,
    pub extra: bool,
}

impl From<ConflictingV1> for ConflictingModel {
    fn from(v1: ConflictingV1) -> Self {
        Self {
            data: v1.data,
            extra: false,
        }
    }
}

#[test]
fn test_struct_with_redundant_serde_derives() {
    let model = ConflictingModel {
        data: "hello".into(),
        extra: true,
    };
    let json = serde_json::to_string(&model).unwrap();
    assert!(json.contains(r#""v":"2""#));
    assert!(json.contains(r#""data":"hello""#));

    let deserialized: ConflictingModel = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, model);

    let v1_json = r#"{"v": "1", "data": "legacy"}"#;
    let from_v1: ConflictingModel = serde_json::from_str(v1_json).unwrap();
    assert_eq!(
        from_v1,
        ConflictingModel {
            data: "legacy".into(),
            extra: false,
        }
    );
}

// =========================================================================
// Test 5: Transitive Multi-Hop In-Code Conversions
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChainV1 {
    pub val: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChainV2 {
    pub val: u64,
}

impl From<ChainV1> for ChainV2 {
    fn from(v1: ChainV1) -> Self {
        Self {
            val: u64::from(v1.val),
        }
    }
}

#[backwards_compat(tag = "v", version = 3, versions(1: ChainV1 => 2, 2: ChainV2 => 3))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainTarget {
    pub val: u128,
}

impl From<ChainV2> for ChainTarget {
    fn from(v2: ChainV2) -> Self {
        Self {
            val: u128::from(v2.val),
        }
    }
}

#[test]
fn test_transitive_from_and_into() {
    let v1 = ChainV1 { val: 42 };

    // Explicit From
    let target_from = ChainTarget::from(v1.clone());
    assert_eq!(target_from.val, 42);

    // Into
    let target_into: ChainTarget = v1.into();
    assert_eq!(target_into.val, 42);
}
