#![deny(missing_debug_implementations)]

use backwards_compat::backwards_compat;
use serde::Deserialize;

// =========================================================================
// 1. Sequential migration: _: ConfigV0 -> 1: ConfigV1 -> Config (version = 2)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfigV0 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
    pub host: String,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        _: ConfigV0,
        1: ConfigV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub name: String,
    pub host: String,
    pub port: u16,
}

impl From<ConfigV0> for ConfigV1 {
    fn from(v0: ConfigV0) -> Self {
        Self {
            name: v0.name,
            host: "127.0.0.1".into(),
        }
    }
}

impl From<ConfigV1> for Config {
    fn from(v1: ConfigV1) -> Self {
        Self {
            name: v1.name,
            host: v1.host,
            port: 8080,
        }
    }
}

#[test]
fn test_sequential_pre_backwards_compat_deserialization_and_conversions() {
    // 1. Unversioned (pre-backwards-compat) JSON payload
    let json_v0 = r#"{"name": "legacy-service"}"#;
    let from_v0: Config = serde_json::from_str(json_v0).unwrap();
    assert_eq!(
        from_v0,
        Config {
            name: "legacy-service".into(),
            host: "127.0.0.1".into(),
            port: 8080,
        }
    );

    // 2. Version 1 JSON payload
    let json_v1 = r#"{"version": "1", "name": "v1-service", "host": "0.0.0.0"}"#;
    let from_v1: Config = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        from_v1,
        Config {
            name: "v1-service".into(),
            host: "0.0.0.0".into(),
            port: 8080,
        }
    );

    // 3. Version 2 JSON payload (current active version)
    let json_v2 = r#"{"version": "2", "name": "v2-service", "host": "localhost", "port": 9090}"#;
    let from_v2: Config = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        from_v2,
        Config {
            name: "v2-service".into(),
            host: "localhost".into(),
            port: 9090,
        }
    );

    // 4. Serialization always emits the active version tag ("2")
    let serialized = serde_json::to_string(&from_v0).unwrap();
    assert!(serialized.contains(r#""version":"2""#));

    // 5. Transitive in-code From<ConfigV0> for Config
    let v0_struct = ConfigV0 {
        name: "manual".into(),
    };
    let converted = Config::from(v0_struct.clone());
    let into_converted: Config = v0_struct.into();
    assert_eq!(converted, into_converted);
    assert_eq!(converted.host, "127.0.0.1");
    assert_eq!(converted.port, 8080);

    // 6. Strict tag error preservation: if "version" IS present with an unknown tag, it must fail
    // rather than falling back to ConfigV0!
    let invalid_version_json = r#"{"version": "99", "name": "bad-version"}"#;
    let err = serde_json::from_str::<Config>(invalid_version_json).unwrap_err();
    assert!(
        err.to_string().contains("unknown variant `99`"),
        "unexpected error: {err}"
    );

    // 7. Strict field error preservation: if "version": "1" is present but missing required v1 field "host",
    // it must report missing field `host` rather than falling back to ConfigV0!
    let malformed_v1_json = r#"{"version": "1", "name": "missing-host"}"#;
    let err_v1 = serde_json::from_str::<Config>(malformed_v1_json).unwrap_err();
    assert!(
        err_v1.to_string().contains("missing field `host`"),
        "unexpected error: {err_v1}"
    );
}

#[test]
fn test_pre_backwards_compat_toml_and_ron() {
    // TOML unversioned
    let toml_v0 = r#"name = "toml-legacy""#;
    let cfg_toml: Config = toml::from_str(toml_v0).unwrap();
    assert_eq!(cfg_toml.name, "toml-legacy");
    assert_eq!(cfg_toml.host, "127.0.0.1");
    assert_eq!(cfg_toml.port, 8080);

    // RON unversioned
    let ron_v0 = r#"(name: "ron-legacy")"#;
    let cfg_ron: Config = ron::from_str(ron_v0).unwrap();
    assert_eq!(cfg_ron.name, "ron-legacy");
    assert_eq!(cfg_ron.host, "127.0.0.1");
    assert_eq!(cfg_ron.port, 8080);
}

// =========================================================================
// 2. Adopting backwards_compat on an unchanged struct (`_: Self`)
// =========================================================================

#[backwards_compat(tag = "v", version = 1, versions(_: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptedModel {
    pub title: String,
    pub enabled: bool,
}

#[test]
fn test_adopted_same_struct_with_untagged_self() {
    // Unversioned payload
    let untagged_json = r#"{"title": "hello", "enabled": true}"#;
    let m1: AdoptedModel = serde_json::from_str(untagged_json).unwrap();
    assert_eq!(
        m1,
        AdoptedModel {
            title: "hello".into(),
            enabled: true,
        }
    );

    // Versioned v1 payload
    let tagged_json = r#"{"v": "1", "title": "hello", "enabled": true}"#;
    let m2: AdoptedModel = serde_json::from_str(tagged_json).unwrap();
    assert_eq!(m1, m2);

    // Serializing always includes `"v":"1"`
    let out = serde_json::to_string(&m1).unwrap();
    assert!(out.contains(r#""v":"1""#));

    // Invalid version tag errors
    let bad_tag = r#"{"v": "2", "title": "hello", "enabled": true}"#;
    let _ = serde_json::from_str::<AdoptedModel>(bad_tag).unwrap_err();
}

// =========================================================================
// 3. Fallible & DAG migration from pre-backwards-compat (`#[fallible] _: V0 => 2`)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("invalid port: {0}")]
    InvalidPort(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerV0 {
    pub port: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerV1 {
    pub port: u16,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    error = PortError,
    versions(
        #[fallible] _: ServerV0 => 2,
        1: ServerV1 => 2,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub port: u16,
    pub tls: bool,
}

impl TryFrom<ServerV0> for Server {
    type Error = PortError;
    fn try_from(v0: ServerV0) -> Result<Self, Self::Error> {
        let port = u16::try_from(v0.port).map_err(|_| PortError::InvalidPort(v0.port))?;
        Ok(Self { port, tls: false })
    }
}

impl From<ServerV1> for Server {
    fn from(v1: ServerV1) -> Self {
        Self {
            port: v1.port,
            tls: true,
        }
    }
}

#[test]
fn test_fallible_dag_pre_backwards_compat() {
    let valid_v0 = r#"{"port": 443}"#;
    let s: Server = serde_json::from_str(valid_v0).unwrap();
    assert_eq!(
        s,
        Server {
            port: 443,
            tls: false
        }
    );

    let invalid_v0 = r#"{"port": -10}"#;
    let err = serde_json::from_str::<Server>(invalid_v0).unwrap_err();
    assert!(err.to_string().contains("invalid port: -10"));
}

// =========================================================================
// 4. Adjacent tagging with pre-backwards-compat scalar/tuple struct
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScalarV0(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScalarV1(pub u64);

#[backwards_compat(
    tag = "t",
    content = "c",
    version = 2,
    versions(
        untagged: ScalarV0,
        1: ScalarV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarTarget(pub String);

impl From<ScalarV0> for ScalarV1 {
    fn from(v0: ScalarV0) -> Self {
        Self(u64::from(v0.0))
    }
}

impl From<ScalarV1> for ScalarTarget {
    fn from(v1: ScalarV1) -> Self {
        Self(v1.0.to_string())
    }
}

#[test]
fn test_adjacent_tagging_pre_backwards_compat() {
    // Raw unversioned scalar (not even a map!)
    let raw_v0 = "42";
    let target0: ScalarTarget = serde_json::from_str(raw_v0).unwrap();
    assert_eq!(target0.0, "42");

    // Version 1 adjacent tagged
    let tagged_v1 = r#"{"t": "1", "c": 100}"#;
    let target1: ScalarTarget = serde_json::from_str(tagged_v1).unwrap();
    assert_eq!(target1.0, "100");

    // Version 2 adjacent tagged
    let tagged_v2 = r#"{"t": "2", "c": "hello"}"#;
    let target2: ScalarTarget = serde_json::from_str(tagged_v2).unwrap();
    assert_eq!(target2.0, "hello");
}

// =========================================================================
// 5. Generics with pre-backwards-compat
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WrapperV0<T> {
    pub item: T,
}

#[backwards_compat(
    tag = "version",
    version = 1,
    versions(none: WrapperV0<T>)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapper<T> {
    pub item: T,
    pub versioned: bool,
}

impl<T> From<WrapperV0<T>> for Wrapper<T> {
    fn from(v0: WrapperV0<T>) -> Self {
        Self {
            item: v0.item,
            versioned: false,
        }
    }
}

#[test]
fn test_generic_pre_backwards_compat() {
    let v0_json = r#"{"item": 123}"#;
    let w0: Wrapper<i32> = serde_json::from_str(v0_json).unwrap();
    assert_eq!(
        w0,
        Wrapper {
            item: 123,
            versioned: false
        }
    );

    let v1_json = r#"{"version": "1", "item": 456, "versioned": true}"#;
    let w1: Wrapper<i32> = serde_json::from_str(v1_json).unwrap();
    assert_eq!(
        w1,
        Wrapper {
            item: 456,
            versioned: true
        }
    );
}

// =========================================================================
// 6. Borrowed lifetimes ('a) with pre-backwards-compat
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BorrowedV0<'a> {
    pub label: &'a str,
}

#[backwards_compat(
    tag = "version",
    version = 1,
    versions(_: BorrowedV0<'a>)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowedModel<'a> {
    pub label: &'a str,
    pub upgraded: bool,
}

impl<'a> From<BorrowedV0<'a>> for BorrowedModel<'a> {
    fn from(v0: BorrowedV0<'a>) -> Self {
        Self {
            label: v0.label,
            upgraded: true,
        }
    }
}

#[test]
fn test_borrowed_lifetime_pre_backwards_compat() {
    let raw_v0 = r#"{"label": "zero-copy-v0"}"#;
    let b0: BorrowedModel<'_> = serde_json::from_str(raw_v0).unwrap();
    assert_eq!(
        b0,
        BorrowedModel {
            label: "zero-copy-v0",
            upgraded: true,
        }
    );

    let raw_v1 = r#"{"version": "1", "label": "zero-copy-v1", "upgraded": false}"#;
    let b1: BorrowedModel<'_> = serde_json::from_str(raw_v1).unwrap();
    assert_eq!(
        b1,
        BorrowedModel {
            label: "zero-copy-v1",
            upgraded: false,
        }
    );
}
