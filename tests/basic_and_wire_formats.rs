use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Sequential chain with implicit Self shadow wire + multi-format roundtrip
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct SeqV1 {
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct SeqV2 {
    pub first_name: String,
    pub last_name: String,
}

impl From<SeqV1> for SeqV2 {
    fn from(v: SeqV1) -> Self {
        Self {
            first_name: v.name,
            last_name: String::new(),
        }
    }
}

#[backwards_compat(
    versions(
        1: SeqV1,
        2: SeqV2,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
    pub first_name: String,
    pub last_name: String,
    pub active: bool,
}

impl From<SeqV2> for UserProfile {
    fn from(v: SeqV2) -> Self {
        Self {
            first_name: v.first_name,
            last_name: v.last_name,
            active: true,
        }
    }
}

#[test]
fn test_sequential_chain_and_formats() {
    // JSON v1 -> v2 -> v3
    let from_v1: UserProfile = serde_json::from_str(r#"{"version":1,"name":"Alice"}"#).unwrap();
    assert_eq!(
        from_v1,
        UserProfile {
            first_name: "Alice".to_owned(),
            last_name: String::new(),
            active: true,
        }
    );

    // JSON v2 -> v3
    let from_v2: UserProfile =
        serde_json::from_str(r#"{"version":2,"first_name":"Bob","last_name":"Smith"}"#).unwrap();
    assert_eq!(
        from_v2,
        UserProfile {
            first_name: "Bob".to_owned(),
            last_name: "Smith".to_owned(),
            active: true,
        }
    );

    // JSON roundtrip at v3
    let json = serde_json::to_string(&from_v2).unwrap();
    assert!(json.contains(r#""version":3"#));
    let roundtrip_json: UserProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip_json, from_v2);

    // TOML roundtrip
    let toml_str = toml::to_string(&from_v2).unwrap();
    assert!(toml_str.contains("version = 3"));
    let roundtrip_toml: UserProfile = toml::from_str(&toml_str).unwrap();
    assert_eq!(roundtrip_toml, from_v2);

    // RON roundtrip
    let ron_str = ron::to_string(&from_v2).unwrap();
    let roundtrip_ron: UserProfile = ron::from_str(&ron_str).unwrap();
    assert_eq!(roundtrip_ron, from_v2);
}

// =========================================================================
// 2. Single-version and empty `versions()` ergonomics
// =========================================================================

#[backwards_compat]
#[derive(Debug, PartialEq, Eq)]
pub struct DefaultVersioned {
    pub value: i32,
}

#[backwards_compat(versions(1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct ExplicitSingleVersion {
    pub label: String,
}

#[test]
fn test_single_version_and_empty_attr() {
    let d = DefaultVersioned { value: 42 };
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains(r#""version":1"#));
    assert_eq!(serde_json::from_str::<DefaultVersioned>(&json).unwrap(), d);

    let s = ExplicitSingleVersion {
        label: "hello".to_owned(),
    };
    let s_json = serde_json::to_string(&s).unwrap();
    assert!(s_json.contains(r#""version":1"#));
    assert_eq!(serde_json::from_str::<ExplicitSingleVersion>(&s_json).unwrap(), s);
}

// =========================================================================
// 3. Shadow wire: tuple struct (adjacent), unit struct, non-Clone, serde attrs
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct PointV1(pub i32);

#[backwards_compat(
    content = "data",
    versions(
        1: PointV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct Point(pub i32, pub i32);

impl From<PointV1> for Point {
    fn from(v: PointV1) -> Self {
        Self(v.0, 0)
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct EmptyV1;

#[backwards_compat(
    versions(
        1: EmptyV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct EmptyUnit;

impl From<EmptyV1> for EmptyUnit {
    fn from(_: EmptyV1) -> Self {
        Self
    }
}

#[backwards_compat(versions(1: Self))]
#[derive(Debug, PartialEq, Eq)]
pub struct NonCloneShadow {
    pub secret: String,
    pub count: u32,
}

#[backwards_compat(versions(1: Self))]
#[derive(Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedSerdeAttrs {
    #[serde(alias = "user_name")]
    pub first_name: String,
    #[serde(default)]
    pub retry_count: u32,
}

#[test]
fn test_shadow_wire_tuple_unit_non_clone_and_serde_attrs() {
    // Adjacent tuple struct
    let p_v1: Point = serde_json::from_str(r#"{"version":1,"data":7}"#).unwrap();
    assert_eq!(p_v1, Point(7, 0));
    let p_json = serde_json::to_string(&Point(3, 4)).unwrap();
    assert_eq!(p_json, r#"{"version":2,"data":[3,4]}"#);
    assert_eq!(serde_json::from_str::<Point>(&p_json).unwrap(), Point(3, 4));

    // Unit struct (internal tagging omits __content)
    let u_json = serde_json::to_string(&EmptyUnit).unwrap();
    assert_eq!(u_json, r#"{"version":2}"#);
    assert_eq!(serde_json::from_str::<EmptyUnit>(&u_json).unwrap(), EmptyUnit);
    assert_eq!(
        serde_json::from_str::<EmptyUnit>(r#"{"version":1}"#).unwrap(),
        EmptyUnit
    );

    // Non-Clone zero-copy shadow wire
    let nc = NonCloneShadow {
        secret: "abc".to_owned(),
        count: 9,
    };
    let nc_json = serde_json::to_string(&nc).unwrap();
    assert_eq!(serde_json::from_str::<NonCloneShadow>(&nc_json).unwrap(), nc);

    // Forwarded serde container + field attrs
    let f: ForwardedSerdeAttrs = serde_json::from_str(r#"{"version":1,"user_name":"Ada"}"#).unwrap();
    assert_eq!(
        f,
        ForwardedSerdeAttrs {
            first_name: "Ada".to_owned(),
            retry_count: 0,
        }
    );
    let f_json = serde_json::to_string(&f).unwrap();
    assert!(f_json.contains(r#""firstName":"Ada""#));
    assert!(f_json.contains(r#""retryCount":0"#));
}

// =========================================================================
// 4. Explicit separate wire structs (borrowed From<&T>, Clone fallback,
//    deserialize-only historical structs)
// =========================================================================

// Historical V1 only derives Deserialize (not Serialize)
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct HistV1 {
    pub raw: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplicitWireV2 {
    pub value: String,
    pub len: usize,
}

impl From<HistV1> for ExplicitWireV2 {
    fn from(v: HistV1) -> Self {
        let len = v.raw.len();
        Self { value: v.raw, len }
    }
}

// Non-Clone target using borrowed `From<&Target> for Wire`
#[backwards_compat(
    versions(
        1: HistV1,
        2: ExplicitWireV2,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct BorrowedWireTarget {
    pub value: String,
}

impl From<ExplicitWireV2> for BorrowedWireTarget {
    fn from(w: ExplicitWireV2) -> Self {
        Self { value: w.value }
    }
}

impl From<&BorrowedWireTarget> for ExplicitWireV2 {
    fn from(t: &BorrowedWireTarget) -> Self {
        Self {
            value: t.value.clone(),
            len: t.value.len(),
        }
    }
}

// Clone target using owned `From<Target> for Wire` fallback
#[backwards_compat(
    versions(
        1: HistV1,
        2: ExplicitWireV2,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedWireTarget {
    pub value: String,
}

impl From<ExplicitWireV2> for OwnedWireTarget {
    fn from(w: ExplicitWireV2) -> Self {
        Self { value: w.value }
    }
}

impl From<OwnedWireTarget> for ExplicitWireV2 {
    fn from(t: OwnedWireTarget) -> Self {
        let len = t.value.len();
        Self { value: t.value, len }
    }
}

#[test]
fn test_explicit_separate_wire_borrowed_and_owned() {
    let b: BorrowedWireTarget = serde_json::from_str(r#"{"version":1,"raw":"hello"}"#).unwrap();
    assert_eq!(
        b,
        BorrowedWireTarget {
            value: "hello".to_owned()
        }
    );
    let b_json = serde_json::to_string(&b).unwrap();
    assert!(b_json.contains(r#""version":2"#));
    assert!(b_json.contains(r#""len":5"#));
    assert_eq!(serde_json::from_str::<BorrowedWireTarget>(&b_json).unwrap(), b);

    let o: OwnedWireTarget = serde_json::from_str(r#"{"version":1,"raw":"world"}"#).unwrap();
    assert_eq!(
        o,
        OwnedWireTarget {
            value: "world".to_owned()
        }
    );
    let o_json = serde_json::to_string(&o).unwrap();
    assert!(o_json.contains(r#""version":2"#));
    assert_eq!(serde_json::from_str::<OwnedWireTarget>(&o_json).unwrap(), o);
}
