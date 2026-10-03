use backwards_compat::backwards_compat;
use serde::Deserialize;

// =========================================================================
// 1. Generic struct with lifetime parameter and type parameter
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenericConfigV1<'a, T> {
    pub name: &'a str,
    pub value: T,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(1: GenericConfigV1<'a, T>)
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericConfig<'a, T: Clone + core::fmt::Debug> {
    pub name: &'a str,
    pub value: T,
    pub active: bool,
}

impl<'a, T: Clone + core::fmt::Debug> From<GenericConfigV1<'a, T>> for GenericConfig<'a, T> {
    fn from(v1: GenericConfigV1<'a, T>) -> Self {
        Self {
            name: v1.name,
            value: v1.value,
            active: true,
        }
    }
}

#[test]
fn test_generic_config_with_lifetime_deserialization() {
    let json_v1 = r#"{"version": "1", "name": "alpha", "value": 42}"#;
    let config: GenericConfig<'_, i32> = serde_json::from_str(json_v1).unwrap();
    assert_eq!(config.name, "alpha");
    assert_eq!(config.value, 42);
    assert!(config.active);

    let json_v2 = r#"{"version": "2", "name": "beta", "value": 99, "active": false}"#;
    let config2: GenericConfig<'_, i32> = serde_json::from_str(json_v2).unwrap();
    assert_eq!(config2.name, "beta");
    assert_eq!(config2.value, 99);
    assert!(!config2.active);
}

#[test]
fn test_generic_config_with_lifetime_serialization() {
    let config = GenericConfig {
        name: "service",
        value: "running".to_owned(),
        active: true,
    };
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""version":"2""#));
    assert!(json.contains(r#""name":"service""#));
    assert!(json.contains(r#""value":"running""#));
    assert!(json.contains(r#""active":true"#));
}

// =========================================================================
// 2. Adjacent tagging for tuple structs and non-map scalar representations
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct ScalarV1(pub u32);

#[backwards_compat(
    tag = "t",
    content = "c",
    version = 2,
    versions(1: ScalarV1)
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ScalarTarget(pub String);

impl From<ScalarV1> for ScalarTarget {
    fn from(v1: ScalarV1) -> Self {
        Self(v1.0.to_string())
    }
}

#[test]
fn test_adjacent_tagging_tuple_struct_upgrade() {
    let json_v1 = r#"{"t": "1", "c": 42}"#;
    let target: ScalarTarget = serde_json::from_str(json_v1).unwrap();
    assert_eq!(target.0, "42");

    let json_v2 = r#"{"t": "2", "c": "hello"}"#;
    let target2: ScalarTarget = serde_json::from_str(json_v2).unwrap();
    assert_eq!(target2.0, "hello");
}

#[test]
fn test_adjacent_tagging_serialization() {
    let target = ScalarTarget("world".to_owned());
    let json = serde_json::to_string(&target).unwrap();
    assert_eq!(json, r#"{"t":"2","c":"world"}"#);
}

// =========================================================================
// 3. Adjacent tagging with fallible DAG transitions
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct NumberPayloadV1(pub i64);

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct NumberPayloadV2 {
    pub value: i64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NumberError {
    #[error("Value out of range")]
    OutOfRange,
}

#[backwards_compat(
    tag = "schema",
    content = "payload",
    version = 3,
    error = NumberError,
    versions(
        #[r#try] 1: NumberPayloadV1 => 3,
        2: NumberPayloadV2 => 3,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NumberModel {
    pub value: u32,
}

impl TryFrom<NumberPayloadV1> for NumberModel {
    type Error = NumberError;
    fn try_from(v1: NumberPayloadV1) -> Result<Self, Self::Error> {
        let value = u32::try_from(v1.0).map_err(|_| NumberError::OutOfRange)?;
        Ok(Self { value })
    }
}

impl From<NumberPayloadV2> for NumberModel {
    fn from(v2: NumberPayloadV2) -> Self {
        Self {
            value: u32::try_from(v2.value).unwrap_or(0),
        }
    }
}

#[test]
fn test_adjacent_tagging_fallible_dag() {
    let json_v1_valid = r#"{"schema": "1", "payload": 100}"#;
    let model: NumberModel = serde_json::from_str(json_v1_valid).unwrap();
    assert_eq!(model.value, 100);

    let json_v1_invalid = r#"{"schema": "1", "payload": -5}"#;
    let _ = serde_json::from_str::<NumberModel>(json_v1_invalid).unwrap_err();

    let json_v2 = r#"{"schema": "2", "payload": {"value": 200}}"#;
    let model_v2: NumberModel = serde_json::from_str(json_v2).unwrap();
    assert_eq!(model_v2.value, 200);

    let model_v3 = NumberModel { value: 300 };
    let json_v3 = serde_json::to_string(&model_v3).unwrap();
    assert_eq!(json_v3, r#"{"schema":"3","payload":{"value":300}}"#);
}
