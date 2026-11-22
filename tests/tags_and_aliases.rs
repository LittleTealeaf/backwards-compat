use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Strict native integer tags vs string tags + deny_unknown_fields
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictV1 {
    pub name: String,
}

#[backwards_compat(
    versions(
        1: StrictV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StrictIntModel {
    pub name: String,
    pub level: u32,
}

impl From<StrictV1> for StrictIntModel {
    fn from(v: StrictV1) -> Self {
        Self {
            name: v.name,
            level: 1,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrV1 {
    pub name: String,
}

#[backwards_compat(
    tag = "schema_version",
    versions(
        "1.0": StrV1,
        "2.0": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StrictStrModel {
    pub name: String,
    pub level: u32,
}

impl From<StrV1> for StrictStrModel {
    fn from(v: StrV1) -> Self {
        Self {
            name: v.name,
            level: 2,
        }
    }
}

#[test]
fn test_strict_native_int_and_str_tags_with_deny_unknown_fields() {
    // Integer tag 1 succeeds even with #[serde(deny_unknown_fields)]
    let v1: StrictIntModel = serde_json::from_str(r#"{"version":1,"name":"alpha"}"#).unwrap();
    assert_eq!(
        v1,
        StrictIntModel {
            name: "alpha".to_owned(),
            level: 1,
        }
    );

    // String "1" is strictly rejected when only integer 1 is declared
    let str_err = serde_json::from_str::<StrictIntModel>(r#"{"version":"1","name":"alpha"}"#).unwrap_err();
    assert!(str_err.to_string().contains("unknown version \"1\""));

    // Serializes native integer 2 (not "2")
    let json = serde_json::to_string(&v1).unwrap();
    assert!(json.contains(r#""version":2"#));
    assert!(!json.contains(r#""version":"2""#));

    // String tags with custom tag field across JSON, TOML, RON
    let s1: StrictStrModel = serde_json::from_str(r#"{"schema_version":"1.0","name":"beta"}"#).unwrap();
    assert_eq!(
        s1,
        StrictStrModel {
            name: "beta".to_owned(),
            level: 2,
        }
    );
    let s_json = serde_json::to_string(&s1).unwrap();
    assert!(s_json.contains(r#""schema_version":"2.0""#));

    let s_toml = toml::to_string(&s1).unwrap();
    assert!(s_toml.contains(r#"schema_version = "2.0""#));
    assert_eq!(toml::from_str::<StrictStrModel>(&s_toml).unwrap(), s1);

    let s_ron = ron::to_string(&s1).unwrap();
    assert_eq!(ron::from_str::<StrictStrModel>(&s_ron).unwrap(), s1);
}

// =========================================================================
// 2. Inline `|` aliases (historical, aliased `#[current] Self`, and
//    aliased `#[current] V2` regression test)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct AliasV1 {
    pub x: i32,
}

#[backwards_compat(
    versions(
        1 | "1" | "v1": AliasV1 => 2,
        #[current]
        2 | "2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct AliasedSelfModel {
    pub x: i32,
}

impl From<AliasV1> for AliasedSelfModel {
    fn from(v: AliasV1) -> Self {
        Self { x: v.x + 10 }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasWireV2 {
    pub x: i32,
}

impl From<AliasV1> for AliasWireV2 {
    fn from(v: AliasV1) -> Self {
        Self { x: v.x + 10 }
    }
}

#[backwards_compat(
    versions(
        1 | "1": AliasV1 => 2,
        2 | "2": AliasWireV2,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasedWireModel {
    pub x: i32,
}

impl From<AliasWireV2> for AliasedWireModel {
    fn from(w: AliasWireV2) -> Self {
        Self { x: w.x }
    }
}

impl From<AliasedWireModel> for AliasWireV2 {
    fn from(m: AliasedWireModel) -> Self {
        Self { x: m.x }
    }
}

#[test]
fn test_inline_pipe_aliases_for_historical_and_current() {
    for payload in [
        r#"{"version":1,"x":5}"#,
        r#"{"version":"1","x":5}"#,
        r#"{"version":"v1","x":5}"#,
    ] {
        let m: AliasedSelfModel = serde_json::from_str(payload).unwrap();
        assert_eq!(m, AliasedSelfModel { x: 15 });
    }

    for payload in [r#"{"version":2,"x":99}"#, r#"{"version":"2","x":99}"#] {
        let m: AliasedSelfModel = serde_json::from_str(payload).unwrap();
        assert_eq!(m, AliasedSelfModel { x: 99 });

        let w: AliasedWireModel = serde_json::from_str(payload).unwrap();
        assert_eq!(w, AliasedWireModel { x: 99 });
    }

    // Serialization emits the primary tag (integer 2)
    let json = serde_json::to_string(&AliasedSelfModel { x: 99 }).unwrap();
    assert!(json.contains(r#""version":2"#));
    let w_json = serde_json::to_string(&AliasedWireModel { x: 99 }).unwrap();
    assert!(w_json.contains(r#""version":2"#));
}

// =========================================================================
// 3. Rich unknown, future, disabled-gate, and invalid-type version errors
// =========================================================================

#[backwards_compat(
    versions(
        #[cfg(any())]
        1: NonExistentV1 => 2,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GatedErrorModel {
    pub x: i32,
}

#[test]
fn test_rich_version_error_messages() {
    // Future integer version (9 > 2)
    let future_err =
        serde_json::from_str::<StrictIntModel>(r#"{"version":9,"name":"x","level":1}"#).unwrap_err();
    assert_eq!(
        future_err.to_string(),
        "version 9 is newer than supported version 2 (supported versions: 1, 2)"
    );

    // Unknown integer version (0 <= 2, no disabled gates)
    let unknown_int_err = serde_json::from_str::<StrictIntModel>(r#"{"version":0,"name":"x"}"#).unwrap_err();
    assert_eq!(
        unknown_int_err.to_string(),
        "unknown version 0 (supported versions: 1, 2)"
    );

    // Unknown version when a #[cfg] version is disabled includes feature-flag hint
    let disabled_hint_err = serde_json::from_str::<GatedErrorModel>(r#"{"version":1,"x":1}"#).unwrap_err();
    assert_eq!(
        disabled_hint_err.to_string(),
        "unknown version 1 (supported versions: 2; some versions may be disabled by feature flags)"
    );

    // Unknown string version
    let unknown_str_err =
        serde_json::from_str::<StrictStrModel>(r#"{"schema_version":"0.5","name":"x"}"#).unwrap_err();
    assert_eq!(
        unknown_str_err.to_string(),
        "unknown version \"0.5\" (supported versions: \"1.0\", \"2.0\")"
    );

    // Invalid tag type (boolean)
    let bad_type_err = serde_json::from_str::<StrictIntModel>(r#"{"version":true,"name":"x"}"#).unwrap_err();
    assert!(
        bad_type_err
            .to_string()
            .contains("expected an integer or string version tag")
    );
}
