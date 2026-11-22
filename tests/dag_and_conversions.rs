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
    // Transitive From<DagV2> for DagTarget generated
    assert_eq!(
        DagTarget::from(DagV2 {
            score: 20,
            label: "mid".to_owned(),
        }),
        from_v2
    );
}

// =========================================================================
// 2. Fallible chains (default boxed error & custom error = ...)
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
    error = ParseConfigError,
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

    // Transitive TryFrom<PortV1> returns ParseConfigError
    let err1 = PortConfig::try_from(PortV1 {
        port_str: "abc".to_owned(),
    })
    .unwrap_err();
    assert_eq!(err1, ParseConfigError::InvalidPort("abc".to_owned()));

    let err2 = PortConfig::try_from(PortV1 {
        port_str: "0".to_owned(),
    })
    .unwrap_err();
    assert_eq!(err2, ParseConfigError::ZeroPort);

    let de_err = serde_json::from_str::<PortConfig>(r#"{"version":1,"port_str":"0"}"#).unwrap_err();
    assert!(de_err.to_string().contains("port must be non-zero"));

    // Default boxed error
    let b_ok: BoxedTarget = serde_json::from_str(r#"{"version":1,"num":"42"}"#).unwrap();
    assert_eq!(b_ok, BoxedTarget { num: 42 });
    let _ = BoxedTarget::try_from(BoxedV1 {
        num: "nope".to_owned(),
    })
    .unwrap_err();
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
    error = ParseConfigError,
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

    // Transitive TryFrom<String> for ForeignTarget works!
    assert_eq!(
        ForeignTarget::try_from("456".to_owned()).unwrap(),
        ForeignTarget { id: 456 }
    );
    assert_eq!(
        ForeignTarget::try_from("bad".to_owned()).unwrap_err(),
        ParseConfigError::InvalidPort("bad".to_owned())
    );

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
    assert_eq!(SequentialDupTarget::from(DupV1 { x: 5 }), s1);

    let a1: AlternatingDupTarget = serde_json::from_str(r#"{"version":1,"x":5}"#).unwrap();
    let a2: AlternatingDupTarget = serde_json::from_str(r#"{"version":2,"x":5}"#).unwrap();
    let a3: AlternatingDupTarget = serde_json::from_str(r#"{"version":3,"x":10}"#).unwrap();
    assert_eq!(a1, AlternatingDupTarget { x: 5 });
    assert_eq!(a2, AlternatingDupTarget { x: 10 });
    assert_eq!(a3, AlternatingDupTarget { x: 10 });
    assert_eq!(AlternatingDupTarget::from(AltB { x: 5 }), a2);
}
