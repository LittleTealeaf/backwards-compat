use backwards_compat::{BackwardsCompat, BackwardsCompatSerialize, Migrated, Tagging, backwards_compat};
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. BackwardsCompat metadata trait (u64, &'static str, Tagging)
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
    external,
    versions(
        1: MetaV1 => 2,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ExternalMetaModel {
    pub x: i32,
}

impl From<MetaV1> for ExternalMetaModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x + 2 }
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

    // External tagging metadata
    assert_eq!(ExternalMetaModel::CURRENT_VERSION, 2);
    assert_eq!(ExternalMetaModel::SERIALIZE_VERSION, 2);
    assert_eq!(ExternalMetaModel::SUPPORTED_VERSIONS, &[1, 2]);
    const {
        assert!(!ExternalMetaModel::ACCEPTS_UNTAGGED);
        assert!(matches!(ExternalMetaModel::TAGGING, Tagging::External));
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

    // External tagging Migrated<T> tracking
    let ext_old: Migrated<ExternalMetaModel> = serde_json::from_str(r#"{"1":{"x":5}}"#).unwrap();
    assert_eq!(ext_old.source_version, Some(1));
    assert!(ext_old.was_migrated());
    assert_eq!(ext_old.into_inner(), ExternalMetaModel { x: 7 });

    let ext_cur: Migrated<ExternalMetaModel> = serde_json::from_str(r#"{"2":{"x":5}}"#).unwrap();
    assert_eq!(ext_cur.source_version, Some(2));
    assert!(!ext_cur.was_migrated());
    assert_eq!(ext_cur.into_inner(), ExternalMetaModel { x: 5 });
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

// =========================================================================
// 4. TAGGING metadata constant + Migrated<T> ergonomics
// =========================================================================

#[backwards_compat(
    tag = "schema",
    versions(
        1: MetaV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomTagOnlyModel {
    pub x: i32,
}

impl From<MetaV1> for CustomTagOnlyModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x }
    }
}

#[backwards_compat(
    content = "data",
    versions(
        1: MetaV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomContentOnlyModel {
    pub x: i32,
}

impl From<MetaV1> for CustomContentOnlyModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x }
    }
}

#[backwards_compat(
    tag = "v",
    content = "payload",
    versions(
        1: MetaV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomTagAndContentModel {
    pub x: i32,
}

impl From<MetaV1> for CustomTagAndContentModel {
    fn from(v: MetaV1) -> Self {
        Self { x: v.x }
    }
}

#[test]
fn test_tagging_metadata_constants() {
    const {
        assert!(matches!(
            IntMetaModel::TAGGING,
            Tagging::Internal { tag } if matches!(tag.as_bytes(), b"version")
        ));
    }
    assert_eq!(IntMetaModel::TAGGING, Tagging::Internal { tag: "version" });
    assert_eq!(StrMetaModel::TAGGING, Tagging::Internal { tag: "version" });
    assert_eq!(ExternalMetaModel::TAGGING, Tagging::External);

    assert_eq!(CustomTagOnlyModel::TAGGING, Tagging::Internal { tag: "schema" });
    assert_eq!(
        CustomContentOnlyModel::TAGGING,
        Tagging::Adjacent {
            tag: "version",
            content: "data",
        }
    );
    assert_eq!(
        CustomTagAndContentModel::TAGGING,
        Tagging::Adjacent {
            tag: "v",
            content: "payload",
        }
    );
}

#[test]
fn test_migrated_ergonomics() {
    fn take_ref(model: &IntMetaModel) -> i32 {
        model.x
    }

    // Tagged current version
    let mut current: Migrated<IntMetaModel> = serde_json::from_str(r#"{"version":2,"x":10}"#).unwrap();
    assert!(!current.is_untagged(), "v2 payload should not be untagged");
    assert_eq!(current.source_version(), Some(2));
    assert_eq!(current.x, 10); // Deref
    assert_eq!(take_ref(current.as_ref()), 10); // AsRef
    current.x += 5; // DerefMut
    assert_eq!(*current, IntMetaModel { x: 15 });

    // Tagged older version + map() preserving source_version and transforming target type
    let older: Migrated<IntMetaModel> = serde_json::from_str(r#"{"version":1,"x":10}"#).unwrap();
    assert!(!older.is_untagged(), "v1 payload should not be untagged");
    assert_eq!(older.source_version(), Some(1));
    let mapped_tagged = older.map(|m| CustomTagOnlyModel { x: m.x * 2 });
    assert_eq!(mapped_tagged.source_version(), Some(1));
    assert!(!mapped_tagged.is_untagged());
    assert_eq!(*mapped_tagged, CustomTagOnlyModel { x: 22 });

    // Untagged `_` payload + map() preserving None source_version
    let untagged: Migrated<IntMetaModel> = serde_json::from_str(r#"{"x":7}"#).unwrap();
    assert!(untagged.is_untagged(), "unversioned payload should be untagged");
    assert_eq!(untagged.source_version(), None);
    let mapped_untagged = untagged.map(|m| IntMetaModel { x: m.x + 100 });
    assert!(mapped_untagged.is_untagged());
    assert_eq!(mapped_untagged.source_version(), None);
    assert_eq!(mapped_untagged.into_inner(), IntMetaModel { x: 107 });
}

// =========================================================================
// 5. BackwardsCompatSerialize: runtime version selection, #[serializable],
//    #[serializable(with = ...)], #[serializable(try_with = ...)],
//    adjacent tagging, via delegation, and unsupported version errors
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiSerV1 {
    pub label: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiSerV2 {
    pub label: String,
    pub count: u16,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiSerV3 {
    pub label: String,
    pub count: u32,
}

impl From<MultiSerV1> for MultiSerV2 {
    fn from(v: MultiSerV1) -> Self {
        Self {
            label: v.label,
            count: 0,
        }
    }
}

impl From<MultiSerV2> for MultiSerV3 {
    fn from(v: MultiSerV2) -> Self {
        Self {
            label: v.label,
            count: u32::from(v.count),
        }
    }
}

fn to_v2_infallible(m: &MultiSerModel) -> MultiSerV2 {
    MultiSerV2 {
        label: m.label.clone(),
        count: u16::try_from(m.count).unwrap_or(u16::MAX),
    }
}

fn try_to_v3(m: &MultiSerModel) -> Result<MultiSerV3, String> {
    if m.label.is_empty() {
        return Err("label cannot be empty for v3".to_owned());
    }
    Ok(MultiSerV3 {
        label: m.label.clone(),
        count: m.count,
    })
}

#[backwards_compat(
    versions(
        _: MultiSerV1 => 1,
        #[cfg(any())]
        0: MultiSerV1 => 1,
        #[serializable]
        1: MultiSerV1,
        #[serializable(with = to_v2_infallible)]
        2: MultiSerV2,
        #[serializable(try_with = try_to_v3)]
        3: MultiSerV3,
        4: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct MultiSerModel {
    pub label: String,
    pub count: u32,
    pub extra: bool,
}

impl From<MultiSerV3> for MultiSerModel {
    fn from(v: MultiSerV3) -> Self {
        Self {
            label: v.label,
            count: v.count,
            extra: false,
        }
    }
}

impl From<&MultiSerModel> for MultiSerV1 {
    fn from(m: &MultiSerModel) -> Self {
        Self {
            label: m.label.clone(),
        }
    }
}

// Adjacently-tagged BackwardsCompatSerialize model using Clone + From<Owned> for v1
#[backwards_compat(
    tag = "v",
    content = "data",
    versions(
        #[serializable]
        1: MultiSerV1,
        2: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjMultiSerModel {
    pub label: String,
    pub count: u32,
}

impl From<MultiSerV1> for AdjMultiSerModel {
    fn from(v: MultiSerV1) -> Self {
        Self {
            label: v.label,
            count: 0,
        }
    }
}

impl From<AdjMultiSerModel> for MultiSerV1 {
    fn from(m: AdjMultiSerModel) -> Self {
        Self { label: m.label }
    }
}

// Proxy + #[backwards_compat(via = ...)] delegation
#[backwards_compat(via = MultiSerModel)]
#[derive(Debug, PartialEq, Eq)]
pub struct ViaMultiSerDomain {
    pub label: String,
    pub count: u32,
    pub extra: bool,
}

impl From<MultiSerModel> for ViaMultiSerDomain {
    fn from(p: MultiSerModel) -> Self {
        Self {
            label: p.label,
            count: p.count,
            extra: p.extra,
        }
    }
}

impl From<&ViaMultiSerDomain> for MultiSerModel {
    fn from(d: &ViaMultiSerDomain) -> Self {
        Self {
            label: d.label.clone(),
            count: d.count,
            extra: d.extra,
        }
    }
}

#[test]
fn test_backwards_compat_serialize_and_via_delegation() {
    // Default model without #[serializable] only includes its serialize version
    assert_eq!(IntMetaModel::SERIALIZABLE_VERSIONS, &[2]);

    // MultiSerModel exposes 1, 2, 3, 4 (excludes `_` and #[cfg(any())] 0)
    assert_eq!(MultiSerModel::SERIALIZABLE_VERSIONS, &[1, 2, 3, 4]);
    assert_eq!(AdjMultiSerModel::SERIALIZABLE_VERSIONS, &[1, 2]);
    assert_eq!(ViaMultiSerDomain::SERIALIZABLE_VERSIONS, &[1, 2, 3, 4]);
    assert_eq!(ViaMultiSerDomain::TAGGING, MultiSerModel::TAGGING);

    let model = MultiSerModel {
        label: "svc".to_owned(),
        count: 42,
        extra: true,
    };

    // #[serializable] (v1) via as_version and serialize_as_version
    let v1_json = serde_json::to_string(&model.as_version(1)).unwrap();
    assert_eq!(v1_json, r#"{"version":1,"label":"svc"}"#);
    let mut direct_v1_ser = serde_json::Serializer::new(Vec::new());
    model.serialize_as_version(1, &mut direct_v1_ser).unwrap();
    assert_eq!(String::from_utf8(direct_v1_ser.into_inner()).unwrap(), v1_json);

    // #[serializable(with = to_v2_infallible)] (v2)
    let v2_json = serde_json::to_string(&model.as_version(2)).unwrap();
    assert_eq!(v2_json, r#"{"version":2,"label":"svc","count":42}"#);

    // #[serializable(try_with = try_to_v3)] Ok case (v3)
    let v3_json = serde_json::to_string(&model.as_version(3)).unwrap();
    assert_eq!(v3_json, r#"{"version":3,"label":"svc","count":42}"#);

    // Current version (v4)
    let v4_json = serde_json::to_string(&model.as_version(4)).unwrap();
    assert_eq!(v4_json, r#"{"version":4,"label":"svc","count":42,"extra":true}"#);

    // #[serializable(try_with = try_to_v3)] Err case
    let invalid_for_v3 = MultiSerModel {
        label: String::new(),
        count: 1,
        extra: false,
    };
    let try_err = serde_json::to_string(&invalid_for_v3.as_version(3)).unwrap_err();
    assert!(
        try_err.to_string().contains("label cannot be empty for v3"),
        "unexpected try_with error: {try_err}"
    );

    // Unsupported runtime version error
    let unsupported_err = serde_json::to_string(&model.as_version(99)).unwrap_err();
    assert!(
        unsupported_err
            .to_string()
            .contains("version 99 is not supported for serialization"),
        "unexpected unsupported version error: {unsupported_err}"
    );
    let mut bad_ser = serde_json::Serializer::new(Vec::new());
    let direct_err = model.serialize_as_version(99, &mut bad_ser).unwrap_err();
    assert!(
        direct_err
            .to_string()
            .contains("version 99 is not supported for serialization"),
        "unexpected direct serialize_as_version error: {direct_err}"
    );

    // Adjacently-tagged runtime version serialization
    let adj = AdjMultiSerModel {
        label: "adj".to_owned(),
        count: 9,
    };
    let adj_v1 = serde_json::to_string(&adj.as_version(1)).unwrap();
    assert_eq!(adj_v1, r#"{"v":1,"data":{"label":"adj"}}"#);
    let adj_v2 = serde_json::to_string(&adj.as_version(2)).unwrap();
    assert_eq!(adj_v2, r#"{"v":2,"data":{"label":"adj","count":9}}"#);
    assert_eq!(
        serde_json::from_str::<AdjMultiSerModel>(&adj_v1).unwrap(),
        AdjMultiSerModel {
            label: "adj".to_owned(),
            count: 0,
        }
    );

    // #[backwards_compat(via = ...)] delegation
    let via_domain = ViaMultiSerDomain {
        label: "via".to_owned(),
        count: 12,
        extra: true,
    };
    let via_v2 = serde_json::to_string(&via_domain.as_version(2)).unwrap();
    assert_eq!(via_v2, r#"{"version":2,"label":"via","count":12}"#);
    let mut via_ser = serde_json::Serializer::new(Vec::new());
    via_domain.serialize_as_version(3, &mut via_ser).unwrap();
    assert_eq!(
        String::from_utf8(via_ser.into_inner()).unwrap(),
        r#"{"version":3,"label":"via","count":12}"#
    );
    let via_err = serde_json::to_string(&via_domain.as_version(77)).unwrap_err();
    assert!(
        via_err
            .to_string()
            .contains("version 77 is not supported for serialization"),
        "unexpected via unsupported version error: {via_err}"
    );
}
