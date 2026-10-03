use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// Historical schemas: only implement Deserialize (NOT Serialize)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LegacyV1 {
    pub legacy_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LegacyV2 {
    pub name: String,
    pub count: u32,
}

// Current wire schema: implements Serialize and Deserialize
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyV3 {
    pub name: String,
    pub count: u32,
    pub active: bool,
}

impl From<LegacyV1> for LegacyV2 {
    fn from(v1: LegacyV1) -> Self {
        Self {
            name: v1.legacy_name,
            count: 1,
        }
    }
}

impl From<LegacyV2> for LegacyV3 {
    fn from(v2: LegacyV2) -> Self {
        Self {
            name: v2.name,
            count: v2.count,
            active: true,
        }
    }
}

// Domain model
#[backwards_compat(tag = "schema_v", version = 3, versions(1: LegacyV1, 2: LegacyV2, 3: LegacyV3))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernModel {
    pub name: String,
    pub count: u32,
    pub active: bool,
}

impl From<LegacyV3> for ModernModel {
    fn from(v3: LegacyV3) -> Self {
        Self {
            name: v3.name,
            count: v3.count,
            active: v3.active,
        }
    }
}

impl From<ModernModel> for LegacyV3 {
    fn from(m: ModernModel) -> Self {
        Self {
            name: m.name,
            count: m.count,
            active: m.active,
        }
    }
}

#[test]
fn test_deserialization_from_historical_versions_without_serialize() {
    let json_v1 = r#"{"schema_v": "1", "legacy_name": "v1_item"}"#;
    let model1: ModernModel = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        model1,
        ModernModel {
            name: "v1_item".to_owned(),
            count: 1,
            active: true,
        }
    );

    let json_v2 = r#"{"schema_v": "2", "name": "v2_item", "count": 42}"#;
    let model2: ModernModel = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        model2,
        ModernModel {
            name: "v2_item".to_owned(),
            count: 42,
            active: true,
        }
    );

    let json_v3 = r#"{"schema_v": "3", "name": "v3_item", "count": 100, "active": false}"#;
    let model3: ModernModel = serde_json::from_str(json_v3).unwrap();
    assert_eq!(
        model3,
        ModernModel {
            name: "v3_item".to_owned(),
            count: 100,
            active: false,
        }
    );
}

#[test]
fn test_serialization_emits_current_version_tag_and_fields() {
    let model = ModernModel {
        name: "test_serialize".to_owned(),
        count: 7,
        active: true,
    };
    let serialized = serde_json::to_string(&model).unwrap();
    assert!(serialized.contains(r#""schema_v":"3""#));
    assert!(serialized.contains(r#""name":"test_serialize""#));
    assert!(serialized.contains(r#""count":7"#));
    assert!(serialized.contains(r#""active":true"#));

    // Roundtrip back to ModernModel
    let roundtrip: ModernModel = serde_json::from_str(&serialized).unwrap();
    assert_eq!(roundtrip, model);
}
