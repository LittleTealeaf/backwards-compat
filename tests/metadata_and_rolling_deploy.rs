use backwards_compat::{BackwardsCompat, Migrated, VersionKey, backwards_compat};
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. BackwardsCompat metadata trait (u64, &'static str, VersionKey)
//    + #[cfg]-aware SUPPORTED_VERSIONS and ACCEPTS_UNTAGGED
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct MetaV0 {
    pub x: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct MetaV1 {
    pub x: i32,
}

#[backwards_compat(
    versions(
        _: MetaV0 => 2,
        #[cfg(any())]
        0: NonExistentV0 => 2,
        #[cfg(test)]
        1: MetaV1 => 2,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct IntMetaModel {
    pub x: i32,
}

impl From<MetaV0> for IntMetaModel {
    fn from(v: MetaV0) -> Self {
        Self { x: v.x }
    }
}

impl From<MetaV1> for IntMetaModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x + 1 }
    }
}

#[backwards_compat(
    versions(
        "v1": MetaV1 => "v2",
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct StrMetaModel {
    pub x: i32,
}

impl From<MetaV1> for StrMetaModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x }
    }
}

#[backwards_compat(
    versions(
        1: MetaV1 => "v2",
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct MixedMetaModel {
    pub x: i32,
}

impl From<MetaV1> for MixedMetaModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x }
    }
}

#[test]
fn test_backwards_compat_metadata_trait() {
    // Int version mode (`u64`), with #[cfg(any())] stripped and #[cfg(test)] included
    assert_eq!(IntMetaModel::CURRENT_VERSION, 2);
    assert_eq!(IntMetaModel::SERIALIZE_VERSION, 2);
    assert_eq!(IntMetaModel::SUPPORTED_VERSIONS, &[1, 2]);
    const {
        assert!(IntMetaModel::ACCEPTS_UNTAGGED);
    }

    // Str version mode (`&'static str`)
    assert_eq!(StrMetaModel::CURRENT_VERSION, "v2");
    assert_eq!(StrMetaModel::SERIALIZE_VERSION, "v2");
    assert_eq!(StrMetaModel::SUPPORTED_VERSIONS, &["v1", "v2"]);
    const {
        assert!(!StrMetaModel::ACCEPTS_UNTAGGED);
    }

    // Mixed version mode (`VersionKey`)
    assert_eq!(MixedMetaModel::CURRENT_VERSION, VersionKey::Str("v2"));
    assert_eq!(MixedMetaModel::CURRENT_VERSION, "v2");
    assert_eq!(MixedMetaModel::SERIALIZE_VERSION, "v2");
    assert_eq!(
        MixedMetaModel::SUPPORTED_VERSIONS,
        &[VersionKey::Int(1), VersionKey::Str("v2")]
    );
    const {
        assert!(!MixedMetaModel::ACCEPTS_UNTAGGED);
    }
}

// =========================================================================
// 2. Opt-in `Migrated<T>` wrapper (source_version, was_migrated, into_inner)
// =========================================================================

#[test]
fn test_migrated_wrapper_tracks_source_version() {
    // Current version -> was_migrated() == false
    let current: Migrated<IntMetaModel> = serde_json::from_str(r#"{"version":2,"x":10}"#).unwrap();
    assert_eq!(current.source_version, Some(2));
    assert!(!current.was_migrated());
    assert_eq!(current.into_inner(), IntMetaModel { x: 10 });

    // Older version -> was_migrated() == true, source_version == Some(1)
    let older: Migrated<IntMetaModel> = serde_json::from_str(r#"{"version":1,"x":10}"#).unwrap();
    assert_eq!(older.source_version, Some(1));
    assert!(older.was_migrated());
    assert_eq!(older.into_inner(), IntMetaModel { x: 11 });

    // Untagged -> was_migrated() == true, source_version == None
    let untagged: Migrated<IntMetaModel> = serde_json::from_str(r#"{"x":10}"#).unwrap();
    assert_eq!(untagged.source_version, None);
    assert!(untagged.was_migrated());
    assert_eq!(untagged.into_inner(), IntMetaModel { x: 10 });

    // Mixed VersionKey tracking
    let mixed_old: Migrated<MixedMetaModel> = serde_json::from_str(r#"{"version":1,"x":5}"#).unwrap();
    assert_eq!(mixed_old.source_version, Some(VersionKey::Int(1)));
    assert!(mixed_old.was_migrated());

    let mixed_cur: Migrated<MixedMetaModel> = serde_json::from_str(r#"{"version":"v2","x":5}"#).unwrap();
    assert_eq!(mixed_cur.source_version, Some(VersionKey::Str("v2")));
    assert!(!mixed_cur.was_migrated());
}

// =========================================================================
// 3. Rolling deployment: `#[serialize]` and `#[serialize(with = fn_path)]`
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct RollV1 {
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollV2 {
    pub name: String,
    pub revision: u32,
}

impl From<RollV1> for RollV2 {
    fn from(v: RollV1) -> Self {
        Self {
            name: v.name,
            revision: 1,
        }
    }
}

// 3a. Bare `#[serialize]` on older version using borrowed `From<&Target> for RollV2` (non-Clone)
#[backwards_compat(
    versions(
        1: RollV1,
        #[serialize]
        2: RollV2,
        #[current]
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct RollingBorrowed {
    pub name: String,
    pub revision: u32,
    pub extra_flag: bool,
}

impl From<RollV2> for RollingBorrowed {
    fn from(v: RollV2) -> Self {
        Self {
            name: v.name,
            revision: v.revision,
            extra_flag: false,
        }
    }
}

impl From<&RollingBorrowed> for RollV2 {
    fn from(t: &RollingBorrowed) -> Self {
        Self {
            name: t.name.clone(),
            revision: t.revision,
        }
    }
}

// 3b. Bare `#[serialize]` on older version using `Clone + From<Target> for RollV2`
#[backwards_compat(
    versions(
        1: RollV1,
        #[serialize]
        2: RollV2,
        #[current]
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollingOwned {
    pub name: String,
    pub revision: u32,
    pub extra_flag: bool,
}

impl From<RollV2> for RollingOwned {
    fn from(v: RollV2) -> Self {
        Self {
            name: v.name,
            revision: v.revision,
            extra_flag: false,
        }
    }
}

impl From<RollingOwned> for RollV2 {
    fn from(t: RollingOwned) -> Self {
        Self {
            name: t.name,
            revision: t.revision,
        }
    }
}

// 3c. `#[serialize(with = downgrade_to_v2)]` without any `From` impl for serialization
fn downgrade_to_v2(t: &RollingWithFn) -> RollV2 {
    RollV2 {
        name: t.name.clone(),
        revision: t.revision,
    }
}

#[backwards_compat(
    versions(
        1: RollV1,
        #[serialize(with = downgrade_to_v2)]
        2: RollV2,
        #[current]
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct RollingWithFn {
    pub name: String,
    pub revision: u32,
    pub extra_flag: bool,
}

impl From<RollV2> for RollingWithFn {
    fn from(v: RollV2) -> Self {
        Self {
            name: v.name,
            revision: v.revision,
            extra_flag: false,
        }
    }
}

#[test]
fn test_rolling_deployment_serialize_older_version() {
    // Metadata distinguishes CURRENT_VERSION (3) from SERIALIZE_VERSION (2)
    assert_eq!(RollingBorrowed::CURRENT_VERSION, 3);
    assert_eq!(RollingBorrowed::SERIALIZE_VERSION, 2);
    assert_eq!(RollingOwned::CURRENT_VERSION, 3);
    assert_eq!(RollingOwned::SERIALIZE_VERSION, 2);
    assert_eq!(RollingWithFn::CURRENT_VERSION, 3);
    assert_eq!(RollingWithFn::SERIALIZE_VERSION, 2);

    // 3a: Reads v1, v2, and v3; writes v2
    let b = RollingBorrowed {
        name: "svc".to_owned(),
        revision: 4,
        extra_flag: true,
    };
    let b_json = serde_json::to_string(&b).unwrap();
    assert_eq!(b_json, r#"{"version":2,"name":"svc","revision":4}"#);
    assert_eq!(
        serde_json::from_str::<RollingBorrowed>(
            r#"{"version":3,"name":"svc","revision":4,"extra_flag":true}"#,
        )
        .unwrap(),
        b
    );

    // 3b: Owned Clone fallback writes v2
    let o = RollingOwned {
        name: "api".to_owned(),
        revision: 5,
        extra_flag: true,
    };
    let o_json = serde_json::to_string(&o).unwrap();
    assert_eq!(o_json, r#"{"version":2,"name":"api","revision":5}"#);

    // 3c: Custom `#[serialize(with = downgrade_to_v2)]` writes v2
    let w = RollingWithFn {
        name: "worker".to_owned(),
        revision: 6,
        extra_flag: true,
    };
    let w_json = serde_json::to_string(&w).unwrap();
    assert_eq!(w_json, r#"{"version":2,"name":"worker","revision":6}"#);
    let w_migrated: Migrated<RollingWithFn> = serde_json::from_str(&w_json).unwrap();
    assert_eq!(w_migrated.source_version, Some(2));
    assert!(w_migrated.was_migrated());
}
