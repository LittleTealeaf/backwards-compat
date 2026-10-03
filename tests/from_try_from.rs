use backwards_compat::backwards_compat;
use core::error::Error;
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Direct in-code `From` conversion (Infallible multi-hop chain)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfallibleV1 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfallibleV2 {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfallibleV3 {
    pub name: String,
    pub count: u32,
    pub extra: String,
}

impl From<InfallibleV1> for InfallibleV2 {
    fn from(v1: InfallibleV1) -> Self {
        Self {
            name: v1.name,
            count: 10,
        }
    }
}

impl From<InfallibleV2> for InfallibleV3 {
    fn from(v2: InfallibleV2) -> Self {
        Self {
            name: v2.name,
            count: v2.count,
            extra: "default_extra".to_owned(),
        }
    }
}

#[backwards_compat(
    tag = "v",
    version = 3,
    versions(1: InfallibleV1, 2: InfallibleV2, 3: InfallibleV3)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfallibleTarget {
    pub name: String,
    pub count: u32,
    pub extra: String,
}

impl From<InfallibleV3> for InfallibleTarget {
    fn from(v3: InfallibleV3) -> Self {
        Self {
            name: v3.name,
            count: v3.count,
            extra: v3.extra,
        }
    }
}

impl From<InfallibleTarget> for InfallibleV3 {
    fn from(target: InfallibleTarget) -> Self {
        Self {
            name: target.name,
            count: target.count,
            extra: target.extra,
        }
    }
}

#[test]
fn test_infallible_from_and_into() {
    let v1 = InfallibleV1 {
        name: "device_a".to_owned(),
    };

    // Direct Target::from(v1)
    let target_from_v1 = InfallibleTarget::from(v1.clone());
    // Direct let target: Target = v1.into()
    let target_into_v1: InfallibleTarget = v1.into();

    let expected = InfallibleTarget {
        name: "device_a".to_owned(),
        count: 10,
        extra: "default_extra".to_owned(),
    };

    assert_eq!(target_from_v1, expected);
    assert_eq!(target_into_v1, expected);

    let v2 = InfallibleV2 {
        name: "device_b".to_owned(),
        count: 42,
    };
    let target_from_v2 = InfallibleTarget::from(v2.clone());
    let target_into_v2: InfallibleTarget = v2.into();

    let expected_v2 = InfallibleTarget {
        name: "device_b".to_owned(),
        count: 42,
        extra: "default_extra".to_owned(),
    };

    assert_eq!(target_from_v2, expected_v2);
    assert_eq!(target_into_v2, expected_v2);

    let v3 = InfallibleV3 {
        name: "device_c".to_owned(),
        count: 99,
        extra: "custom_extra".to_owned(),
    };
    let target_from_v3 = InfallibleTarget::from(v3.clone());
    let target_into_v3: InfallibleTarget = v3.into();

    let expected_v3 = InfallibleTarget {
        name: "device_c".to_owned(),
        count: 99,
        extra: "custom_extra".to_owned(),
    };

    assert_eq!(target_from_v3, expected_v3);
    assert_eq!(target_into_v3, expected_v3);
}

// =========================================================================
// 2. Direct in-code `TryFrom` conversion with custom `#[error = MyError]`
// =========================================================================

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CustomModelError {
    #[error("Value exceeds limit at step 1: {0}")]
    LimitExceededV1(u32),
    #[error("Value exceeds limit at step 2: {0}")]
    LimitExceededV2(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomErrorV1 {
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomErrorV2 {
    pub value: u32,
}

impl TryFrom<CustomErrorV1> for CustomErrorV2 {
    type Error = CustomModelError;

    fn try_from(v1: CustomErrorV1) -> Result<Self, Self::Error> {
        if v1.value > 100 {
            return Err(CustomModelError::LimitExceededV1(v1.value));
        }
        Ok(Self { value: v1.value * 2 })
    }
}

#[backwards_compat(
    tag = "version",
    version = 2,
    error = CustomModelError,
    versions(#[fallible] 1: CustomErrorV1, #[fallible] 2: CustomErrorV2)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomErrorTarget {
    pub value: u32,
}

impl TryFrom<CustomErrorV2> for CustomErrorTarget {
    type Error = CustomModelError;

    fn try_from(v2: CustomErrorV2) -> Result<Self, Self::Error> {
        if v2.value > 150 {
            return Err(CustomModelError::LimitExceededV2(v2.value));
        }
        Ok(Self { value: v2.value })
    }
}

impl From<CustomErrorTarget> for CustomErrorV2 {
    fn from(t: CustomErrorTarget) -> Self {
        Self { value: t.value }
    }
}

#[test]
fn test_custom_error_try_from_success_and_failure() {
    // Successful conversion
    let v1_ok = CustomErrorV1 { value: 50 };
    let res: Result<CustomErrorTarget, CustomModelError> = CustomErrorTarget::try_from(v1_ok.clone());
    assert_eq!(res, Ok(CustomErrorTarget { value: 100 }));

    let res_into: Result<CustomErrorTarget, CustomModelError> = v1_ok.try_into();
    assert_eq!(res_into, Ok(CustomErrorTarget { value: 100 }));

    // Failing at step 1
    let v1_fail_step1 = CustomErrorV1 { value: 105 };
    let res_err1 = CustomErrorTarget::try_from(v1_fail_step1);
    assert_eq!(res_err1, Err(CustomModelError::LimitExceededV1(105)));

    // Failing at step 2 (80 * 2 = 160 > 150)
    let v1_fail_step2 = CustomErrorV1 { value: 80 };
    let res_err2 = CustomErrorTarget::try_from(v1_fail_step2);
    assert_eq!(res_err2, Err(CustomModelError::LimitExceededV2(160)));
}

// =========================================================================
// 3. Direct in-code `TryFrom` without `#[error = ...]` (fallback to Box<dyn Error + Send + Sync>)
// =========================================================================

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Parse error: {0}")]
pub struct StepError(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxedV1 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxedV2 {
    pub parsed: u64,
}

impl TryFrom<BoxedV1> for BoxedV2 {
    type Error = StepError;

    fn try_from(v1: BoxedV1) -> Result<Self, Self::Error> {
        let parsed = v1.raw.parse::<u64>().map_err(|e| StepError(e.to_string()))?;
        Ok(Self { parsed })
    }
}

#[backwards_compat(
    tag = "ver",
    version = 2,
    versions(#[fallible] 1: BoxedV1, #[fallible] 2: BoxedV2)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxedTarget {
    pub parsed: u64,
}

impl TryFrom<BoxedV2> for BoxedTarget {
    type Error = StepError;

    fn try_from(v2: BoxedV2) -> Result<Self, Self::Error> {
        if v2.parsed == 0 {
            return Err(StepError("zero is not allowed".to_owned()));
        }
        Ok(Self { parsed: v2.parsed })
    }
}

impl From<BoxedTarget> for BoxedV2 {
    fn from(t: BoxedTarget) -> Self {
        Self { parsed: t.parsed }
    }
}

#[test]
fn test_boxed_error_try_from_success_and_failure() {
    // Success
    let v1_valid = BoxedV1 {
        raw: "12345".to_owned(),
    };
    let target = BoxedTarget::try_from(v1_valid).expect("should succeed");
    assert_eq!(target, BoxedTarget { parsed: 12345 });

    // Failure in step 1
    let v1_invalid = BoxedV1 {
        raw: "not_a_number".to_owned(),
    };
    let err: Result<BoxedTarget, Box<dyn Error + Send + Sync>> = BoxedTarget::try_from(v1_invalid);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("invalid digit"));

    // Failure in step 2
    let v1_zero = BoxedV1 { raw: "0".to_owned() };
    let err2 = BoxedTarget::try_from(v1_zero);
    assert!(err2.is_err());
    assert!(err2.unwrap_err().to_string().contains("zero is not allowed"));
}

// =========================================================================
// 4. Multi-hop DAG jump with manual conversion (e.g. 1: V1 => 3, 2: V2 => 3, 3: V3)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagV1 {
    pub legacy_payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagV2 {
    pub intermediate_payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagV3 {
    pub full_name: String,
    pub migrated_from: u32,
}

// Direct jump from DagV1 to DagV3 (bypassing DagV2)
impl From<DagV1> for DagV3 {
    fn from(v1: DagV1) -> Self {
        Self {
            full_name: format!("v1_migrated:{}", v1.legacy_payload),
            migrated_from: 1,
        }
    }
}

// Jump from DagV2 to DagV3
impl From<DagV2> for DagV3 {
    fn from(v2: DagV2) -> Self {
        Self {
            full_name: format!("v2_migrated:{}", v2.intermediate_payload),
            migrated_from: 2,
        }
    }
}

#[backwards_compat(
    tag = "dag_ver",
    version = 3,
    versions(1: DagV1 => 3, 2: DagV2 => 3, 3: DagV3)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagTarget {
    pub full_name: String,
    pub migrated_from: u32,
}

impl From<DagV3> for DagTarget {
    fn from(v3: DagV3) -> Self {
        Self {
            full_name: v3.full_name,
            migrated_from: v3.migrated_from,
        }
    }
}

impl From<DagTarget> for DagV3 {
    fn from(t: DagTarget) -> Self {
        Self {
            full_name: t.full_name,
            migrated_from: t.migrated_from,
        }
    }
}

#[test]
fn test_dag_jump_manual_conversion() {
    let v1 = DagV1 {
        legacy_payload: "hello".to_owned(),
    };
    let target1: DagTarget = DagTarget::from(v1.clone());
    let target1_into: DagTarget = v1.into();
    assert_eq!(
        target1,
        DagTarget {
            full_name: "v1_migrated:hello".to_owned(),
            migrated_from: 1,
        }
    );
    assert_eq!(target1_into, target1);

    let v2 = DagV2 {
        intermediate_payload: "world".to_owned(),
    };
    let target2: DagTarget = DagTarget::from(v2.clone());
    let target2_into: DagTarget = v2.into();
    assert_eq!(
        target2,
        DagTarget {
            full_name: "v2_migrated:world".to_owned(),
            migrated_from: 2,
        }
    );
    assert_eq!(target2_into, target2);
}

// =========================================================================
// 5. Redundant version tags pointing to the same type (deduplication)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedLegacy {
    pub info: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedCurrent {
    pub info: String,
}

impl From<SharedLegacy> for SharedCurrent {
    fn from(legacy: SharedLegacy) -> Self {
        Self {
            info: format!("shared:{}", legacy.info),
        }
    }
}

#[backwards_compat(
    tag = "v",
    version = "v2",
    versions(
        "v0": SharedLegacy => "v2",
        "v1": SharedLegacy => "v2",
        "v1_alt": SharedLegacy => "v2",
        "v2": SharedCurrent,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeduplicatedTarget {
    pub info: String,
}

impl From<SharedCurrent> for DeduplicatedTarget {
    fn from(c: SharedCurrent) -> Self {
        Self { info: c.info }
    }
}

impl From<DeduplicatedTarget> for SharedCurrent {
    fn from(t: DeduplicatedTarget) -> Self {
        Self { info: t.info }
    }
}

#[test]
fn test_deduplicated_version_tags_trait_impl() {
    let legacy = SharedLegacy {
        info: "data".to_owned(),
    };
    let target: DeduplicatedTarget = DeduplicatedTarget::from(legacy.clone());
    let target_into: DeduplicatedTarget = legacy.into();

    assert_eq!(
        target,
        DeduplicatedTarget {
            info: "shared:data".to_owned()
        }
    );
    assert_eq!(target_into, target);
}

// =========================================================================
// 6. Target model included in the versions list (no reflexive From collision)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfTargetV1 {
    pub value: u32,
}

#[backwards_compat(tag = "ver", version = 2, versions(1: SelfTargetV1))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTargetModel {
    pub value: u32,
}

impl From<SelfTargetV1> for SelfTargetModel {
    fn from(v1: SelfTargetV1) -> Self {
        Self {
            value: v1.value + 100,
        }
    }
}

#[test]
#[allow(
    clippy::useless_conversion,
    reason = "blanket From<T> for T self-conversion test"
)]
fn test_target_in_versions_list() {
    let v1 = SelfTargetV1 { value: 5 };
    let model_from_v1 = SelfTargetModel::from(v1);
    assert_eq!(model_from_v1, SelfTargetModel { value: 105 });

    // Standard blanket From<T> for T works without compiler errors
    let v2 = SelfTargetModel { value: 200 };
    let model_from_v2 = SelfTargetModel::from(v2);
    assert_eq!(model_from_v2, SelfTargetModel { value: 200 });
}

// =========================================================================
// 7. Serde deserialization output and manual in-code conversion output are identical
// =========================================================================

#[test]
fn test_serde_deserialization_matches_manual_conversion() {
    // 7.1 Infallible chain
    let json_v1 = serde_json::json!({
        "v": "1",
        "name": "sensor_42"
    });
    let serde_target_v1: InfallibleTarget = serde_json::from_value(json_v1).unwrap();
    let manual_v1 = InfallibleV1 {
        name: "sensor_42".to_owned(),
    };
    let manual_target_v1: InfallibleTarget = manual_v1.into();
    assert_eq!(serde_target_v1, manual_target_v1);

    let json_v2 = serde_json::json!({
        "v": "2",
        "name": "sensor_42",
        "count": 77
    });
    let serde_target_v2: InfallibleTarget = serde_json::from_value(json_v2).unwrap();
    let manual_v2 = InfallibleV2 {
        name: "sensor_42".to_owned(),
        count: 77,
    };
    let manual_target_v2: InfallibleTarget = manual_v2.into();
    assert_eq!(serde_target_v2, manual_target_v2);

    // 7.2 Custom Error Fallible chain
    let json_fallible_ok = serde_json::json!({
        "version": "1",
        "value": 40
    });
    let serde_fallible_target: CustomErrorTarget = serde_json::from_value(json_fallible_ok).unwrap();
    let manual_fallible_target = CustomErrorTarget::try_from(CustomErrorV1 { value: 40 }).unwrap();
    assert_eq!(serde_fallible_target, manual_fallible_target);

    // 7.3 DAG jump
    let json_dag_v1 = serde_json::json!({
        "dag_ver": "1",
        "legacy_payload": "test_dag"
    });
    let serde_dag_target: DagTarget = serde_json::from_value(json_dag_v1).unwrap();
    let manual_dag_target = DagTarget::from(DagV1 {
        legacy_payload: "test_dag".to_owned(),
    });
    assert_eq!(serde_dag_target, manual_dag_target);

    // 7.4 Deduplicated versions
    let json_shared_v0 = serde_json::json!({
        "v": "v0",
        "info": "shared_payload"
    });
    let serde_shared_v0: DeduplicatedTarget = serde_json::from_value(json_shared_v0).unwrap();
    let json_shared_v1 = serde_json::json!({
        "v": "v1",
        "info": "shared_payload"
    });
    let serde_shared_v1: DeduplicatedTarget = serde_json::from_value(json_shared_v1).unwrap();
    let manual_shared: DeduplicatedTarget = SharedLegacy {
        info: "shared_payload".to_owned(),
    }
    .into();
    assert_eq!(serde_shared_v0, manual_shared);
    assert_eq!(serde_shared_v1, manual_shared);
}
