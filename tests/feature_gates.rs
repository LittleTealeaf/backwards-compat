use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// =========================================================================
// 1. Mixed active/inactive DAG with fallible terminal version & custom error
//    (0, 1, and 2+ disabled gates all compile with zero warnings)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("tournament error: {0}")]
pub struct TournamentError(pub String);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TestTournament {
    pub legacy_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct V5Tournament {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct V6Tournament {
    pub title: String,
    pub rounds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedTournament {
    pub title: String,
    pub rounds: u32,
    pub ranked: bool,
}

#[backwards_compat(
    tag = "version",
    error = TournamentError,
    versions = [
        #[cfg(test)]
        2: TestTournament => 7,
        #[cfg(any())]
        3: NonExistentV3 => 5,
        #[cfg(any())]
        4: NonExistentV4 => 5,
        5: V5Tournament => 6,
        6: V6Tournament => 7,
        #[current]
        #[fallible]
        7: SerializedTournament,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tournament {
    pub title: String,
    pub rounds: u32,
    pub ranked: bool,
}

#[cfg(test)]
impl From<TestTournament> for SerializedTournament {
    fn from(v: TestTournament) -> Self {
        Self {
            title: v.legacy_title,
            rounds: 1,
            ranked: false,
        }
    }
}

impl From<V5Tournament> for V6Tournament {
    fn from(v: V5Tournament) -> Self {
        Self {
            title: v.title,
            rounds: 3,
        }
    }
}

impl From<V6Tournament> for SerializedTournament {
    fn from(v: V6Tournament) -> Self {
        Self {
            title: v.title,
            rounds: v.rounds,
            ranked: true,
        }
    }
}

impl TryFrom<SerializedTournament> for Tournament {
    type Error = TournamentError;

    fn try_from(v: SerializedTournament) -> Result<Self, Self::Error> {
        if v.title.is_empty() {
            return Err(TournamentError("title cannot be empty".to_owned()));
        }
        Ok(Self {
            title: v.title,
            rounds: v.rounds,
            ranked: v.ranked,
        })
    }
}

impl From<Tournament> for SerializedTournament {
    fn from(t: Tournament) -> Self {
        Self {
            title: t.title,
            rounds: t.rounds,
            ranked: t.ranked,
        }
    }
}

#[test]
fn test_mixed_active_inactive_dag_with_fallible_terminal_and_custom_error() {
    // Active gated v2 deserializes and jumps directly to v7
    let v2: Tournament = serde_json::from_str(r#"{"version":2,"legacy_title":"Test Cup"}"#).unwrap();
    assert_eq!(
        v2,
        Tournament {
            title: "Test Cup".to_owned(),
            rounds: 1,
            ranked: false,
        }
    );

    // Transitive TryFrom<TestTournament> works and propagates custom error
    let direct_v2 = Tournament::try_from(TestTournament {
        legacy_title: "Direct Cup".to_owned(),
    })
    .unwrap();
    assert_eq!(direct_v2.title, "Direct Cup");

    let err_v2 = Tournament::try_from(TestTournament {
        legacy_title: String::new(),
    })
    .unwrap_err();
    assert_eq!(err_v2, TournamentError("title cannot be empty".to_owned()));

    // Unconditional v5, v6, v7 deserialize and convert
    let v5: Tournament = serde_json::from_str(r#"{"version":5,"title":"Spring"}"#).unwrap();
    assert_eq!(
        v5,
        Tournament {
            title: "Spring".to_owned(),
            rounds: 3,
            ranked: true,
        }
    );
    assert_eq!(
        Tournament::try_from(V5Tournament {
            title: "Spring".to_owned(),
        })
        .unwrap(),
        v5
    );

    let v6: Tournament = serde_json::from_str(r#"{"version":6,"title":"Summer","rounds":5}"#).unwrap();
    assert_eq!(
        v6,
        Tournament {
            title: "Summer".to_owned(),
            rounds: 5,
            ranked: true,
        }
    );

    let v7: Tournament =
        serde_json::from_str(r#"{"version":7,"title":"Finals","rounds":7,"ranked":false}"#).unwrap();
    assert_eq!(
        v7,
        Tournament {
            title: "Finals".to_owned(),
            rounds: 7,
            ranked: false,
        }
    );

    // Validation error on deserialization
    let invalid = serde_json::from_str::<Tournament>(r#"{"version":7,"title":"","rounds":7,"ranked":false}"#)
        .unwrap_err();
    assert!(invalid.to_string().contains("title cannot be empty"));

    // Serialization emits native integer version 7
    let json = serde_json::to_string(&v7).unwrap();
    assert!(json.contains(r#""version":7"#));

    // Inactive gated v3 and v4 fail deserialization with feature-flag hint
    let err3 = serde_json::from_str::<Tournament>(r#"{"version":3,"title":"Old"}"#).unwrap_err();
    assert!(err3.to_string().contains("feature flags"));
    let err4 = serde_json::from_str::<Tournament>(r#"{"version":4,"title":"Old"}"#).unwrap_err();
    assert!(err4.to_string().contains("feature flags"));
}

// =========================================================================
// 2. Chained gated versions (both active, and active -> inactive)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChainV1 {
    pub raw: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChainV2 {
    pub raw: u32,
    pub bonus: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainV3 {
    pub total: u32,
}

#[backwards_compat(
    versions = [
        #[cfg(test)]
        1: ChainV1 => 2,
        #[cfg(test)]
        2: ChainV2 => 3,
        3: ChainV3,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveChainTarget {
    pub total: u32,
}

impl From<ChainV1> for ChainV2 {
    fn from(v: ChainV1) -> Self {
        Self {
            raw: v.raw,
            bonus: 10,
        }
    }
}

impl From<ChainV2> for ChainV3 {
    fn from(v: ChainV2) -> Self {
        Self {
            total: v.raw + v.bonus,
        }
    }
}

impl From<ChainV3> for ActiveChainTarget {
    fn from(v: ChainV3) -> Self {
        Self { total: v.total }
    }
}

impl From<ActiveChainTarget> for ChainV3 {
    fn from(t: ActiveChainTarget) -> Self {
        Self { total: t.total }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OrphanV1 {
    pub raw: u32,
}

#[backwards_compat(
    versions = [
        #[cfg(test)]
        1: OrphanV1 => 2,
        #[cfg(any())]
        2: MissingV2 => 3,
        3: ChainV3,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialChainTarget {
    pub total: u32,
}

impl From<ChainV3> for PartialChainTarget {
    fn from(v: ChainV3) -> Self {
        Self { total: v.total }
    }
}

impl From<PartialChainTarget> for ChainV3 {
    fn from(t: PartialChainTarget) -> Self {
        Self { total: t.total }
    }
}

#[test]
fn test_chained_gated_versions() {
    let from_v1: ActiveChainTarget = serde_json::from_str(r#"{"version":1,"raw":5}"#).unwrap();
    assert_eq!(from_v1, ActiveChainTarget { total: 15 });
    assert_eq!(ActiveChainTarget::from(ChainV1 { raw: 5 }), from_v1);

    let from_v2: ActiveChainTarget = serde_json::from_str(r#"{"version":2,"raw":5,"bonus":20}"#).unwrap();
    assert_eq!(from_v2, ActiveChainTarget { total: 25 });

    // Active -> inactive: v3 succeeds, v1 and v2 fail
    let v3: PartialChainTarget = serde_json::from_str(r#"{"version":3,"total":42}"#).unwrap();
    assert_eq!(v3, PartialChainTarget { total: 42 });
    let _ = serde_json::from_str::<PartialChainTarget>(r#"{"version":1,"raw":5}"#).unwrap_err();
    let _ = serde_json::from_str::<PartialChainTarget>(r#"{"version":2,"raw":5}"#).unwrap_err();
}

// =========================================================================
// 3. Multiple stacked #[cfg(...)] attributes on a single entry
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MultiActiveV1 {
    pub n: i32,
}

#[backwards_compat(
    versions = [
        #[cfg(test)]
        #[cfg(all())]
        1: MultiActiveV1 => 3,
        #[cfg(test)]
        #[cfg(any())]
        2: MissingMultiV2 => 3,
        3: Self,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiCfgTarget {
    pub n: i32,
}

impl From<MultiActiveV1> for MultiCfgTarget {
    fn from(v: MultiActiveV1) -> Self {
        Self { n: v.n * 2 }
    }
}

#[test]
fn test_multiple_cfg_attributes_on_single_entry() {
    let v1: MultiCfgTarget = serde_json::from_str(r#"{"version":1,"n":7}"#).unwrap();
    assert_eq!(v1, MultiCfgTarget { n: 14 });
    assert_eq!(MultiCfgTarget::from(MultiActiveV1 { n: 7 }), v1);

    let v3: MultiCfgTarget = serde_json::from_str(r#"{"version":3,"n":9}"#).unwrap();
    assert_eq!(v3, MultiCfgTarget { n: 9 });

    let _ = serde_json::from_str::<MultiCfgTarget>(r#"{"version":2,"n":7}"#).unwrap_err();
}

// =========================================================================
// 4. Duplicate Rust types with different #[cfg(...)] (including regression
//    for unconditional + later #[cfg(any())] #[fallible] duplicate)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SharedV1 {
    pub val: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DupWireV2 {
    pub val: i32,
}

impl From<SharedV1> for DupWireV2 {
    fn from(v: SharedV1) -> Self {
        Self { val: v.val + 1 }
    }
}

#[backwards_compat(
    versions = [
        #[cfg(any())]
        1: SharedV1 => 3,
        #[cfg(test)]
        2: SharedV1 => 3,
        3: DupWireV2,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupGatedTarget {
    pub val: i32,
}

impl From<DupWireV2> for DupGatedTarget {
    fn from(v: DupWireV2) -> Self {
        Self { val: v.val }
    }
}

impl From<DupGatedTarget> for DupWireV2 {
    fn from(t: DupGatedTarget) -> Self {
        Self { val: t.val }
    }
}

#[backwards_compat(
    versions = [
        1: SharedV1 => 3,
        #[cfg(any())]
        #[fallible]
        2: SharedV1 => 3,
        3: DupWireV2,
    ]
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupFallibleRegressionTarget {
    pub val: i32,
}

impl From<DupWireV2> for DupFallibleRegressionTarget {
    fn from(v: DupWireV2) -> Self {
        Self { val: v.val }
    }
}

impl From<DupFallibleRegressionTarget> for DupWireV2 {
    fn from(t: DupFallibleRegressionTarget) -> Self {
        Self { val: t.val }
    }
}

#[test]
fn test_duplicate_types_with_different_cfgs() {
    let _ = serde_json::from_str::<DupGatedTarget>(r#"{"version":1,"val":10}"#).unwrap_err();
    let g2: DupGatedTarget = serde_json::from_str(r#"{"version":2,"val":10}"#).unwrap();
    assert_eq!(g2, DupGatedTarget { val: 11 });
    assert_eq!(DupGatedTarget::from(SharedV1 { val: 10 }), g2);

    let r1: DupFallibleRegressionTarget = serde_json::from_str(r#"{"version":1,"val":30}"#).unwrap();
    assert_eq!(r1, DupFallibleRegressionTarget { val: 31 });
    assert_eq!(DupFallibleRegressionTarget::from(SharedV1 { val: 30 }), r1);
    let _ = serde_json::from_str::<DupFallibleRegressionTarget>(r#"{"version":2,"val":30}"#).unwrap_err();
}

// =========================================================================
// 5. Gated version with generics and lifetimes
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenericGatedV1<'a, T> {
    pub name: &'a str,
    pub value: T,
}

#[backwards_compat(
    versions(
        #[cfg(test)]
        1: GenericGatedV1<'a, T> => 3,
        #[cfg(any())]
        2: MissingGenericV2<'a, T> => 3,
        3: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericGated<'a, T: Clone + core::fmt::Debug> {
    pub name: &'a str,
    pub value: T,
    pub enabled: bool,
}

impl<'a, T: Clone + core::fmt::Debug> From<GenericGatedV1<'a, T>> for GenericGated<'a, T> {
    fn from(v: GenericGatedV1<'a, T>) -> Self {
        Self {
            name: v.name,
            value: v.value,
            enabled: true,
        }
    }
}

#[test]
fn test_gated_version_with_generics_and_lifetimes() {
    let v1: GenericGated<'_, i32> =
        serde_json::from_str(r#"{"version":1,"name":"alpha","value":42}"#).unwrap();
    assert_eq!(
        v1,
        GenericGated {
            name: "alpha",
            value: 42,
            enabled: true,
        }
    );

    let _ = serde_json::from_str::<GenericGated<'_, i32>>(r#"{"version":2,"name":"beta","value":99}"#)
        .unwrap_err();

    let v3: GenericGated<'_, i32> =
        serde_json::from_str(r#"{"version":3,"name":"gamma","value":7,"enabled":false}"#).unwrap();
    assert_eq!(
        v3,
        GenericGated {
            name: "gamma",
            value: 7,
            enabled: false,
        }
    );
    let json = serde_json::to_string(&v3).unwrap();
    assert!(json.contains(r#""version":3"#));
}
