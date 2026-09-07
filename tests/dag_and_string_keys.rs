use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. String Literal Version Keys ("0.1", "1.0", "2.0")
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub name: String,
    pub port: u16,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfigV0_1 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfigV1_0 {
    pub name: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfigV2_0 {
    pub name: String,
    pub port: u16,
    pub features: Vec<String>,
}

impl From<AppConfigV0_1> for AppConfigV1_0 {
    fn from(v0: AppConfigV0_1) -> Self {
        Self {
            name: v0.name,
            port: 8080,
        }
    }
}

impl From<AppConfigV1_0> for AppConfigV2_0 {
    fn from(v1: AppConfigV1_0) -> Self {
        Self {
            name: v1.name,
            port: v1.port,
            features: vec!["default".to_string()],
        }
    }
}

impl From<AppConfigV2_0> for AppConfig {
    fn from(v2: AppConfigV2_0) -> Self {
        Self {
            name: v2.name,
            port: v2.port,
            features: v2.features,
        }
    }
}

impl From<AppConfig> for AppConfigV2_0 {
    fn from(cfg: AppConfig) -> Self {
        Self {
            name: cfg.name,
            port: cfg.port,
            features: cfg.features,
        }
    }
}

backwards_compat! {
    #[tag = "version"]
    compat AppConfig {
        "0.1": AppConfigV0_1,
        "1.0": AppConfigV1_0,
        "2.0": AppConfigV2_0,
    }
}

#[test]
fn test_string_keys_json_upgrade_from_v0_1() {
    let raw = r#"{"version": "0.1", "name": "MyApp"}"#;
    let config: AppConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(
        config,
        AppConfig {
            name: "MyApp".to_string(),
            port: 8080,
            features: vec!["default".to_string()],
        }
    );
}

#[test]
fn test_string_keys_json_upgrade_from_v1_0() {
    let raw = r#"{"version": "1.0", "name": "MyApp", "port": 9000}"#;
    let config: AppConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(
        config,
        AppConfig {
            name: "MyApp".to_string(),
            port: 9000,
            features: vec!["default".to_string()],
        }
    );
}

#[test]
fn test_string_keys_json_upgrade_from_v2_0() {
    let raw = r#"{"version": "2.0", "name": "MyApp", "port": 3000, "features": ["auth", "metrics"]}"#;
    let config: AppConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(
        config,
        AppConfig {
            name: "MyApp".to_string(),
            port: 3000,
            features: vec!["auth".to_string(), "metrics".to_string()],
        }
    );
}

#[test]
fn test_string_keys_serialization_emits_target_version() {
    let config = AppConfig {
        name: "CloudService".to_string(),
        port: 443,
        features: vec!["tls".to_string(), "http2".to_string()],
    };
    let json = serde_json::to_string(&config).unwrap();
    assert!(
        json.contains(r#""version":"2.0""#),
        "JSON output should contain target version '2.0': {}",
        json
    );

    let roundtrip: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, roundtrip);
}

#[test]
fn test_string_keys_ron_support() {
    let config = AppConfig {
        name: "RonApp".to_string(),
        port: 8000,
        features: vec!["cache".to_string()],
    };
    let ron_str = ron::to_string(&config).unwrap();
    assert!(
        ron_str.contains("version") && ron_str.contains(r#""2.0""#),
        "RON output should contain version '2.0': {}",
        ron_str
    );

    let roundtrip: AppConfig = ron::from_str(&ron_str).unwrap();
    assert_eq!(config, roundtrip);

    // Also verify deserializing historical versions from RON
    let v0_1_ron = r#"(version: "0.1", name: "OldRon")"#;
    let migrated_v0: AppConfig = ron::from_str(v0_1_ron).unwrap();
    assert_eq!(
        migrated_v0,
        AppConfig {
            name: "OldRon".to_string(),
            port: 8080,
            features: vec!["default".to_string()],
        }
    );

    let v1_0_ron = r#"(version: "1.0", name: "MidRon", port: 5000)"#;
    let migrated_v1: AppConfig = ron::from_str(v1_0_ron).unwrap();
    assert_eq!(
        migrated_v1,
        AppConfig {
            name: "MidRon".to_string(),
            port: 5000,
            features: vec!["default".to_string()],
        }
    );
}

#[test]
fn test_string_keys_toml_support() {
    let config = AppConfig {
        name: "TomlApp".to_string(),
        port: 7000,
        features: vec!["toml_support".to_string()],
    };
    let toml_str = toml::to_string(&config).unwrap();
    assert!(
        toml_str.contains(r#"version = "2.0""#),
        "TOML output should contain version = '2.0': {}",
        toml_str
    );

    let roundtrip: AppConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(config, roundtrip);

    // Also verify deserializing historical versions from TOML
    let v0_1_toml = "version = \"0.1\"\nname = \"OldToml\"\n";
    let migrated_v0: AppConfig = toml::from_str(v0_1_toml).unwrap();
    assert_eq!(
        migrated_v0,
        AppConfig {
            name: "OldToml".to_string(),
            port: 8080,
            features: vec!["default".to_string()],
        }
    );

    let v1_0_toml = "version = \"1.0\"\nname = \"MidToml\"\nport = 5000\n";
    let migrated_v1: AppConfig = toml::from_str(v1_0_toml).unwrap();
    assert_eq!(
        migrated_v1,
        AppConfig {
            name: "MidToml".to_string(),
            port: 5000,
            features: vec!["default".to_string()],
        }
    );
}

// ============================================================================
// 2. Explicit DAG Shortcut / Jump
//    1: V1 => 3 (skips 2: V2)
//    2: V2 => 3
//    3: V3
//    Target: MyModel at version 3.
//    Crucially: V1 does NOT implement From<V1> for V2.
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct MyModel {
    pub message: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JumpV1 {
    pub msg: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JumpV2 {
    pub intermediate_note: String,
    pub intermediate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JumpV3 {
    pub message: String,
    pub count: usize,
}

// NOTE: V1 intentionally does NOT implement From<JumpV1> for JumpV2.
// It directly converts to JumpV3:
impl From<JumpV1> for JumpV3 {
    fn from(v1: JumpV1) -> Self {
        Self {
            message: format!("v1_shortcut: {}", v1.msg),
            count: 42,
        }
    }
}

// V2 converts to JumpV3:
impl From<JumpV2> for JumpV3 {
    fn from(v2: JumpV2) -> Self {
        Self {
            message: format!("v2_step: {}", v2.intermediate_note),
            count: v2.intermediate_count,
        }
    }
}

// V3 converts to MyModel:
impl From<JumpV3> for MyModel {
    fn from(v3: JumpV3) -> Self {
        Self {
            message: v3.message,
            count: v3.count,
        }
    }
}

// MyModel converts to JumpV3 for serialization:
impl From<MyModel> for JumpV3 {
    fn from(m: MyModel) -> Self {
        Self {
            message: m.message,
            count: m.count,
        }
    }
}

backwards_compat! {
    #[tag = "version"]
    compat MyModel {
        1: JumpV1 => 3,
        2: JumpV2 => 3,
        3: JumpV3,
    }
}

#[test]
fn test_dag_jump_from_v1_bypasses_v2() {
    let raw = r#"{"version": "1", "msg": "hello from v1"}"#;
    let model: MyModel = serde_json::from_str(raw).unwrap();
    assert_eq!(
        model,
        MyModel {
            message: "v1_shortcut: hello from v1".to_string(),
            count: 42,
        }
    );
}

#[test]
fn test_dag_jump_from_v2() {
    let raw = r#"{"version": "2", "intermediate_note": "hello from v2", "intermediate_count": 99}"#;
    let model: MyModel = serde_json::from_str(raw).unwrap();
    assert_eq!(
        model,
        MyModel {
            message: "v2_step: hello from v2".to_string(),
            count: 99,
        }
    );
}

#[test]
fn test_dag_jump_from_v3() {
    let raw = r#"{"version": "3", "message": "hello from v3", "count": 7}"#;
    let model: MyModel = serde_json::from_str(raw).unwrap();
    assert_eq!(
        model,
        MyModel {
            message: "hello from v3".to_string(),
            count: 7,
        }
    );
}

#[test]
fn test_dag_jump_serialization_emits_target_version() {
    let model = MyModel {
        message: "hello target".to_string(),
        count: 100,
    };
    let json = serde_json::to_string(&model).unwrap();
    assert!(
        json.contains(r#""version":"3""#),
        "Serialized JSON should contain target version '3': {}",
        json
    );

    let roundtrip: MyModel = serde_json::from_str(&json).unwrap();
    assert_eq!(model, roundtrip);
}

// ============================================================================
// 3. Multi-hop DAG with Fallible Branch
//    Branch A (multi-hop with fallible step):
//      1: TelemetryV1 => 2 (infallible)
//      #[fallible] 2: TelemetryV2 => 4 (fallible: parses string temperature, can fail)
//    Branch B (shortcut):
//      3: TelemetryV3 => 4 (infallible)
//    Terminal:
//      4: TelemetryV4 (infallible to TelemetryEvent)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryEvent {
    pub device_id: String,
    pub timestamp: u64,
    pub temperature_c: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryV1 {
    pub dev_id: u32,
    pub raw_temp: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryV2 {
    pub device_str: String,
    pub temp_str: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryV3 {
    pub id: String,
    pub temp_f: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryV4 {
    pub device_id: String,
    pub timestamp: u64,
    pub temperature_c: f64,
}

// V1 -> V2: Infallible step
impl From<TelemetryV1> for TelemetryV2 {
    fn from(v1: TelemetryV1) -> Self {
        Self {
            device_str: format!("DEV-{}", v1.dev_id),
            temp_str: v1.raw_temp,
        }
    }
}

// V2 -> V4: Fallible step
impl TryFrom<TelemetryV2> for TelemetryV4 {
    type Error = String;

    fn try_from(v2: TelemetryV2) -> Result<Self, Self::Error> {
        let temp: f64 = v2
            .temp_str
            .parse()
            .map_err(|_| format!("Invalid temperature value: '{}'", v2.temp_str))?;

        if temp < -273.15 {
            return Err(format!("Temperature {} is below absolute zero (-273.15 C)", temp));
        }

        Ok(Self {
            device_id: v2.device_str,
            timestamp: 1700000000,
            temperature_c: temp,
        })
    }
}

// V3 -> V4: Infallible shortcut step (Fahrenheit to Celsius)
impl From<TelemetryV3> for TelemetryV4 {
    fn from(v3: TelemetryV3) -> Self {
        let celsius = (v3.temp_f - 32.0) * 5.0 / 9.0;
        Self {
            device_id: v3.id,
            timestamp: 1700000000,
            temperature_c: celsius,
        }
    }
}

// V4 -> TelemetryEvent: Infallible
impl From<TelemetryV4> for TelemetryEvent {
    fn from(v4: TelemetryV4) -> Self {
        Self {
            device_id: v4.device_id,
            timestamp: v4.timestamp,
            temperature_c: v4.temperature_c,
        }
    }
}

// TelemetryEvent -> TelemetryV4: For serialization
impl From<TelemetryEvent> for TelemetryV4 {
    fn from(event: TelemetryEvent) -> Self {
        Self {
            device_id: event.device_id,
            timestamp: event.timestamp,
            temperature_c: event.temperature_c,
        }
    }
}

backwards_compat! {
    #[tag = "version"]
    compat TelemetryEvent {
        1: TelemetryV1 => 2,
        #[fallible]
        2: TelemetryV2 => 4,
        3: TelemetryV3 => 4,
        4: TelemetryV4,
    }
}

#[test]
fn test_fallible_branch_v1_success() {
    // V1 with valid temperature string converts V1 -> V2 -> V4 -> TelemetryEvent
    let raw = r#"{"version": "1", "dev_id": 101, "raw_temp": "25.5"}"#;
    let event: TelemetryEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(
        event,
        TelemetryEvent {
            device_id: "DEV-101".to_string(),
            timestamp: 1700000000,
            temperature_c: 25.5,
        }
    );
}

#[test]
fn test_fallible_branch_v1_failure_invalid_number() {
    // V1 with non-numeric temperature fails during the V2 -> V4 step
    let raw = r#"{"version": "1", "dev_id": 101, "raw_temp": "NOT_A_NUM"}"#;
    let err = serde_json::from_str::<TelemetryEvent>(raw).unwrap_err();
    assert!(
        err.to_string().contains("Invalid temperature value: 'NOT_A_NUM'"),
        "Error message should mention parsing failure: {}",
        err
    );
}

#[test]
fn test_fallible_branch_v1_failure_below_absolute_zero() {
    // V1 with -300.0 C fails during the V2 -> V4 step validation
    let raw = r#"{"version": "1", "dev_id": 101, "raw_temp": "-300.0"}"#;
    let err = serde_json::from_str::<TelemetryEvent>(raw).unwrap_err();
    assert!(
        err.to_string().contains("below absolute zero"),
        "Error message should mention absolute zero: {}",
        err
    );
}

#[test]
fn test_fallible_branch_v2_success() {
    // V2 with valid temperature converts V2 -> V4 -> TelemetryEvent
    let raw = r#"{"version": "2", "device_str": "SENSOR-ALPHA", "temp_str": "100.0"}"#;
    let event: TelemetryEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(
        event,
        TelemetryEvent {
            device_id: "SENSOR-ALPHA".to_string(),
            timestamp: 1700000000,
            temperature_c: 100.0,
        }
    );
}

#[test]
fn test_fallible_branch_v2_failure() {
    // V2 with invalid string fails during the V2 -> V4 step
    let raw = r#"{"version": "2", "device_str": "SENSOR-ALPHA", "temp_str": "CORRUPT"}"#;
    let err = serde_json::from_str::<TelemetryEvent>(raw).unwrap_err();
    assert!(
        err.to_string().contains("Invalid temperature value: 'CORRUPT'"),
        "Error message should mention parsing failure: {}",
        err
    );
}

#[test]
fn test_infallible_branch_v3_success() {
    // V3 shortcut is completely infallible: converts V3 -> V4 -> TelemetryEvent (32 F = 0 C)
    let raw = r#"{"version": "3", "id": "SENSOR-BETA", "temp_f": 32.0}"#;
    let event: TelemetryEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(
        event,
        TelemetryEvent {
            device_id: "SENSOR-BETA".to_string(),
            timestamp: 1700000000,
            temperature_c: 0.0,
        }
    );
}

#[test]
fn test_infallible_v4_success() {
    // V4 directly converts to TelemetryEvent
    let raw = r#"{"version": "4", "device_id": "DIRECT-4", "timestamp": 123456, "temperature_c": 18.2}"#;
    let event: TelemetryEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(
        event,
        TelemetryEvent {
            device_id: "DIRECT-4".to_string(),
            timestamp: 123456,
            temperature_c: 18.2,
        }
    );
}

#[test]
fn test_telemetry_serialization_emits_target_version() {
    let event = TelemetryEvent {
        device_id: "DEV-MAIN".to_string(),
        timestamp: 1700000000,
        temperature_c: 21.0,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        json.contains(r#""version":"4""#),
        "Serialization should emit target version '4': {}",
        json
    );

    let roundtrip: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, roundtrip);
}
