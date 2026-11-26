use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// =========================================================================
// 1. DAG jumps (`=> tag` and `=> Self` bypassing explicit current wire)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct DagV1 {
    pub score: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct DagV2 {
    pub score: i32,
    pub label: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagV3Wire {
    pub score: i32,
    pub label: String,
    pub tier: u8,
}

#[backwards_compat(
    versions(
        1: DagV1 => Self,
        2: DagV2 => 3,
        #[current]
        3: DagV3Wire,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagTarget {
    pub score: i32,
    pub label: String,
    pub tier: u8,
}

// Tag 1 jumps directly to Self, bypassing DagV2 and DagV3Wire
impl From<DagV1> for DagTarget {
    fn from(v: DagV1) -> Self {
        Self {
            score: v.score,
            label: "direct".to_owned(),
            tier: 0,
        }
    }
}

impl From<DagV2> for DagV3Wire {
    fn from(v: DagV2) -> Self {
        Self {
            score: v.score,
            label: v.label,
            tier: 1,
        }
    }
}

impl From<DagV3Wire> for DagTarget {
    fn from(w: DagV3Wire) -> Self {
        Self {
            score: w.score,
            label: w.label,
            tier: w.tier,
        }
    }
}

impl From<DagTarget> for DagV3Wire {
    fn from(t: DagTarget) -> Self {
        Self {
            score: t.score,
            label: t.label,
            tier: t.tier,
        }
    }
}

#[test]
fn test_dag_jumps_and_self_shorthand() {
    let from_v1: DagTarget = serde_json::from_str(r#"{"version":1,"score":10}"#).unwrap();
    assert_eq!(
        from_v1,
        DagTarget {
            score: 10,
            label: "direct".to_owned(),
            tier: 0,
        }
    );

    let from_v2: DagTarget = serde_json::from_str(r#"{"version":2,"score":20,"label":"mid"}"#).unwrap();
    assert_eq!(
        from_v2,
        DagTarget {
            score: 20,
            label: "mid".to_owned(),
            tier: 1,
        }
    );
}

// =========================================================================
// 2. Fallible chains (custom and standard error types)
// =========================================================================

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseConfigError {
    #[error("invalid port: {0}")]
    InvalidPort(String),
    #[error("port must be non-zero")]
    ZeroPort,
}

#[derive(Debug, Deserialize)]
pub struct PortV1 {
    pub port_str: String,
}

#[derive(Debug, Deserialize)]
pub struct PortV2 {
    pub port: u16,
}

#[backwards_compat(
    versions(
        #[fallible]
        1: PortV1 => 2,
        #[fallible]
        2: PortV2 => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct PortConfig {
    pub port: u16,
}

impl TryFrom<PortV1> for PortV2 {
    type Error = ParseConfigError;

    fn try_from(v: PortV1) -> Result<Self, Self::Error> {
        let port = v
            .port_str
            .parse::<u16>()
            .map_err(|_| ParseConfigError::InvalidPort(v.port_str))?;
        Ok(Self { port })
    }
}

impl TryFrom<PortV2> for PortConfig {
    type Error = ParseConfigError;

    fn try_from(v: PortV2) -> Result<Self, Self::Error> {
        if v.port == 0 {
            return Err(ParseConfigError::ZeroPort);
        }
        Ok(Self { port: v.port })
    }
}

// Default boxed error
#[derive(Debug, Deserialize)]
pub struct BoxedV1 {
    pub num: String,
}

#[derive(Debug, Deserialize)]
pub struct BoxedV2 {
    pub num: u32,
}

#[backwards_compat(
    versions(
        #[fallible]
        1: BoxedV1 => 2,
        2: BoxedV2 => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct BoxedTarget {
    pub num: u32,
}

impl TryFrom<BoxedV1> for BoxedV2 {
    type Error = core::num::ParseIntError;

    fn try_from(v: BoxedV1) -> Result<Self, Self::Error> {
        Ok(Self { num: v.num.parse()? })
    }
}

impl From<BoxedV2> for BoxedTarget {
    fn from(v: BoxedV2) -> Self {
        Self { num: v.num }
    }
}

#[test]
fn test_fallible_chains_custom_and_default_error() {
    let ok: PortConfig = serde_json::from_str(r#"{"version":1,"port_str":"8080"}"#).unwrap();
    assert_eq!(ok, PortConfig { port: 8080 });

    let de_err_parse = serde_json::from_str::<PortConfig>(r#"{"version":1,"port_str":"abc"}"#).unwrap_err();
    assert!(de_err_parse.to_string().contains("invalid port: abc"));

    let de_err = serde_json::from_str::<PortConfig>(r#"{"version":1,"port_str":"0"}"#).unwrap_err();
    assert!(de_err.to_string().contains("port must be non-zero"));

    // Standard error type in fallible chain
    let b_ok: BoxedTarget = serde_json::from_str(r#"{"version":1,"num":"42"}"#).unwrap();
    assert_eq!(b_ok, BoxedTarget { num: 42 });
    let _ = serde_json::from_str::<BoxedTarget>(r#"{"version":1,"num":"nope"}"#).unwrap_err();
}

// =========================================================================
// 3. `#[with(fn_path)]` — infallible, fallible, 1-step, multi-step,
//    and orphan-rule bypass with foreign std types!
// =========================================================================

mod converters {
    use super::{ForeignTarget, ParseConfigError};

    pub fn str_to_u32(s: String) -> Result<u32, ParseConfigError> {
        s.parse::<u32>().map_err(|_| ParseConfigError::InvalidPort(s))
    }

    pub const fn u32_to_target(n: u32) -> ForeignTarget {
        ForeignTarget { id: n }
    }
}

// Uses foreign `String` and `u32` directly as wire types without violating the orphan rule!
#[backwards_compat(
    content = "data",
    versions(
        #[fallible]
        #[with(converters::str_to_u32)]
        1: String => 2,
        #[with(converters::u32_to_target)]
        2: u32 => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ForeignTarget {
    pub id: u32,
}

#[test]
fn test_with_fn_orphan_rule_bypass_one_step_and_multi_step() {
    // 2-step via String -> (str_to_u32) -> u32 -> (u32_to_target) -> ForeignTarget
    let from_v1: ForeignTarget = serde_json::from_str(r#"{"version":1,"data":"123"}"#).unwrap();
    assert_eq!(from_v1, ForeignTarget { id: 123 });

    let err_v1 = serde_json::from_str::<ForeignTarget>(r#"{"version":1,"data":"bad"}"#).unwrap_err();
    assert!(err_v1.to_string().contains("invalid port: bad"));

    // 1-step via u32 -> (u32_to_target) -> ForeignTarget directly in match arm
    let from_v2: ForeignTarget = serde_json::from_str(r#"{"version":2,"data":789}"#).unwrap();
    assert_eq!(from_v2, ForeignTarget { id: 789 });
}

// =========================================================================
// 4. Duplicate Rust types (sequential, alternating, fallible)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct DupV1 {
    pub x: i32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DupV2 {
    pub x: i32,
}

impl From<DupV1> for DupV2 {
    fn from(v: DupV1) -> Self {
        Self { x: v.x + 1 }
    }
}

#[backwards_compat(
    versions(
        1: DupV1,
        2: DupV1,
        3: DupV2,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequentialDupTarget {
    pub x: i32,
}

impl From<DupV2> for SequentialDupTarget {
    fn from(v: DupV2) -> Self {
        Self { x: v.x }
    }
}

impl From<SequentialDupTarget> for DupV2 {
    fn from(t: SequentialDupTarget) -> Self {
        Self { x: t.x }
    }
}

// Alternating: V1 -> V2 -> V1
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltA {
    pub x: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct AltB {
    pub x: i32,
}

impl From<AltB> for AltA {
    fn from(v: AltB) -> Self {
        Self { x: v.x * 2 }
    }
}

#[backwards_compat(
    versions(
        1: AltA,
        2: AltB,
        3: AltA,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternatingDupTarget {
    pub x: i32,
}

impl From<AltA> for AlternatingDupTarget {
    fn from(v: AltA) -> Self {
        Self { x: v.x }
    }
}

impl From<AlternatingDupTarget> for AltA {
    fn from(t: AlternatingDupTarget) -> Self {
        Self { x: t.x }
    }
}

#[test]
fn test_duplicate_types_sequential_and_alternating() {
    let s1: SequentialDupTarget = serde_json::from_str(r#"{"version":1,"x":5}"#).unwrap();
    let s2: SequentialDupTarget = serde_json::from_str(r#"{"version":2,"x":5}"#).unwrap();
    let s3: SequentialDupTarget = serde_json::from_str(r#"{"version":3,"x":6}"#).unwrap();
    assert_eq!(s1, SequentialDupTarget { x: 6 });
    assert_eq!(s2, SequentialDupTarget { x: 6 });
    assert_eq!(s3, SequentialDupTarget { x: 6 });

    let a1: AlternatingDupTarget = serde_json::from_str(r#"{"version":1,"x":5}"#).unwrap();
    let a2: AlternatingDupTarget = serde_json::from_str(r#"{"version":2,"x":5}"#).unwrap();
    let a3: AlternatingDupTarget = serde_json::from_str(r#"{"version":3,"x":10}"#).unwrap();
    assert_eq!(a1, AlternatingDupTarget { x: 5 });
    assert_eq!(a2, AlternatingDupTarget { x: 10 });
    assert_eq!(a3, AlternatingDupTarget { x: 10 });
}

// =========================================================================
// 5. Display-only, non-Error, non-Send/Sync heterogeneous errors across steps
// =========================================================================

pub struct DisplayOnlyErrA {
    msg: String,
    _not_send_sync: core::marker::PhantomData<*const ()>,
}

impl core::fmt::Display for DisplayOnlyErrA {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "step-a display error: {}", self.msg)
    }
}

pub struct DisplayOnlyErrB {
    code: u32,
    _not_send_sync: core::marker::PhantomData<*const ()>,
}

impl core::fmt::Display for DisplayOnlyErrB {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "step-b display error: code={}", self.code)
    }
}

#[derive(Debug, Deserialize)]
pub struct HeteroV1 {
    pub raw: String,
}

#[derive(Debug, Deserialize)]
pub struct HeteroV2 {
    pub val: u32,
}

#[backwards_compat(
    versions(
        #[fallible]
        1: HeteroV1 => 2,
        #[fallible]
        2: HeteroV2 => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct HeteroTarget {
    pub val: u32,
}

impl TryFrom<HeteroV1> for HeteroV2 {
    type Error = DisplayOnlyErrA;

    fn try_from(v: HeteroV1) -> Result<Self, Self::Error> {
        let val = v.raw.parse::<u32>().map_err(|_| DisplayOnlyErrA {
            msg: v.raw,
            _not_send_sync: core::marker::PhantomData,
        })?;
        Ok(Self { val })
    }
}

impl TryFrom<HeteroV2> for HeteroTarget {
    type Error = DisplayOnlyErrB;

    fn try_from(v: HeteroV2) -> Result<Self, Self::Error> {
        if v.val > 100 {
            return Err(DisplayOnlyErrB {
                code: v.val,
                _not_send_sync: core::marker::PhantomData,
            });
        }
        Ok(Self { val: v.val })
    }
}

#[test]
fn test_multistep_fallible_heterogeneous_display_only_errors() {
    // Happy path through both fallible steps
    let ok: HeteroTarget = serde_json::from_str(r#"{"version":1,"raw":"42"}"#).unwrap();
    assert_eq!(ok, HeteroTarget { val: 42 });

    // Failure in step 1 (DisplayOnlyErrA)
    let err_a = serde_json::from_str::<HeteroTarget>(r#"{"version":1,"raw":"not-a-num"}"#).unwrap_err();
    assert!(err_a.to_string().contains("step-a display error: not-a-num"));

    // Passes step 1, fails in step 2 (DisplayOnlyErrB)
    let err_b_from_v1 = serde_json::from_str::<HeteroTarget>(r#"{"version":1,"raw":"250"}"#).unwrap_err();
    assert!(
        err_b_from_v1
            .to_string()
            .contains("step-b display error: code=250")
    );

    // Direct v2 failure in step 2 (DisplayOnlyErrB)
    let err_b_from_v2 = serde_json::from_str::<HeteroTarget>(r#"{"version":2,"val":999}"#).unwrap_err();
    assert!(
        err_b_from_v2
            .to_string()
            .contains("step-b display error: code=999")
    );
}

// =========================================================================
// 6. Same wire type across two tags with distinct inlined migration paths
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct SharedWireV1 {
    pub value: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct SharedWireV2 {
    pub value: i32,
}

impl From<SharedWireV1> for SharedWireV2 {
    fn from(v: SharedWireV1) -> Self {
        Self { value: v.value + 1 }
    }
}

impl From<SharedWireV2> for DivergentWireTarget {
    fn from(v: SharedWireV2) -> Self {
        Self {
            value: v.value,
            route: "via_v2".to_owned(),
        }
    }
}

fn special_v1_to_target(v: SharedWireV1) -> DivergentWireTarget {
    DivergentWireTarget {
        value: v.value * 10,
        route: "special".to_owned(),
    }
}

#[backwards_compat(
    versions(
        1: SharedWireV1 => 2,
        2: SharedWireV2 => 4,
        #[with(special_v1_to_target)]
        3: SharedWireV1 => Self,
        4: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct DivergentWireTarget {
    pub value: i32,
    pub route: String,
}

#[test]
fn test_shared_wire_type_divergent_inlined_paths() {
    // Version 1 follows 1 (SharedWireV1) -> 2 (SharedWireV2) -> 4 (Self)
    let from_v1: DivergentWireTarget = serde_json::from_str(r#"{"version":1,"value":7}"#).unwrap();
    assert_eq!(
        from_v1,
        DivergentWireTarget {
            value: 8,
            route: "via_v2".to_owned(),
        }
    );

    // Version 2 follows 2 (SharedWireV2) -> 4 (Self)
    let from_v2: DivergentWireTarget = serde_json::from_str(r#"{"version":2,"value":7}"#).unwrap();
    assert_eq!(
        from_v2,
        DivergentWireTarget {
            value: 7,
            route: "via_v2".to_owned(),
        }
    );

    // Version 3 shares wire type SharedWireV1 but runs its own #[with(special_v1_to_target)] => Self arm
    let from_v3: DivergentWireTarget = serde_json::from_str(r#"{"version":3,"value":7}"#).unwrap();
    assert_eq!(
        from_v3,
        DivergentWireTarget {
            value: 70,
            route: "special".to_owned(),
        }
    );

    // Version 4 deserializes Self directly
    let from_v4: DivergentWireTarget =
        serde_json::from_str(r#"{"version":4,"value":7,"route":"direct"}"#).unwrap();
    assert_eq!(
        from_v4,
        DivergentWireTarget {
            value: 7,
            route: "direct".to_owned(),
        }
    );
}

// =========================================================================
// 5. Explicit `=> <Type>` chains, jumps, and domain target (`=> TargetStruct`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TypeChainV0 {
    pub raw: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TypeChainV1 {
    pub step: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TypeChainV2 {
    pub step: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TypeChainV3 {
    pub step: i32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeChainV4 {
    pub step: i32,
    pub path: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct TypeChainDirect {
    pub direct: i32,
}

#[backwards_compat(
    versions(
        0: TypeChainV0 => TypeChainV3,
        1: TypeChainV1 => 2,
        2: TypeChainV2 => TypeChainV3,
        3: TypeChainV3 => TypeChainV4,
        #[current]
        4: TypeChainV4,
        5: TypeChainDirect => TypeChainTarget,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeChainTarget {
    pub step: i32,
    pub path: String,
}

impl From<TypeChainV0> for TypeChainV3 {
    fn from(v: TypeChainV0) -> Self {
        Self { step: v.raw + 30 }
    }
}

impl From<TypeChainV1> for TypeChainV2 {
    fn from(v: TypeChainV1) -> Self {
        Self { step: v.step + 1 }
    }
}

impl From<TypeChainV2> for TypeChainV3 {
    fn from(v: TypeChainV2) -> Self {
        Self { step: v.step + 10 }
    }
}

impl From<TypeChainV3> for TypeChainV4 {
    fn from(v: TypeChainV3) -> Self {
        Self {
            step: v.step + 100,
            path: "v4".to_owned(),
        }
    }
}

impl From<TypeChainV4> for TypeChainTarget {
    fn from(v: TypeChainV4) -> Self {
        Self {
            step: v.step,
            path: v.path,
        }
    }
}

impl From<TypeChainTarget> for TypeChainV4 {
    fn from(t: TypeChainTarget) -> Self {
        Self {
            step: t.step,
            path: t.path,
        }
    }
}

impl From<TypeChainDirect> for TypeChainTarget {
    fn from(v: TypeChainDirect) -> Self {
        Self {
            step: v.direct,
            path: "direct".to_owned(),
        }
    }
}

#[test]
fn test_explicit_type_targets_chains_jumps_and_domain_target() {
    // Jump: V0 => V3 => V4 => Target (+30 +100)
    let from_v0: TypeChainTarget = serde_json::from_str(r#"{"version":0,"raw":5}"#).unwrap();
    assert_eq!(
        from_v0,
        TypeChainTarget {
            step: 135,
            path: "v4".to_owned(),
        }
    );

    // Mixed chain: V1 => 2 (V2) => V3 => V4 => Target (+1 +10 +100)
    let from_v1: TypeChainTarget = serde_json::from_str(r#"{"version":1,"step":2}"#).unwrap();
    assert_eq!(
        from_v1,
        TypeChainTarget {
            step: 113,
            path: "v4".to_owned(),
        }
    );
    let from_v2: TypeChainTarget = serde_json::from_str(r#"{"version":2,"step":3}"#).unwrap();
    assert_eq!(from_v2, from_v1);

    // Direct domain target bypassing current wire TypeChainV4
    let from_direct: TypeChainTarget = serde_json::from_str(r#"{"version":5,"direct":77}"#).unwrap();
    assert_eq!(
        from_direct,
        TypeChainTarget {
            step: 77,
            path: "direct".to_owned(),
        }
    );
    assert_eq!(TypeChainTarget::from(TypeChainDirect { direct: 77 }), from_direct);

    // Serialization uses #[current] version 4
    let serialized = serde_json::to_string(&from_v0).unwrap();
    assert_eq!(serialized, r#"{"version":4,"step":135,"path":"v4"}"#);
}

// =========================================================================
// 6. Bidirectional duplicate-type `=>` inheritance (forward & reverse)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct InheritV1 {
    pub v1_num: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct InheritV2 {
    pub v2_name: String,
}

#[backwards_compat(
    versions(
        _: InheritV1,
        _: InheritV2,
        1: InheritV1 => Self,
        2: InheritV2 => Self,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardInheritTarget {
    pub summary: String,
}

impl From<InheritV1> for ForwardInheritTarget {
    fn from(v: InheritV1) -> Self {
        Self {
            summary: format!("v1:{}", v.v1_num),
        }
    }
}

impl From<InheritV2> for ForwardInheritTarget {
    fn from(v: InheritV2) -> Self {
        Self {
            summary: format!("v2:{}", v.v2_name),
        }
    }
}

#[backwards_compat(
    versions(
        1: InheritV1 => Self,
        2: InheritV2 => Self,
        _: InheritV1,
        _: InheritV2,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseInheritTarget {
    pub summary: String,
}

impl From<InheritV1> for ReverseInheritTarget {
    fn from(v: InheritV1) -> Self {
        Self {
            summary: format!("v1:{}", v.v1_num),
        }
    }
}

impl From<InheritV2> for ReverseInheritTarget {
    fn from(v: InheritV2) -> Self {
        Self {
            summary: format!("v2:{}", v.v2_name),
        }
    }
}

#[test]
fn test_duplicate_type_inheritance_forward_and_reverse() {
    // Forward order
    let fwd_untagged_v1: ForwardInheritTarget = serde_json::from_str(r#"{"v1_num":10}"#).unwrap();
    let fwd_untagged_v2: ForwardInheritTarget = serde_json::from_str(r#"{"v2_name":"alpha"}"#).unwrap();
    let fwd_tagged_v1: ForwardInheritTarget = serde_json::from_str(r#"{"version":1,"v1_num":10}"#).unwrap();
    let fwd_tagged_v2: ForwardInheritTarget =
        serde_json::from_str(r#"{"version":2,"v2_name":"alpha"}"#).unwrap();
    let fwd_current: ForwardInheritTarget =
        serde_json::from_str(r#"{"version":3,"summary":"current"}"#).unwrap();

    assert_eq!(
        fwd_untagged_v1,
        ForwardInheritTarget {
            summary: "v1:10".to_owned()
        }
    );
    assert_eq!(fwd_tagged_v1, fwd_untagged_v1);
    assert_eq!(
        fwd_untagged_v2,
        ForwardInheritTarget {
            summary: "v2:alpha".to_owned()
        }
    );
    assert_eq!(fwd_tagged_v2, fwd_untagged_v2);
    assert_eq!(
        fwd_current,
        ForwardInheritTarget {
            summary: "current".to_owned()
        }
    );
    assert_eq!(
        serde_json::to_string(&fwd_current).unwrap(),
        r#"{"version":3,"summary":"current"}"#
    );

    // Reverse order
    let rev_untagged_v1: ReverseInheritTarget = serde_json::from_str(r#"{"v1_num":20}"#).unwrap();
    let rev_untagged_v2: ReverseInheritTarget = serde_json::from_str(r#"{"v2_name":"beta"}"#).unwrap();
    let rev_tagged_v1: ReverseInheritTarget = serde_json::from_str(r#"{"version":1,"v1_num":20}"#).unwrap();
    let rev_tagged_v2: ReverseInheritTarget =
        serde_json::from_str(r#"{"version":2,"v2_name":"beta"}"#).unwrap();
    let rev_current: ReverseInheritTarget =
        serde_json::from_str(r#"{"version":3,"summary":"current"}"#).unwrap();

    assert_eq!(
        rev_untagged_v1,
        ReverseInheritTarget {
            summary: "v1:20".to_owned()
        }
    );
    assert_eq!(rev_tagged_v1, rev_untagged_v1);
    assert_eq!(
        rev_untagged_v2,
        ReverseInheritTarget {
            summary: "v2:beta".to_owned()
        }
    );
    assert_eq!(rev_tagged_v2, rev_untagged_v2);
    assert_eq!(
        rev_current,
        ReverseInheritTarget {
            summary: "current".to_owned()
        }
    );
    assert_eq!(
        serde_json::to_string(&rev_current).unwrap(),
        r#"{"version":3,"summary":"current"}"#
    );
}

// =========================================================================
// 7. Targeting a type shared across multiple rows and inline pipe aliases
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct SharedV1 {
    pub val: i32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct SharedV2 {
    pub val: i32,
}

impl From<SharedV1> for SharedV2 {
    fn from(v: SharedV1) -> Self {
        Self { val: v.val * 2 }
    }
}

#[backwards_compat(
    versions(
        1: SharedV1 => SharedV2,
        2: SharedV2,
        3: SharedV2,
        4: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRowTypeTarget {
    pub val: i32,
}

impl From<SharedV2> for MultiRowTypeTarget {
    fn from(v: SharedV2) -> Self {
        Self { val: v.val + 1 }
    }
}

#[backwards_compat(
    versions(
        1: SharedV1 => SharedV2,
        _ | 2 | 20: SharedV2,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeAliasTypeTarget {
    pub val: i32,
}

impl From<SharedV2> for PipeAliasTypeTarget {
    fn from(v: SharedV2) -> Self {
        Self { val: v.val + 10 }
    }
}

#[test]
fn test_explicit_type_target_shared_rows_and_pipe_aliases() {
    let m1: MultiRowTypeTarget = serde_json::from_str(r#"{"version":1,"val":4}"#).unwrap();
    let m2: MultiRowTypeTarget = serde_json::from_str(r#"{"version":2,"val":8}"#).unwrap();
    let m3: MultiRowTypeTarget = serde_json::from_str(r#"{"version":3,"val":8}"#).unwrap();
    assert_eq!(m1, MultiRowTypeTarget { val: 9 });
    assert_eq!(m2, MultiRowTypeTarget { val: 9 });
    assert_eq!(m3, MultiRowTypeTarget { val: 9 });
    assert_eq!(serde_json::to_string(&m1).unwrap(), r#"{"version":4,"val":9}"#);

    let p1: PipeAliasTypeTarget = serde_json::from_str(r#"{"version":1,"val":4}"#).unwrap();
    let p_untagged: PipeAliasTypeTarget = serde_json::from_str(r#"{"val":8}"#).unwrap();
    let p_int: PipeAliasTypeTarget = serde_json::from_str(r#"{"version":2,"val":8}"#).unwrap();
    let p_str: PipeAliasTypeTarget = serde_json::from_str(r#"{"version":20,"val":8}"#).unwrap();
    assert_eq!(p1, PipeAliasTypeTarget { val: 18 });
    assert_eq!(p_untagged, p1);
    assert_eq!(p_int, p1);
    assert_eq!(p_str, p1);
    assert_eq!(serde_json::to_string(&p1).unwrap(), r#"{"version":3,"val":18}"#);
}

// =========================================================================
// 8. `=> <Type>` with `#[fallible]`, `#[with(...)]`, and generics/lifetimes
// =========================================================================

mod generic_converters {
    use super::{GenV1, GenV2, ParseConfigError};

    pub fn upgrade_v1<T>(v: GenV1<'_, T>) -> Result<GenV2<'_, T>, ParseConfigError> {
        let port = v
            .port_str
            .parse::<u16>()
            .map_err(|_| ParseConfigError::InvalidPort(v.port_str.to_owned()))?;
        Ok(GenV2 {
            port,
            tag: v.tag,
            payload: v.payload,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenV1<'a, T> {
    pub port_str: &'a str,
    pub tag: &'a str,
    pub payload: T,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenV2<'a, T> {
    pub port: u16,
    pub tag: &'a str,
    pub payload: T,
}

#[backwards_compat(
    versions(
        #[fallible]
        #[with(generic_converters::upgrade_v1)]
        1: GenV1<'a, T> => GenV2<'a, T>,
        2: GenV2<'a, T> => GenTarget<'a, T>,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenTarget<'a, T> {
    pub port: u16,
    pub tag: &'a str,
    pub payload: T,
}

impl<'a, T> From<GenV2<'a, T>> for GenTarget<'a, T> {
    fn from(v: GenV2<'a, T>) -> Self {
        Self {
            port: v.port,
            tag: v.tag,
            payload: v.payload,
        }
    }
}

#[test]
fn test_explicit_type_with_fallible_with_fn_and_generics() {
    let json_v1 = r#"{"version":1,"port_str":"9000","tag":"edge","payload":42}"#;
    let parsed: GenTarget<'_, i32> = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        parsed,
        GenTarget {
            port: 9000,
            tag: "edge",
            payload: 42,
        }
    );

    let de_err = serde_json::from_str::<GenTarget<'_, i32>>(
        r#"{"version":1,"port_str":"bad","tag":"edge","payload":1}"#,
    )
    .unwrap_err();
    assert!(de_err.to_string().contains("invalid port: bad"));

    // Serialization uses version 3
    let serialized = serde_json::to_string(&parsed).unwrap();
    assert_eq!(
        serialized,
        r#"{"version":3,"port":9000,"tag":"edge","payload":42}"#
    );
}
