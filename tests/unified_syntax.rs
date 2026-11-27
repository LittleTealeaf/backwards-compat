#![allow(
    clippy::needless_pass_by_value,
    clippy::trivially_copy_pass_by_ref,
    reason = "conversion function signatures are fixed by the backwards_compat macro contract"
)]

use backwards_compat::{BackwardsCompatSerialize as _, backwards_compat};
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Container-level #[migrate(...)] — all four forms (Ok + Err)
// =========================================================================

#[derive(Debug, Deserialize)]
struct MigV1 {
    raw: String,
}

#[derive(Debug, Deserialize)]
struct MigV2 {
    num: i32,
}

#[derive(Debug, Deserialize)]
struct MigV3 {
    label: String,
}

#[derive(Debug, Deserialize)]
struct MigV4 {
    code: i32,
}

fn mig_v3_to_target(v: MigV3) -> UnifiedMig {
    UnifiedMig {
        value: v.label.len() as u64,
        tag: v.label,
    }
}

fn mig_v4_try_to_target(v: MigV4) -> Result<UnifiedMig, &'static str> {
    if v.code < 0 {
        return Err("negative code");
    }
    Ok(UnifiedMig {
        value: v.code.cast_unsigned().into(),
        tag: "v4".to_owned(),
    })
}

#[backwards_compat(
    versions(
        #[migrate(from)]
        1: MigV1 => Self,
        #[migrate(try_from)]
        2: MigV2 => Self,
        #[migrate(with = mig_v3_to_target)]
        3: MigV3 => Self,
        #[migrate(try_with = mig_v4_try_to_target)]
        4: MigV4 => Self,
        5: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct UnifiedMig {
    value: u64,
    tag: String,
}

impl From<MigV1> for UnifiedMig {
    fn from(v: MigV1) -> Self {
        Self { value: 1, tag: v.raw }
    }
}

impl TryFrom<MigV2> for UnifiedMig {
    type Error = &'static str;

    fn try_from(v: MigV2) -> Result<Self, Self::Error> {
        if v.num < 0 {
            return Err("negative num");
        }
        Ok(Self {
            value: v.num.cast_unsigned().into(),
            tag: "v2".to_owned(),
        })
    }
}

#[test]
fn test_container_migrate_all_four_forms() {
    // v1: #[migrate(from)]
    let v1: UnifiedMig = serde_json::from_str(r#"{"version":1,"raw":"hi"}"#).unwrap();
    assert_eq!(
        v1,
        UnifiedMig {
            value: 1,
            tag: "hi".to_owned(),
        }
    );

    // v2: #[migrate(try_from)] Ok & Err
    let v2_ok: UnifiedMig = serde_json::from_str(r#"{"version":2,"num":42}"#).unwrap();
    assert_eq!(
        v2_ok,
        UnifiedMig {
            value: 42,
            tag: "v2".to_owned(),
        }
    );
    let v2_err = serde_json::from_str::<UnifiedMig>(r#"{"version":2,"num":-5}"#).unwrap_err();
    assert!(v2_err.to_string().contains("negative num"));

    // v3: #[migrate(with = ...)]
    let v3: UnifiedMig = serde_json::from_str(r#"{"version":3,"label":"abc"}"#).unwrap();
    assert_eq!(
        v3,
        UnifiedMig {
            value: 3,
            tag: "abc".to_owned(),
        }
    );

    // v4: #[migrate(try_with = ...)] Ok & Err
    let v4_ok: UnifiedMig = serde_json::from_str(r#"{"version":4,"code":9}"#).unwrap();
    assert_eq!(
        v4_ok,
        UnifiedMig {
            value: 9,
            tag: "v4".to_owned(),
        }
    );
    let v4_err = serde_json::from_str::<UnifiedMig>(r#"{"version":4,"code":-1}"#).unwrap_err();
    assert!(v4_err.to_string().contains("negative code"));
}

// =========================================================================
// 2. Field-level #[migrate(...)] and #[serialize(...)]
// =========================================================================

fn field_from_str(s: String) -> u32 {
    u32::try_from(s.len()).unwrap()
}

fn field_try_from_i32(n: i32) -> Result<u32, &'static str> {
    u32::try_from(n).map_err(|_| "negative field")
}

fn field_try_to_i32(v: &u32) -> Result<i32, &'static str> {
    i32::try_from(*v).map_err(|_| "field overflow")
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FieldUnified {
    #[backwards_compat(versions(
        #[migrate(from)]
        1: u16 => Self,
        #[migrate(try_from)]
        2: i64 => Self,
        #[migrate(with = field_from_str)]
        3: String => Self,
        #[migrate(try_with = field_try_from_i32)]
        #[serialize(try_with = field_try_to_i32)]
        4: i32 => Self,
        #[current]
        5: Self,
    ))]
    count: u32,
}

#[test]
fn test_field_level_migrate_and_serialize_try_with() {
    let f1: FieldUnified = serde_json::from_str(r#"{"count":{"version":1,"data":7}}"#).unwrap();
    assert_eq!(f1.count, 7);

    let f2_ok: FieldUnified = serde_json::from_str(r#"{"count":{"version":2,"data":100}}"#).unwrap();
    assert_eq!(f2_ok.count, 100);
    let _ = serde_json::from_str::<FieldUnified>(r#"{"count":{"version":2,"data":-1}}"#).unwrap_err();

    let f3: FieldUnified = serde_json::from_str(r#"{"count":{"version":3,"data":"abcd"}}"#).unwrap();
    assert_eq!(f3.count, 4);

    let f4_ok: FieldUnified = serde_json::from_str(r#"{"count":{"version":4,"data":55}}"#).unwrap();
    assert_eq!(f4_ok.count, 55);
    let f4_err = serde_json::from_str::<FieldUnified>(r#"{"count":{"version":4,"data":-3}}"#).unwrap_err();
    assert!(f4_err.to_string().contains("negative field"));

    // Field-level #[serialize(try_with = ...)] Ok & Err
    let ok_val = serde_json::to_value(&FieldUnified { count: 12 }).unwrap();
    assert_eq!(ok_val, serde_json::json!({"count": {"version": 4, "data": 12}}));

    let err = serde_json::to_string(&FieldUnified { count: u32::MAX }).unwrap_err();
    assert!(err.to_string().contains("field overflow"));
}

// =========================================================================
// 3. #[serialize(into)], #[serialize(try_into)] (borrowed & clone fallback),
//    #[serialize(with = ...)], #[serialize(try_with = ...)]
// =========================================================================

#[derive(Debug, Serialize, Deserialize)]
struct SerWireV1 {
    small: u8,
}

// 3a. #[serialize(into)]
#[backwards_compat(
    versions(
        #[serialize(into)]
        1: SerWireV1,
        #[current]
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerIntoModel {
    val: u8,
}

impl From<SerWireV1> for SerIntoModel {
    fn from(v: SerWireV1) -> Self {
        Self { val: v.small }
    }
}

impl From<&SerIntoModel> for SerWireV1 {
    fn from(v: &SerIntoModel) -> Self {
        Self { small: v.val }
    }
}

// 3b. #[serialize(try_into)] with borrowed zero-clone TryFrom<&Self> (does NOT derive Clone)
#[backwards_compat(
    versions(
        #[serialize(try_into)]
        1: SerWireV1,
        #[current]
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerTryIntoBorrowed {
    val: u16,
}

impl From<SerWireV1> for SerTryIntoBorrowed {
    fn from(v: SerWireV1) -> Self {
        Self {
            val: u16::from(v.small),
        }
    }
}

impl TryFrom<&SerTryIntoBorrowed> for SerWireV1 {
    type Error = &'static str;

    fn try_from(v: &SerTryIntoBorrowed) -> Result<Self, Self::Error> {
        let small = u8::try_from(v.val).map_err(|_| "borrowed overflow")?;
        Ok(Self { small })
    }
}

// 3c. #[serialize(try_into)] with Clone + TryFrom<Self> fallback
#[backwards_compat(
    versions(
        #[serialize(try_into)]
        1: SerWireV1,
        #[current]
        2: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct SerTryIntoCloneFallback {
    val: u16,
}

impl From<SerWireV1> for SerTryIntoCloneFallback {
    fn from(v: SerWireV1) -> Self {
        Self {
            val: u16::from(v.small),
        }
    }
}

impl TryFrom<SerTryIntoCloneFallback> for SerWireV1 {
    type Error = &'static str;

    fn try_from(v: SerTryIntoCloneFallback) -> Result<Self, Self::Error> {
        let small = u8::try_from(v.val).map_err(|_| "owned overflow")?;
        Ok(Self { small })
    }
}

// 3d. #[serialize(with = ...)] and #[serialize(try_with = ...)]
fn ser_with_fn(v: &SerWithModel) -> SerWireV1 {
    SerWireV1 { small: v.val }
}

#[backwards_compat(
    versions(
        #[serialize(with = ser_with_fn)]
        1: SerWireV1,
        #[current]
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerWithModel {
    val: u8,
}

impl From<SerWireV1> for SerWithModel {
    fn from(v: SerWireV1) -> Self {
        Self { val: v.small }
    }
}

fn ser_try_with_fn(v: &SerTryWithModel) -> Result<SerWireV1, &'static str> {
    let small = u8::try_from(v.val).map_err(|_| "try_with overflow")?;
    Ok(SerWireV1 { small })
}

#[backwards_compat(
    versions(
        #[serialize(try_with = ser_try_with_fn)]
        1: SerWireV1,
        #[current]
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct SerTryWithModel {
    val: u16,
}

impl From<SerWireV1> for SerTryWithModel {
    fn from(v: SerWireV1) -> Self {
        Self {
            val: u16::from(v.small),
        }
    }
}

#[test]
fn test_serialize_all_four_modes_including_borrowed_and_clone_try_into() {
    assert_eq!(
        serde_json::to_value(SerIntoModel { val: 5 }).unwrap(),
        serde_json::json!({"version": 1, "small": 5})
    );

    // Borrowed zero-clone TryFrom<&T>
    assert_eq!(
        serde_json::to_value(SerTryIntoBorrowed { val: 200 }).unwrap(),
        serde_json::json!({"version": 1, "small": 200})
    );
    let err_borrowed = serde_json::to_string(&SerTryIntoBorrowed { val: 300 }).unwrap_err();
    assert!(err_borrowed.to_string().contains("borrowed overflow"));

    // Clone + TryFrom<T> fallback
    assert_eq!(
        serde_json::to_value(SerTryIntoCloneFallback { val: 255 }).unwrap(),
        serde_json::json!({"version": 1, "small": 255})
    );
    let err_owned = serde_json::to_string(&SerTryIntoCloneFallback { val: 256 }).unwrap_err();
    assert!(err_owned.to_string().contains("owned overflow"));

    // with & try_with
    assert_eq!(
        serde_json::to_value(SerWithModel { val: 9 }).unwrap(),
        serde_json::json!({"version": 1, "small": 9})
    );
    assert_eq!(
        serde_json::to_value(SerTryWithModel { val: 7 }).unwrap(),
        serde_json::json!({"version": 1, "small": 7})
    );
    let err_try_with = serde_json::to_string(&SerTryWithModel { val: 999 }).unwrap_err();
    assert!(err_try_with.to_string().contains("try_with overflow"));
}

// =========================================================================
// 4. #[serializable(into)], #[serializable(try_into)], with, try_with
// =========================================================================

#[derive(Debug, Serialize, Deserialize)]
struct MultiV1 {
    a: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct MultiV2 {
    b: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct MultiV3 {
    c: u16,
}

#[derive(Debug, Serialize, Deserialize)]
struct MultiV4 {
    d: u8,
}

fn multi_to_v3(m: &MultiSerializable) -> MultiV3 {
    MultiV3 { c: m.val }
}

fn multi_try_to_v4(m: &MultiSerializable) -> Result<MultiV4, &'static str> {
    let d = u8::try_from(m.val).map_err(|_| "v4 overflow")?;
    Ok(MultiV4 { d })
}

#[backwards_compat(
    versions(
        #[serializable(into)]
        1: MultiV1 => Self,
        #[serializable(try_into)]
        2: MultiV2 => Self,
        #[serializable(with = multi_to_v3)]
        3: MultiV3 => Self,
        #[serializable(try_with = multi_try_to_v4)]
        4: MultiV4 => Self,
        5: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct MultiSerializable {
    val: u16,
}

impl From<MultiV1> for MultiSerializable {
    fn from(v: MultiV1) -> Self {
        Self { val: u16::from(v.a) }
    }
}
impl From<MultiV2> for MultiSerializable {
    fn from(v: MultiV2) -> Self {
        Self { val: u16::from(v.b) }
    }
}
impl From<MultiV3> for MultiSerializable {
    fn from(v: MultiV3) -> Self {
        Self { val: v.c }
    }
}
impl From<MultiV4> for MultiSerializable {
    fn from(v: MultiV4) -> Self {
        Self { val: u16::from(v.d) }
    }
}

impl From<&MultiSerializable> for MultiV1 {
    fn from(m: &MultiSerializable) -> Self {
        Self {
            a: u8::try_from(m.val).unwrap_or(u8::MAX),
        }
    }
}

impl TryFrom<&MultiSerializable> for MultiV2 {
    type Error = &'static str;

    fn try_from(m: &MultiSerializable) -> Result<Self, Self::Error> {
        let b = u8::try_from(m.val).map_err(|_| "v2 overflow")?;
        Ok(Self { b })
    }
}

#[test]
fn test_serializable_all_four_modes() {
    let ok = MultiSerializable { val: 42 };
    assert_eq!(
        serde_json::to_value(ok.as_version(1)).unwrap(),
        serde_json::json!({"version": 1, "a": 42})
    );
    assert_eq!(
        serde_json::to_value(ok.as_version(2)).unwrap(),
        serde_json::json!({"version": 2, "b": 42})
    );
    assert_eq!(
        serde_json::to_value(ok.as_version(3)).unwrap(),
        serde_json::json!({"version": 3, "c": 42})
    );
    assert_eq!(
        serde_json::to_value(ok.as_version(4)).unwrap(),
        serde_json::json!({"version": 4, "d": 42})
    );

    let big = MultiSerializable { val: 500 };
    let err_v2 = serde_json::to_string(&big.as_version(2)).unwrap_err();
    assert!(err_v2.to_string().contains("v2 overflow"));
    let err_v4 = serde_json::to_string(&big.as_version(4)).unwrap_err();
    assert!(err_v4.to_string().contains("v4 overflow"));
}

// =========================================================================
// 5. `via` mode with try_from = fn_path / try_from(fn_path) and try_into
// =========================================================================

#[backwards_compat(versions(1: Self))]
#[derive(Debug, PartialEq, Eq)]
struct ViaProxyNum {
    n: i32,
}

fn via_decode(p: ViaProxyNum) -> Result<ViaEqDomain, &'static str> {
    if p.n < 0 {
        return Err("via decode negative");
    }
    Ok(ViaEqDomain {
        n: p.n.cast_unsigned(),
    })
}

fn via_encode(d: &ViaEqDomain) -> Result<ViaProxyNum, &'static str> {
    let n = i32::try_from(d.n).map_err(|_| "via encode overflow")?;
    Ok(ViaProxyNum { n })
}

#[backwards_compat(via = ViaProxyNum, try_from = via_decode, try_into = via_encode)]
#[derive(Debug, PartialEq, Eq)]
struct ViaEqDomain {
    n: u32,
}

fn via_paren_decode(p: ViaProxyNum) -> Result<ViaParenDomain, &'static str> {
    if p.n == 0 {
        return Err("zero not allowed");
    }
    Ok(ViaParenDomain { n: p.n })
}

fn via_paren_encode(d: &ViaParenDomain) -> Result<ViaProxyNum, &'static str> {
    if d.n == 0 {
        return Err("cannot encode zero");
    }
    Ok(ViaProxyNum { n: d.n })
}

#[backwards_compat(via(ViaProxyNum), try_from(via_paren_decode), try_into(via_paren_encode))]
#[derive(Debug, PartialEq, Eq)]
struct ViaParenDomain {
    n: i32,
}

#[test]
fn test_via_try_from_and_try_into_fn_paths() {
    let ok: ViaEqDomain = serde_json::from_str(r#"{"version":1,"n":10}"#).unwrap();
    assert_eq!(ok, ViaEqDomain { n: 10 });
    assert_eq!(
        serde_json::to_value(&ok).unwrap(),
        serde_json::json!({"version": 1, "n": 10})
    );

    let err_de = serde_json::from_str::<ViaEqDomain>(r#"{"version":1,"n":-1}"#).unwrap_err();
    assert!(err_de.to_string().contains("via decode negative"));

    let err_ser = serde_json::to_string(&ViaEqDomain { n: u32::MAX }).unwrap_err();
    assert!(err_ser.to_string().contains("via encode overflow"));

    let ok_p: ViaParenDomain = serde_json::from_str(r#"{"version":1,"n":5}"#).unwrap();
    assert_eq!(ok_p, ViaParenDomain { n: 5 });
    assert_eq!(
        serde_json::to_value(&ok_p).unwrap(),
        serde_json::json!({"version": 1, "n": 5})
    );

    let err_p_de = serde_json::from_str::<ViaParenDomain>(r#"{"version":1,"n":0}"#).unwrap_err();
    assert!(err_p_de.to_string().contains("zero not allowed"));
    let err_p_ser = serde_json::to_string(&ViaParenDomain { n: 0 }).unwrap_err();
    assert!(err_p_ser.to_string().contains("cannot encode zero"));
}

// =========================================================================
// 6. Legacy #[fallible] and #[with(...)] under #[allow(deprecated)]
// =========================================================================

fn legacy_with_v2(v: MigV3) -> LegacyContainer {
    LegacyContainer {
        value: v.label.len() as u64,
    }
}

fn legacy_fallible_with_v3(v: MigV4) -> Result<LegacyContainer, &'static str> {
    if v.code < 0 {
        return Err("legacy negative");
    }
    Ok(LegacyContainer {
        value: v.code.cast_unsigned().into(),
    })
}

#[allow(deprecated, reason = "explicitly testing deprecated legacy syntax")]
#[backwards_compat(
    versions(
        #[fallible]
        1: MigV2 => Self,
        #[with(legacy_with_v2)]
        2: MigV3 => Self,
        #[fallible]
        #[with(legacy_fallible_with_v3)]
        3: MigV4 => Self,
        4: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct LegacyContainer {
    value: u64,
}

impl TryFrom<MigV2> for LegacyContainer {
    type Error = &'static str;

    fn try_from(v: MigV2) -> Result<Self, Self::Error> {
        if v.num < 0 {
            return Err("legacy v1 negative");
        }
        Ok(Self {
            value: v.num.cast_unsigned().into(),
        })
    }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LegacyFieldContainer {
    #[allow(deprecated, reason = "explicitly testing deprecated legacy field syntax")]
    #[backwards_compat(versions(
        #[fallible]
        1: i64 => Self,
        #[with(field_from_str)]
        2: String => Self,
        #[fallible]
        #[with(field_try_from_i32)]
        3: i32 => Self,
        4: Self,
    ))]
    count: u32,
}

#[test]
fn test_legacy_fallible_and_with_still_work_with_allow_deprecated() {
    let c1: LegacyContainer = serde_json::from_str(r#"{"version":1,"num":8}"#).unwrap();
    assert_eq!(c1, LegacyContainer { value: 8 });
    let _ = serde_json::from_str::<LegacyContainer>(r#"{"version":1,"num":-1}"#).unwrap_err();

    let c2: LegacyContainer = serde_json::from_str(r#"{"version":2,"label":"hello"}"#).unwrap();
    assert_eq!(c2, LegacyContainer { value: 5 });

    let c3: LegacyContainer = serde_json::from_str(r#"{"version":3,"code":11}"#).unwrap();
    assert_eq!(c3, LegacyContainer { value: 11 });
    let c3_err = serde_json::from_str::<LegacyContainer>(r#"{"version":3,"code":-2}"#).unwrap_err();
    assert!(c3_err.to_string().contains("legacy negative"));

    let f1: LegacyFieldContainer = serde_json::from_str(r#"{"count":{"version":1,"data":3}}"#).unwrap();
    assert_eq!(f1.count, 3);
    let f2: LegacyFieldContainer = serde_json::from_str(r#"{"count":{"version":2,"data":"xy"}}"#).unwrap();
    assert_eq!(f2.count, 2);
    let f3: LegacyFieldContainer = serde_json::from_str(r#"{"count":{"version":3,"data":9}}"#).unwrap();
    assert_eq!(f3.count, 9);
    let _ = serde_json::from_str::<LegacyFieldContainer>(r#"{"count":{"version":3,"data":-1}}"#).unwrap_err();
}
