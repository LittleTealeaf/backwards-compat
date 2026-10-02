use backwards_compat::backwards_compat;
use serde::Deserialize;
use thiserror::Error;

fn default_port() -> u16 {
    8080
}

fn default_timeout() -> u64 {
    30
}

// -----------------------------------------------------------------------------
// 1. Named struct implicit target with serde attributes:
//    - Field-level: #[serde(rename = "...")]
//    - Field-level: #[serde(default = "...")]
//    - Field-level: #[serde(default)]
//    - Container-level: #[serde(rename_all = "kebab-case")]
//    - Multiple historical versions: ServerConfigV1 -> ServerConfigV2 -> ServerConfig
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServerConfigV1 {
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServerConfigV2 {
    pub host: String,
    pub port: u16,
}

impl From<ServerConfigV1> for ServerConfigV2 {
    fn from(v1: ServerConfigV1) -> Self {
        Self {
            host: v1.host,
            port: 80,
        }
    }
}

#[backwards_compat(
    tag = "schema_version",
    version = 3,
    versions(
        1: ServerConfigV1 => 2,
        2: ServerConfigV2 => 3,
    )
)]
#[derive(Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct ServerConfig {
    #[serde(rename = "service-name")]
    pub name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub use_tls: bool,
    pub active_endpoints: Vec<String>,
}

impl From<ServerConfigV2> for ServerConfig {
    fn from(v2: ServerConfigV2) -> Self {
        Self {
            name: v2.host,
            port: v2.port,
            timeout_seconds: 30,
            use_tls: true,
            active_endpoints: vec!["/health".to_string()],
        }
    }
}

// -----------------------------------------------------------------------------
// 2. Fallible multi-step chain into implicit target struct
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MetricV1 {
    pub metric_str: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MetricV2 {
    pub raw_val: i64,
}

#[derive(Debug, Error)]
pub enum MetricMigrationError {
    #[error("failed to parse metric value: {0}")]
    Parse(#[from] std::num::ParseIntError),
    #[error("metric value cannot be negative: {0}")]
    Negative(i64),
}

impl TryFrom<MetricV1> for MetricV2 {
    type Error = MetricMigrationError;
    fn try_from(v1: MetricV1) -> Result<Self, Self::Error> {
        let val = v1.metric_str.parse::<i64>()?;
        Ok(Self { raw_val: val })
    }
}

#[backwards_compat(
    tag = "v",
    version = 3,
    error = MetricMigrationError,
    versions(
        #[fallible]
        1: MetricV1 => 2,
        #[fallible]
        2: MetricV2 => 3,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub value: u64,
    #[serde(default)]
    pub unit: String,
}

impl TryFrom<MetricV2> for Metric {
    type Error = MetricMigrationError;
    fn try_from(v2: MetricV2) -> Result<Self, Self::Error> {
        if v2.raw_val < 0 {
            return Err(MetricMigrationError::Negative(v2.raw_val));
        }
        Ok(Self {
            value: v2.raw_val as u64,
            unit: "count".to_string(),
        })
    }
}

// -----------------------------------------------------------------------------
// 3. String version tags with multiple historical versions into implicit target
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DatabaseConfigV1 {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DatabaseConfigV2 {
    pub host: String,
    pub port: u16,
}

impl From<DatabaseConfigV1> for DatabaseConfigV2 {
    fn from(v1: DatabaseConfigV1) -> Self {
        Self {
            host: v1.url,
            port: 5432,
        }
    }
}

#[backwards_compat(
    tag = "api_version",
    version = "v3.0",
    versions(
        "v1.0": DatabaseConfigV1 => "v2.0",
        "v2.0": DatabaseConfigV2 => "v3.0",
    )
)]
#[derive(Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub pool_size: usize,
}

impl From<DatabaseConfigV2> for DatabaseConfig {
    fn from(v2: DatabaseConfigV2) -> Self {
        Self {
            host: v2.host,
            port: v2.port,
            pool_size: 10,
        }
    }
}

// -----------------------------------------------------------------------------
// 4. Tuple struct implicit target (verifying From conversions and Serde restriction)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Coord1D(pub f64);

#[backwards_compat(
    tag = "coord_v",
    version = 2,
    versions(
        1: Coord1D,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Coord2D(pub f64, pub f64);

impl From<Coord1D> for Coord2D {
    fn from(v1: Coord1D) -> Self {
        Self(v1.0, 0.0)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[test]
fn test_implicit_target_json_deserialization_chain() {
    // V1 payload upgrading through V2 to V3
    let json_v1 = r#"{"schema_version": "1", "host": "api.example.com"}"#;
    let cfg1: ServerConfig = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        cfg1,
        ServerConfig {
            name: "api.example.com".to_string(),
            port: 80,
            timeout_seconds: 30,
            use_tls: true,
            active_endpoints: vec!["/health".to_string()],
        }
    );

    // V2 payload upgrading to V3
    let json_v2 = r#"{"schema_version": "2", "host": "db.example.com", "port": 5432}"#;
    let cfg2: ServerConfig = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        cfg2,
        ServerConfig {
            name: "db.example.com".to_string(),
            port: 5432,
            timeout_seconds: 30,
            use_tls: true,
            active_endpoints: vec!["/health".to_string()],
        }
    );

    // V3 payload (implicit target) with all fields provided
    let json_v3_full = r#"{
        "schema_version": "3",
        "service-name": "worker.example.com",
        "port": 9000,
        "timeout-seconds": 60,
        "use-tls": false,
        "active-endpoints": ["/jobs", "/metrics"]
    }"#;
    let cfg3_full: ServerConfig = serde_json::from_str(json_v3_full).unwrap();
    assert_eq!(
        cfg3_full,
        ServerConfig {
            name: "worker.example.com".to_string(),
            port: 9000,
            timeout_seconds: 60,
            use_tls: false,
            active_endpoints: vec!["/jobs".to_string(), "/metrics".to_string()],
        }
    );

    // V3 payload with defaults omitted (tests #[serde(default = "default_port")], #[serde(default = "default_timeout")], and #[serde(default)])
    let json_v3_defaults = r#"{
        "schema_version": "3",
        "service-name": "default.example.com",
        "active-endpoints": []
    }"#;
    let cfg3_defaults: ServerConfig = serde_json::from_str(json_v3_defaults).unwrap();
    assert_eq!(
        cfg3_defaults,
        ServerConfig {
            name: "default.example.com".to_string(),
            port: 8080,
            timeout_seconds: 30,
            use_tls: false,
            active_endpoints: vec![],
        }
    );
}

#[test]
fn test_implicit_target_json_serialization_roundtrip() {
    let cfg = ServerConfig {
        name: "gateway".to_string(),
        port: 443,
        timeout_seconds: 15,
        use_tls: true,
        active_endpoints: vec!["/".to_string()],
    };

    let serialized = serde_json::to_string(&cfg).unwrap();
    assert!(serialized.contains(r#""schema_version":"3""#));
    assert!(serialized.contains(r#""service-name":"gateway""#));
    assert!(serialized.contains(r#""port":443"#));
    assert!(serialized.contains(r#""timeout-seconds":15"#));
    assert!(serialized.contains(r#""use-tls":true"#));
    assert!(serialized.contains(r#""active-endpoints":["/"]"#));

    let deserialized: ServerConfig = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, cfg);
}

#[test]
fn test_implicit_target_toml_support() {
    // V1 toml
    let toml_v1 = r#"
        schema_version = "1"
        host = "toml.v1.com"
    "#;
    let cfg1: ServerConfig = toml::from_str(toml_v1).unwrap();
    assert_eq!(cfg1.name, "toml.v1.com");
    assert_eq!(cfg1.port, 80);
    assert_eq!(cfg1.timeout_seconds, 30);
    assert!(cfg1.use_tls);

    // V2 toml
    let toml_v2 = r#"
        schema_version = "2"
        host = "toml.v2.com"
        port = 8443
    "#;
    let cfg2: ServerConfig = toml::from_str(toml_v2).unwrap();
    assert_eq!(cfg2.name, "toml.v2.com");
    assert_eq!(cfg2.port, 8443);
    assert_eq!(cfg2.timeout_seconds, 30);
    assert!(cfg2.use_tls);

    // V3 toml with defaults omitted
    let toml_v3 = r#"
        schema_version = "3"
        service-name = "toml.v3.com"
        active-endpoints = ["/toml"]
    "#;
    let cfg3: ServerConfig = toml::from_str(toml_v3).unwrap();
    assert_eq!(cfg3.name, "toml.v3.com");
    assert_eq!(cfg3.port, 8080);
    assert_eq!(cfg3.timeout_seconds, 30);
    assert!(!cfg3.use_tls);

    // V3 toml serialization roundtrip
    let serialized = toml::to_string(&cfg3).unwrap();
    assert!(serialized.contains("schema_version = \"3\""));
    assert!(serialized.contains("service-name = \"toml.v3.com\""));
    let deserialized: ServerConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized, cfg3);
}

#[test]
fn test_implicit_target_ron_support() {
    // V1 ron
    let ron_v1 = r#"(
        schema_version: "1",
        host: "ron.v1.com",
    )"#;
    let cfg1: ServerConfig = ron::from_str(ron_v1).unwrap();
    assert_eq!(cfg1.name, "ron.v1.com");
    assert_eq!(cfg1.port, 80);

    // V2 ron
    let ron_v2 = r#"(
        schema_version: "2",
        host: "ron.v2.com",
        port: 9090,
    )"#;
    let cfg2: ServerConfig = ron::from_str(ron_v2).unwrap();
    assert_eq!(cfg2.name, "ron.v2.com");
    assert_eq!(cfg2.port, 9090);

    // V3 ron serialization and deserialization
    let cfg3 = ServerConfig {
        name: "ron.v3.com".to_string(),
        port: 8080,
        timeout_seconds: 45,
        use_tls: true,
        active_endpoints: vec!["/status".to_string()],
    };
    let serialized = ron::to_string(&cfg3).unwrap();
    assert!(serialized.contains("schema_version:\"3\""));
    assert!(serialized.contains("service-name:\"ron.v3.com\""));
    let deserialized: ServerConfig = ron::from_str(&serialized).unwrap();
    assert_eq!(deserialized, cfg3);
}

#[test]
fn test_fallible_implicit_target_chain() {
    // Success: V1 -> V2 -> Metric
    let json_v1 = r#"{"v": "1", "metric_str": "42"}"#;
    let metric: Metric = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        metric,
        Metric {
            value: 42,
            unit: "count".to_string(),
        }
    );

    // Success: V2 -> Metric
    let json_v2 = r#"{"v": "2", "raw_val": 100}"#;
    let metric2: Metric = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        metric2,
        Metric {
            value: 100,
            unit: "count".to_string(),
        }
    );

    // Success: V3 direct with default unit
    let json_v3 = r#"{"v": "3", "value": 500}"#;
    let metric3: Metric = serde_json::from_str(json_v3).unwrap();
    assert_eq!(
        metric3,
        Metric {
            value: 500,
            unit: "".to_string(),
        }
    );

    // Failure in step 1 (V1 -> V2 invalid integer parsing)
    let json_v1_err = r#"{"v": "1", "metric_str": "not_a_number"}"#;
    let err1 = serde_json::from_str::<Metric>(json_v1_err);
    assert!(err1.is_err());
    let err1_msg = err1.unwrap_err().to_string();
    assert!(err1_msg.contains("failed to parse metric value"));

    // Failure in step 2 (V2 -> Metric negative value)
    let json_v2_err = r#"{"v": "2", "raw_val": -5}"#;
    let err2 = serde_json::from_str::<Metric>(json_v2_err);
    assert!(err2.is_err());
    let err2_msg = err2.unwrap_err().to_string();
    assert!(err2_msg.contains("metric value cannot be negative"));
}

#[test]
fn test_string_keys_implicit_target_support() {
    // V1 ("v1.0" -> V2 "v2.0" -> Target "v3.0")
    let json_v1 = r#"{"api_version": "v1.0", "url": "postgres://localhost"}"#;
    let db1: DatabaseConfig = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        db1,
        DatabaseConfig {
            host: "postgres://localhost".to_string(),
            port: 5432,
            pool_size: 10,
        }
    );

    // V2 ("v2.0" -> Target "v3.0")
    let json_v2 = r#"{"api_version": "v2.0", "host": "mysql.local", "port": 3306}"#;
    let db2: DatabaseConfig = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        db2,
        DatabaseConfig {
            host: "mysql.local".to_string(),
            port: 3306,
            pool_size: 10,
        }
    );

    // V3 with default pool_size
    let json_v3_default = r#"{"api_version": "v3.0", "host": "redis.local", "port": 6379}"#;
    let db3: DatabaseConfig = serde_json::from_str(json_v3_default).unwrap();
    assert_eq!(
        db3,
        DatabaseConfig {
            host: "redis.local".to_string(),
            port: 6379,
            pool_size: 0,
        }
    );

    // V3 serialization roundtrip
    let db = DatabaseConfig {
        host: "primary.db".to_string(),
        port: 5432,
        pool_size: 25,
    };
    let serialized = serde_json::to_string(&db).unwrap();
    assert!(serialized.contains(r#""api_version":"v3.0""#));
    assert!(serialized.contains(r#""host":"primary.db""#));
    assert!(serialized.contains(r#""pool_size":25"#));
    let deserialized: DatabaseConfig = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, db);

    // Container attribute #[serde(deny_unknown_fields)] preserved on shadow wire
    let json_unknown = r#"{"api_version": "v3.0", "host": "h", "port": 80, "extra": "forbidden"}"#;
    let err = serde_json::from_str::<DatabaseConfig>(json_unknown);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown field `extra`"));
}

#[test]
fn test_tuple_struct_implicit_target_from_conversion() {
    let c1 = Coord1D(5.5);
    let c2 = Coord2D::from(c1);
    assert_eq!(c2, Coord2D(5.5, 0.0));

    // Serde internally tagged enums cannot serialize/deserialize tuple structs
    let res = serde_json::to_string(&c2);
    assert!(res.is_err());
}
