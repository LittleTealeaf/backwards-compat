use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum MigrationError {
    #[error("Legacy port {0} is invalid")]
    InvalidPort(u32),
    #[error("Domain validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfigV1 {
    pub port: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfigV2 {
    pub port: u16,
}

impl TryFrom<ServerConfigV1> for ServerConfigV2 {
    type Error = MigrationError;

    fn try_from(v1: ServerConfigV1) -> Result<Self, Self::Error> {
        let port: u16 = v1
            .port
            .try_into()
            .map_err(|_| MigrationError::InvalidPort(v1.port))?;
        Ok(Self { port })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub port: u16,
}

impl TryFrom<ServerConfigV2> for ServerConfig {
    type Error = MigrationError;

    fn try_from(v2: ServerConfigV2) -> Result<Self, Self::Error> {
        if v2.port == 0 {
            return Err(MigrationError::Validation("Port cannot be 0".to_string()));
        }
        Ok(Self { port: v2.port })
    }
}

impl From<ServerConfig> for ServerConfigV2 {
    fn from(s: ServerConfig) -> Self {
        Self { port: s.port }
    }
}

backwards_compat! {
    #[tag = "version", version = 2, error = MigrationError]
    compat ServerConfig {
        #[fallible] 1: ServerConfigV1,
        #[fallible] 2: ServerConfigV2,
    }
}

#[test]
fn test_fallible_success() {
    let raw = r#"{"version": "1", "port": 8080}"#;
    let config: ServerConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.port, 8080);
}

#[test]
fn test_fallible_step_error() {
    // 70000 is too large for u16
    let raw = r#"{"version": "1", "port": 70000}"#;
    let result: Result<ServerConfig, _> = serde_json::from_str(raw);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Legacy port 70000 is invalid"));
}

#[test]
fn test_fallible_final_validation_error() {
    let raw = r#"{"version": "2", "port": 0}"#;
    let result: Result<ServerConfig, _> = serde_json::from_str(raw);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Port cannot be 0"));
}

#[test]
fn test_fallible_roundtrip() {
    let config = ServerConfig { port: 3000 };
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""version":"2""#));
    assert!(json.contains(r#""port":3000"#));

    let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, parsed);
}
