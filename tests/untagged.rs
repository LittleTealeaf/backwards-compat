use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// =========================================================================
// 1. Single `_` (`_: LegacyV0` and `_: Self`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct LegacyV0 {
    pub old_name: String,
}

#[backwards_compat(
    versions(
        _: LegacyV0 => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct SingleUntaggedModel {
    pub name: String,
}

impl From<LegacyV0> for SingleUntaggedModel {
    fn from(v: LegacyV0) -> Self {
        Self { name: v.old_name }
    }
}

#[backwards_compat(
    versions(
        _: Self,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct UntaggedSelfModel {
    pub title: String,
}

#[test]
fn test_single_untagged_type_and_self() {
    let from_legacy: SingleUntaggedModel = serde_json::from_str(r#"{"old_name":"alice"}"#).unwrap();
    assert_eq!(
        from_legacy,
        SingleUntaggedModel {
            name: "alice".to_owned(),
        }
    );

    let from_v1: SingleUntaggedModel = serde_json::from_str(r#"{"version":1,"name":"bob"}"#).unwrap();
    assert_eq!(
        from_v1,
        SingleUntaggedModel {
            name: "bob".to_owned(),
        }
    );

    let self_untagged: UntaggedSelfModel = serde_json::from_str(r#"{"title":"unversioned"}"#).unwrap();
    assert_eq!(
        self_untagged,
        UntaggedSelfModel {
            title: "unversioned".to_owned(),
        }
    );
    let json = serde_json::to_string(&self_untagged).unwrap();
    assert!(json.contains(r#""version":1"#));

    // RON: unversioned `(...)`, named `Struct(...)`, versioned `(...)`, and struct-formatted output
    let ron_legacy: SingleUntaggedModel = ron::from_str(r#"(old_name:"alice")"#).unwrap();
    assert_eq!(ron_legacy, from_legacy);
    let ron_named: SingleUntaggedModel = ron::from_str(r#"LegacyV0(old_name:"alice")"#).unwrap();
    assert_eq!(ron_named, from_legacy);
    let ron_v1: SingleUntaggedModel = ron::from_str(r#"(version:1,name:"bob")"#).unwrap();
    assert_eq!(ron_v1, from_v1);
    let ron_out = ron::to_string(&from_v1).unwrap();
    assert!(
        ron_out.starts_with('(') && ron_out.ends_with(')'),
        "got: {ron_out}"
    );
    assert_eq!(ron::from_str::<SingleUntaggedModel>(&ron_out).unwrap(), from_v1);
}

// =========================================================================
// 2. Multiple `_` entries tried in declaration order
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct DetailedLegacy {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct MinimalLegacy {
    pub host: String,
}

#[backwards_compat(
    versions(
        _: DetailedLegacy => 1,
        _: MinimalLegacy => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct MultiUntaggedConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
}

impl From<DetailedLegacy> for MultiUntaggedConfig {
    fn from(v: DetailedLegacy) -> Self {
        Self {
            host: v.host,
            port: v.port,
            tls: false,
        }
    }
}

impl From<MinimalLegacy> for MultiUntaggedConfig {
    fn from(v: MinimalLegacy) -> Self {
        Self {
            host: v.host,
            port: 80,
            tls: false,
        }
    }
}

#[test]
fn test_multiple_untagged_entries_declaration_order() {
    // Matches more specific DetailedLegacy first
    let detailed: MultiUntaggedConfig =
        serde_json::from_str(r#"{"host":"example.com","port":8443}"#).unwrap();
    assert_eq!(
        detailed,
        MultiUntaggedConfig {
            host: "example.com".to_owned(),
            port: 8443,
            tls: false,
        }
    );

    // Falls back to MinimalLegacy when port is absent
    let minimal: MultiUntaggedConfig = serde_json::from_str(r#"{"host":"localhost"}"#).unwrap();
    assert_eq!(
        minimal,
        MultiUntaggedConfig {
            host: "localhost".to_owned(),
            port: 80,
            tls: false,
        }
    );

    // Tagged v1 still works normally
    let v1: MultiUntaggedConfig =
        serde_json::from_str(r#"{"version":1,"host":"prod","port":443,"tls":true}"#).unwrap();
    assert_eq!(
        v1,
        MultiUntaggedConfig {
            host: "prod".to_owned(),
            port: 443,
            tls: true,
        }
    );
}

// =========================================================================
// 3. `_` with `#[with(fn_path)]` and `#[fallible]`
// =========================================================================

#[derive(Debug, Error, PartialEq, Eq)]
#[error("empty host is not allowed")]
pub struct EmptyHostError;

#[derive(Debug, Deserialize)]
pub struct RawLegacyHost {
    pub raw_host: String,
}

fn validate_legacy_host(v: RawLegacyHost) -> Result<ValidatedHost, EmptyHostError> {
    if v.raw_host.is_empty() {
        return Err(EmptyHostError);
    }
    Ok(ValidatedHost { host: v.raw_host })
}

#[backwards_compat(
    versions(
        #[fallible]
        #[with(validate_legacy_host)]
        _: RawLegacyHost => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedHost {
    pub host: String,
}

#[test]
fn test_untagged_with_fn_and_fallible() {
    let ok: ValidatedHost = serde_json::from_str(r#"{"raw_host":"db.internal"}"#).unwrap();
    assert_eq!(
        ok,
        ValidatedHost {
            host: "db.internal".to_owned(),
        }
    );

    let err = serde_json::from_str::<ValidatedHost>(r#"{"raw_host":""}"#).unwrap_err();
    assert!(err.to_string().contains("empty host is not allowed"));
}

// =========================================================================
// 4. `#[cfg(...)]`-gated single and multiple `_` entries
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatedUntaggedV1 {
    pub name: String,
}

impl From<LegacyV0> for GatedUntaggedV1 {
    fn from(v: LegacyV0) -> Self {
        Self { name: v.old_name }
    }
}

#[backwards_compat(
    versions(
        #[cfg(test)]
        _: LegacyV0 => 1,
        #[cfg(any())]
        _: NonExistentUntagged => 1,
        1: GatedUntaggedV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGatedUntagged {
    pub name: String,
}

impl From<GatedUntaggedV1> for ActiveGatedUntagged {
    fn from(v: GatedUntaggedV1) -> Self {
        Self { name: v.name }
    }
}

impl From<ActiveGatedUntagged> for GatedUntaggedV1 {
    fn from(t: ActiveGatedUntagged) -> Self {
        Self { name: t.name }
    }
}

#[backwards_compat(
    versions(
        #[cfg(any())]
        _: NonExistentUntaggedA => 1,
        #[cfg(any())]
        _: NonExistentUntaggedB => 1,
        1: GatedUntaggedV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveGatedUntagged {
    pub name: String,
}

impl From<GatedUntaggedV1> for InactiveGatedUntagged {
    fn from(v: GatedUntaggedV1) -> Self {
        Self { name: v.name }
    }
}

impl From<InactiveGatedUntagged> for GatedUntaggedV1 {
    fn from(t: InactiveGatedUntagged) -> Self {
        Self { name: t.name }
    }
}

#[test]
fn test_cfg_gated_single_and_multiple_untagged() {
    let active: ActiveGatedUntagged = serde_json::from_str(r#"{"old_name":"legacy"}"#).unwrap();
    assert_eq!(
        active,
        ActiveGatedUntagged {
            name: "legacy".to_owned(),
        }
    );

    // All untagged entries disabled -> unversioned JSON is rejected, tagged v1 works
    let _ = serde_json::from_str::<InactiveGatedUntagged>(r#"{"old_name":"legacy"}"#).unwrap_err();
    let tagged: InactiveGatedUntagged = serde_json::from_str(r#"{"version":1,"name":"current"}"#).unwrap();
    assert_eq!(
        tagged,
        InactiveGatedUntagged {
            name: "current".to_owned(),
        }
    );
}
