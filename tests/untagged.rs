use backwards_compat::{Migrated, backwards_compat};
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

// =========================================================================
// 5. One-way tag-type migrations & colliding legacy `version` fields
//    via mismatched-tag fallback to `_`
// =========================================================================

// 5a. Legacy string-tagged family ("v1" -> "v2") -> New integer-tagged family (3 -> 4)
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrFamilyV1 {
    pub label: String,
}

#[backwards_compat(
    versions(
        "v1": StrFamilyV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct LegacyStrFamily {
    pub label: String,
    pub weight: u32,
}

impl From<StrFamilyV1> for LegacyStrFamily {
    fn from(v: StrFamilyV1) -> Self {
        Self {
            label: v.label,
            weight: 10,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntFamilyV3 {
    pub label: String,
    pub weight: u32,
}

impl From<LegacyStrFamily> for IntFamilyV3 {
    fn from(v: LegacyStrFamily) -> Self {
        Self {
            label: v.label,
            weight: v.weight,
        }
    }
}

#[backwards_compat(
    versions(
        _: LegacyStrFamily => 3,
        3: IntFamilyV3,
        4: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NewIntFamily {
    pub label: String,
    pub weight: u32,
    pub active: bool,
}

impl From<IntFamilyV3> for NewIntFamily {
    fn from(v: IntFamilyV3) -> Self {
        Self {
            label: v.label,
            weight: v.weight,
            active: true,
        }
    }
}

#[test]
fn test_mismatched_string_tag_falls_back_to_legacy_str_family() {
    let expected_from_v1 = NewIntFamily {
        label: "svc".to_owned(),
        weight: 10,
        active: true,
    };

    // Tag-first string payload -> buffers tag + remaining fields -> LegacyStrFamily ("v1" -> "v2") -> 3 -> 4
    let tag_first: Migrated<NewIntFamily> =
        serde_json::from_str(r#"{"version":"v1","label":"svc"}"#).unwrap();
    assert!(tag_first.is_untagged(), "mismatched tag should route through `_`");
    assert!(tag_first.was_migrated(), "mismatched tag should mark migrated");
    assert_eq!(*tag_first, expected_from_v1);

    // Tag-after-fields string payload -> LegacyStrFamily ("v2") -> 3 -> 4
    let tag_last: Migrated<NewIntFamily> =
        serde_json::from_str(r#"{"label":"svc","weight":77,"version":"v2"}"#).unwrap();
    assert!(
        tag_last.is_untagged(),
        "out-of-order mismatched tag should route through `_`"
    );
    assert_eq!(
        *tag_last,
        NewIntFamily {
            label: "svc".to_owned(),
            weight: 77,
            active: true,
        }
    );

    // Matching-type unknown integer version still fails fast (does NOT fall back to `_`)
    let unknown_int_err =
        serde_json::from_str::<NewIntFamily>(r#"{"version":99,"label":"svc"}"#).unwrap_err();
    assert!(
        unknown_int_err
            .to_string()
            .contains("version 99 is newer than supported version 4"),
        "unexpected error for unknown integer version: {unknown_int_err}"
    );

    // Multi-format round-trip: CBOR, named MessagePack, YAML
    let legacy_wire = LegacyStrFamily {
        label: "multi".to_owned(),
        weight: 5,
    };
    let mut cbor_buf = Vec::new();
    ciborium::into_writer(&legacy_wire, &mut cbor_buf).unwrap();
    let dec_cbor: NewIntFamily = ciborium::from_reader(cbor_buf.as_slice()).unwrap();
    assert_eq!(
        dec_cbor,
        NewIntFamily {
            label: "multi".to_owned(),
            weight: 5,
            active: true,
        }
    );

    let rmp_bytes = rmp_serde::to_vec_named(&legacy_wire).unwrap();
    let dec_rmp: NewIntFamily = rmp_serde::from_slice(&rmp_bytes).unwrap();
    assert_eq!(dec_rmp, dec_cbor);

    // Serializes as new integer version 4
    let out_json = serde_json::to_string(&dec_cbor).unwrap();
    assert_eq!(
        out_json,
        r#"{"version":4,"label":"multi","weight":5,"active":true}"#
    );
}

// 5b. Legacy integer-tagged family (1 -> 2) -> New string-tagged family ("v3" -> "v4")
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntFamilyV1 {
    pub name: String,
}

#[backwards_compat(
    versions(
        1: IntFamilyV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct LegacyIntFamily {
    pub name: String,
    pub rev: u32,
}

impl From<IntFamilyV1> for LegacyIntFamily {
    fn from(v: IntFamilyV1) -> Self {
        Self { name: v.name, rev: 1 }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrFamilyV3 {
    pub name: String,
    pub rev: u32,
}

impl From<LegacyIntFamily> for StrFamilyV3 {
    fn from(v: LegacyIntFamily) -> Self {
        Self {
            name: v.name,
            rev: v.rev,
        }
    }
}

#[backwards_compat(
    versions(
        _: LegacyIntFamily => "v3",
        "v3": StrFamilyV3,
        "v4": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NewStrFamily {
    pub name: String,
    pub rev: u32,
    pub verified: bool,
}

impl From<StrFamilyV3> for NewStrFamily {
    fn from(v: StrFamilyV3) -> Self {
        Self {
            name: v.name,
            rev: v.rev,
            verified: true,
        }
    }
}

#[test]
fn test_mismatched_int_tag_falls_back_to_legacy_int_family() {
    let from_int_v1: Migrated<NewStrFamily> =
        serde_json::from_str(r#"{"version":1,"name":"alpha"}"#).unwrap();
    assert!(
        from_int_v1.is_untagged(),
        "integer tag on str model should fall back to `_`"
    );
    assert_eq!(
        *from_int_v1,
        NewStrFamily {
            name: "alpha".to_owned(),
            rev: 1,
            verified: true,
        }
    );

    let from_int_v2_reordered: Migrated<NewStrFamily> =
        serde_json::from_str(r#"{"rev":9,"name":"beta","version":2}"#).unwrap();
    assert!(
        from_int_v2_reordered.is_untagged(),
        "out-of-order int tag should fall back"
    );
    assert_eq!(
        *from_int_v2_reordered,
        NewStrFamily {
            name: "beta".to_owned(),
            rev: 9,
            verified: true,
        }
    );

    // Matching-type unknown string version still fails fast
    let unknown_str_err =
        serde_json::from_str::<NewStrFamily>(r#"{"version":"v99","name":"alpha"}"#).unwrap_err();
    assert!(
        unknown_str_err.to_string().contains("unknown version \"v99\""),
        "unexpected error for unknown string version: {unknown_str_err}"
    );
}

// 5c. Adjacently-tagged (`content = "data"`) tag-type transition
#[backwards_compat(
    tag = "version",
    content = "data",
    versions(
        "v1": StrFamilyV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct LegacyAdjStrFamily {
    pub label: String,
    pub weight: u32,
}

impl From<StrFamilyV1> for LegacyAdjStrFamily {
    fn from(v: StrFamilyV1) -> Self {
        Self {
            label: v.label,
            weight: 10,
        }
    }
}

#[backwards_compat(
    tag = "version",
    content = "data",
    versions(
        _: LegacyAdjStrFamily => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NewAdjIntFamily {
    pub label: String,
    pub weight: u32,
    pub extra: bool,
}

impl From<LegacyAdjStrFamily> for NewAdjIntFamily {
    fn from(v: LegacyAdjStrFamily) -> Self {
        Self {
            label: v.label,
            weight: v.weight,
            extra: false,
        }
    }
}

#[test]
fn test_adjacent_mismatched_tag_falls_back_to_legacy_adj_family() {
    // Tag before content
    let tag_first: Migrated<NewAdjIntFamily> =
        serde_json::from_str(r#"{"version":"v1","data":{"label":"adj"}}"#).unwrap();
    assert!(
        tag_first.is_untagged(),
        "adjacent mismatched tag should fall back"
    );
    assert_eq!(
        *tag_first,
        NewAdjIntFamily {
            label: "adj".to_owned(),
            weight: 10,
            extra: false,
        }
    );

    // Content before tag
    let content_first: Migrated<NewAdjIntFamily> =
        serde_json::from_str(r#"{"data":{"label":"adj2","weight":22},"version":"v2"}"#).unwrap();
    assert!(
        content_first.is_untagged(),
        "reordered adjacent mismatched tag should fall back"
    );
    assert_eq!(
        *content_first,
        NewAdjIntFamily {
            label: "adj2".to_owned(),
            weight: 22,
            extra: false,
        }
    );
}

// 5d. Pre-backwards-compat struct that had its own `version: String` field
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct PreCompatWithVersionField {
    pub version: String,
    pub host: String,
}

#[backwards_compat(
    versions(
        _: PreCompatWithVersionField => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct MigratedFromCollidingField {
    pub api_version: String,
    pub host: String,
}

impl From<PreCompatWithVersionField> for MigratedFromCollidingField {
    fn from(v: PreCompatWithVersionField) -> Self {
        Self {
            api_version: v.version,
            host: v.host,
        }
    }
}

#[test]
fn test_unversioned_struct_with_colliding_version_field() {
    let legacy: Migrated<MigratedFromCollidingField> =
        serde_json::from_str(r#"{"version":"1.2.3","host":"localhost"}"#).unwrap();
    assert!(
        legacy.is_untagged(),
        "colliding string version field should fall back to `_`"
    );
    assert_eq!(
        *legacy,
        MigratedFromCollidingField {
            api_version: "1.2.3".to_owned(),
            host: "localhost".to_owned(),
        }
    );
}
