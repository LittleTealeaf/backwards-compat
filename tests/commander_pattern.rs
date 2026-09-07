use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum SchemaError {
    #[error("Validation error: {0}")]
    Validation(String),
}

// V1: Legacy untagged format with string fields and floats
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaV1 {
    pub players: Vec<String>,
    pub score: f64,
}

// V2: Untagged format with player counts and rounded score
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaV2 {
    pub player_count: usize,
    pub score: u32,
}

impl From<SchemaV1> for SchemaV2 {
    fn from(v1: SchemaV1) -> Self {
        Self {
            player_count: v1.players.len(),
            score: v1.score.round() as u32,
        }
    }
}

// V3: Can be tagged (version "3") OR untagged legacy!
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaV3 {
    pub player_count: usize,
    pub score: u32,
    pub active: bool,
}

impl From<SchemaV2> for SchemaV3 {
    fn from(v2: SchemaV2) -> Self {
        Self {
            player_count: v2.player_count,
            score: v2.score,
            active: true,
        }
    }
}

// V4: Tagged only (version "4")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaV4 {
    pub player_count: usize,
    pub score: u32,
    pub active: bool,
    pub tag: String,
}

impl From<SchemaV3> for SchemaV4 {
    fn from(v3: SchemaV3) -> Self {
        Self {
            player_count: v3.player_count,
            score: v3.score,
            active: v3.active,
            tag: "migrated".to_string(),
        }
    }
}

// V5: Tagged only (version "5")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaV5 {
    pub player_count: usize,
    pub score: u32,
    pub active: bool,
    pub tag: String,
    pub revision: usize,
}

impl From<SchemaV4> for SchemaV5 {
    fn from(v4: SchemaV4) -> Self {
        Self {
            player_count: v4.player_count,
            score: v4.score,
            active: v4.active,
            tag: v4.tag,
            revision: 1,
        }
    }
}

// Domain Model Target Struct
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SerializedSchema", into = "SerdeSchema")]
pub struct FinalModel {
    pub player_count: usize,
    pub score: u32,
    pub active: bool,
    pub tag: String,
    pub revision: usize,
}

impl TryFrom<SchemaV5> for FinalModel {
    type Error = SchemaError;

    fn try_from(v5: SchemaV5) -> Result<Self, Self::Error> {
        if v5.player_count > 100 {
            return Err(SchemaError::Validation("Too many players".to_string()));
        }
        Ok(Self {
            player_count: v5.player_count,
            score: v5.score,
            active: v5.active,
            tag: v5.tag,
            revision: v5.revision,
        })
    }
}

impl From<FinalModel> for SchemaV5 {
    fn from(m: FinalModel) -> Self {
        Self {
            player_count: m.player_count,
            score: m.score,
            active: m.active,
            tag: m.tag,
            revision: m.revision,
        }
    }
}

backwards_compat! {
    #[tag = "version"]
    #[target(FinalModel, error = SchemaError)]
    #[tagged = SerdeSchema]
    #[untagged = SerializedSchema]
    pub enum SerializedSchema {
        #[untagged_only]
        v1 = SchemaV1,
        #[untagged_only]
        v2 = SchemaV2,
        #[untagged]
        v3 = SchemaV3,
        v4 = SchemaV4,
        #[fallible]
        v5 = SchemaV5,
    }
}

#[test]
fn test_v1_untagged_upgrade() {
    let ron_v1 = r#"
        (
            players: ["Alice", "Bob", "Charlie", "Dave"],
            score: 42.4,
        )
    "#;
    let serialized: SerializedSchema = ron::from_str(ron_v1).unwrap();
    let model: FinalModel = serialized.upgrade().unwrap();
    assert_eq!(model.player_count, 4);
    assert_eq!(model.score, 42);
    assert!(model.active);
    assert_eq!(model.tag, "migrated");
    assert_eq!(model.revision, 1);
}

#[test]
fn test_v2_untagged_upgrade() {
    let json_v2 = r#"{"player_count": 8, "score": 100}"#;
    let serialized: SerializedSchema = serde_json::from_str(json_v2).unwrap();
    let model: FinalModel = serialized.upgrade().unwrap();
    assert_eq!(model.player_count, 8);
    assert_eq!(model.score, 100);
    assert!(model.active);
    assert_eq!(model.tag, "migrated");
}

#[test]
fn test_v3_untagged_upgrade() {
    let ron_v3_untagged = r#"(player_count: 6, score: 88, active: false)"#;
    let serialized: SerializedSchema = ron::from_str(ron_v3_untagged).unwrap();
    let model: FinalModel = serialized.upgrade().unwrap();
    assert_eq!(model.player_count, 6);
    assert_eq!(model.score, 88);
    assert!(!model.active);
}

#[test]
fn test_v3_tagged_upgrade() {
    let json_v3_tagged = r#"{"version": "3", "player_count": 5, "score": 75, "active": true}"#;
    let serialized: SerializedSchema = serde_json::from_str(json_v3_tagged).unwrap();
    let model: FinalModel = serialized.upgrade().unwrap();
    assert_eq!(model.player_count, 5);
    assert_eq!(model.score, 75);
}

#[test]
fn test_v4_tagged_upgrade() {
    let json_v4 =
        r#"{"version": "4", "player_count": 4, "score": 50, "active": true, "tag": "custom"}"#;
    let serialized: SerializedSchema = serde_json::from_str(json_v4).unwrap();
    let model: FinalModel = serialized.upgrade().unwrap();
    assert_eq!(model.player_count, 4);
    assert_eq!(model.tag, "custom");
}

#[test]
fn test_v5_tagged_upgrade() {
    let json_v5 = r#"{"version": "5", "player_count": 10, "score": 200, "active": false, "tag": "v5", "revision": 3}"#;
    let model: FinalModel = serde_json::from_str(json_v5).unwrap();
    assert_eq!(model.player_count, 10);
    assert_eq!(model.revision, 3);
}

#[test]
fn test_target_serde_roundtrip() {
    let model = FinalModel {
        player_count: 12,
        score: 150,
        active: true,
        tag: "pro".to_string(),
        revision: 2,
    };
    let json = serde_json::to_string(&model).unwrap();
    assert!(json.contains(r#""version":"5""#));

    let restored: FinalModel = serde_json::from_str(&json).unwrap();
    assert_eq!(model, restored);
}

#[test]
fn test_validation_error_propagation() {
    let json_invalid = r#"{"version": "5", "player_count": 150, "score": 10, "active": true, "tag": "too-many", "revision": 1}"#;
    let result: Result<FinalModel, _> = serde_json::from_str(json_invalid);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Too many players"));
}
