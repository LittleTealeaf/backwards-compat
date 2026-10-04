use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 1. Sequential duplicates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V1Config {
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V2Config {
    pub host: String,
    pub port: u16,
}

impl From<V1Config> for V2Config {
    fn from(v1: V1Config) -> Self {
        Self {
            host: v1.host,
            port: 8080,
        }
    }
}

#[backwards_compat(
    tag = "version",
    version = 3,
    versions(1: V1Config, 2: V1Config, 3: V2Config)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub port: u16,
}

impl From<V2Config> for Config {
    fn from(v2: V2Config) -> Self {
        Self {
            host: v2.host,
            port: v2.port,
        }
    }
}

impl From<Config> for V2Config {
    fn from(c: Config) -> Self {
        Self {
            host: c.host,
            port: c.port,
        }
    }
}

#[test]
fn test_sequential_duplicates_deserialization_v1() {
    let json = r#"{"version": "1", "host": "localhost"}"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(
        config,
        Config {
            host: "localhost".to_owned(),
            port: 8080,
        }
    );
}

#[test]
fn test_sequential_duplicates_deserialization_v2() {
    let json = r#"{"version": "2", "host": "localhost"}"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(
        config,
        Config {
            host: "localhost".to_owned(),
            port: 8080,
        }
    );
}

#[test]
fn test_sequential_duplicates_deserialization_v3() {
    let json = r#"{"version": "3", "host": "remote", "port": 9000}"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(
        config,
        Config {
            host: "remote".to_owned(),
            port: 9000,
        }
    );
}

#[test]
fn test_sequential_duplicates_serialization_uses_active_version() {
    let config = Config {
        host: "example.com".to_owned(),
        port: 443,
    };
    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(json, r#"{"version":"3","host":"example.com","port":443}"#);
}

// ---------------------------------------------------------------------------
// 2. Alternating duplicates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V1Alt {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V2Alt {
    pub full_name: String,
}

impl From<V2Alt> for V1Alt {
    fn from(v2: V2Alt) -> Self {
        Self { name: v2.full_name }
    }
}

#[backwards_compat(
    tag = "schema_version",
    version = 3,
    versions(1: V1Alt, 2: V2Alt, 3: V1Alt)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigAlt {
    pub name: String,
}

impl From<V1Alt> for ConfigAlt {
    fn from(v1: V1Alt) -> Self {
        Self { name: v1.name }
    }
}

impl From<ConfigAlt> for V1Alt {
    fn from(c: ConfigAlt) -> Self {
        Self { name: c.name }
    }
}

#[test]
fn test_alternating_duplicates_deserialization_v1() {
    let json = r#"{"schema_version": "1", "name": "Alice"}"#;
    let cfg: ConfigAlt = serde_json::from_str(json).unwrap();
    assert_eq!(
        cfg,
        ConfigAlt {
            name: "Alice".to_owned()
        }
    );
}

#[test]
fn test_alternating_duplicates_deserialization_v2() {
    let json = r#"{"schema_version": "2", "full_name": "Bob"}"#;
    let cfg: ConfigAlt = serde_json::from_str(json).unwrap();
    assert_eq!(
        cfg,
        ConfigAlt {
            name: "Bob".to_owned()
        }
    );
}

#[test]
fn test_alternating_duplicates_deserialization_v3() {
    let json = r#"{"schema_version": "3", "name": "Charlie"}"#;
    let cfg: ConfigAlt = serde_json::from_str(json).unwrap();
    assert_eq!(
        cfg,
        ConfigAlt {
            name: "Charlie".to_owned()
        }
    );
}

#[test]
fn test_alternating_duplicates_serialization() {
    let cfg = ConfigAlt {
        name: "Dave".to_owned(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json, r#"{"schema_version":"3","name":"Dave"}"#);
}

// ---------------------------------------------------------------------------
// 3. Fallible transitions with duplicate types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallibleV1 {
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallibleV2 {
    pub count: u32,
}

impl TryFrom<FallibleV1> for FallibleV2 {
    type Error = String;

    fn try_from(v1: FallibleV1) -> Result<Self, Self::Error> {
        if v1.count < 0 {
            Err("count cannot be negative".to_owned())
        } else {
            Ok(Self {
                count: v1.count.cast_unsigned(),
            })
        }
    }
}

#[backwards_compat(
    tag = "v",
    version = 3,
    error = String,
    versions(
        1: FallibleV1,
        #[fallible]
        2: FallibleV1,
        3: FallibleV2
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallibleModel {
    pub count: u32,
}

impl From<FallibleV2> for FallibleModel {
    fn from(v2: FallibleV2) -> Self {
        Self { count: v2.count }
    }
}

impl From<FallibleModel> for FallibleV2 {
    fn from(m: FallibleModel) -> Self {
        Self { count: m.count }
    }
}

#[test]
fn test_fallible_duplicate_types_v1_success() {
    let json = r#"{"v": "1", "count": 42}"#;
    let model: FallibleModel = serde_json::from_str(json).unwrap();
    assert_eq!(model, FallibleModel { count: 42 });
}

#[test]
fn test_fallible_duplicate_types_v1_error() {
    let json = r#"{"v": "1", "count": -5}"#;
    let err = serde_json::from_str::<FallibleModel>(json).unwrap_err();
    assert!(err.to_string().contains("count cannot be negative"));
}

#[test]
fn test_fallible_duplicate_types_v2_success() {
    let json = r#"{"v": "2", "count": 100}"#;
    let model: FallibleModel = serde_json::from_str(json).unwrap();
    assert_eq!(model, FallibleModel { count: 100 });
}

#[test]
fn test_fallible_duplicate_types_v2_error() {
    let json = r#"{"v": "2", "count": -1}"#;
    let err = serde_json::from_str::<FallibleModel>(json).unwrap_err();
    assert!(err.to_string().contains("count cannot be negative"));
}

#[test]
fn test_fallible_duplicate_types_v3_success() {
    let json = r#"{"v": "3", "count": 200}"#;
    let model: FallibleModel = serde_json::from_str(json).unwrap();
    assert_eq!(model, FallibleModel { count: 200 });
}

// ---------------------------------------------------------------------------
// 4. Adjacent tagging with duplicate types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjV1 {
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjV2 {
    pub value: u32,
    pub label: String,
}

impl From<AdjV1> for AdjV2 {
    fn from(v1: AdjV1) -> Self {
        Self {
            value: v1.value,
            label: "default".to_owned(),
        }
    }
}

#[backwards_compat(
    tag = "type",
    content = "data",
    version = 3,
    versions(1: AdjV1, 2: AdjV1, 3: AdjV2)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjModel {
    pub value: u32,
    pub label: String,
}

impl From<AdjV2> for AdjModel {
    fn from(v2: AdjV2) -> Self {
        Self {
            value: v2.value,
            label: v2.label,
        }
    }
}

impl From<AdjModel> for AdjV2 {
    fn from(m: AdjModel) -> Self {
        Self {
            value: m.value,
            label: m.label,
        }
    }
}

#[test]
fn test_adjacent_tagging_duplicate_types_v1() {
    let json = r#"{"type": "1", "data": {"value": 10}}"#;
    let model: AdjModel = serde_json::from_str(json).unwrap();
    assert_eq!(
        model,
        AdjModel {
            value: 10,
            label: "default".to_owned(),
        }
    );
}

#[test]
fn test_adjacent_tagging_duplicate_types_v2() {
    let json = r#"{"type": "2", "data": {"value": 20}}"#;
    let model: AdjModel = serde_json::from_str(json).unwrap();
    assert_eq!(
        model,
        AdjModel {
            value: 20,
            label: "default".to_owned(),
        }
    );
}

#[test]
fn test_adjacent_tagging_duplicate_types_v3() {
    let json = r#"{"type": "3", "data": {"value": 30, "label": "custom"}}"#;
    let model: AdjModel = serde_json::from_str(json).unwrap();
    assert_eq!(
        model,
        AdjModel {
            value: 30,
            label: "custom".to_owned(),
        }
    );
}

#[test]
fn test_adjacent_tagging_duplicate_types_serialization() {
    let model = AdjModel {
        value: 40,
        label: "serialized".to_owned(),
    };
    let json = serde_json::to_string(&model).unwrap();
    assert_eq!(json, r#"{"type":"3","data":{"value":40,"label":"serialized"}}"#);
}

// ---------------------------------------------------------------------------
// 5. Transitive From/Into and format support with duplicate types
// ---------------------------------------------------------------------------

#[test]
fn test_transitive_from_conversions_with_duplicate_types() {
    let v1 = V1Config {
        host: "auto.example.com".to_owned(),
    };
    let config: Config = v1.into();
    assert_eq!(
        config,
        Config {
            host: "auto.example.com".to_owned(),
            port: 8080,
        }
    );

    let v2_alt = V2Alt {
        full_name: "Transitive Alt".to_owned(),
    };
    let config_alt: ConfigAlt = v2_alt.into();
    assert_eq!(
        config_alt,
        ConfigAlt {
            name: "Transitive Alt".to_owned(),
        }
    );
}

#[test]
fn test_ron_support_with_duplicate_types() {
    let raw_ron_v1 = r#"{"version": "1", "host": "ron.example.com"}"#;
    let config: Config = ron::from_str(raw_ron_v1).unwrap();
    assert_eq!(
        config,
        Config {
            host: "ron.example.com".to_owned(),
            port: 8080,
        }
    );

    let raw_ron_v2 = r#"{"version": "2", "host": "ron2.example.com"}"#;
    let config2: Config = ron::from_str(raw_ron_v2).unwrap();
    assert_eq!(
        config2,
        Config {
            host: "ron2.example.com".to_owned(),
            port: 8080,
        }
    );
}

#[test]
fn test_toml_support_with_duplicate_types() {
    let raw_toml_v1 = r#"
version = "1"
host = "toml.example.com"
"#;
    let config: Config = toml::from_str(raw_toml_v1).unwrap();
    assert_eq!(
        config,
        Config {
            host: "toml.example.com".to_owned(),
            port: 8080,
        }
    );

    let raw_toml_v2 = r#"
version = "2"
host = "toml2.example.com"
"#;
    let config2: Config = toml::from_str(raw_toml_v2).unwrap();
    assert_eq!(
        config2,
        Config {
            host: "toml2.example.com".to_owned(),
            port: 8080,
        }
    );
}
