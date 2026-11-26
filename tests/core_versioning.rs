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

    // RON roundtrip: default struct syntax `(...)` and legacy `{...}` / `(...)` inputs
    let ron_str = ron::to_string(&from_v2).unwrap();
    assert!(
        ron_str.starts_with('(') && ron_str.ends_with(')'),
        "got: {ron_str}"
    );
    let roundtrip_ron: UserProfile = ron::from_str(&ron_str).unwrap();
    assert_eq!(roundtrip_ron, from_v2);

    let from_ron_v1_paren: UserProfile = ron::from_str(r#"(version:1,name:"Alice")"#).unwrap();
    assert_eq!(from_ron_v1_paren, from_v1);
    let from_ron_v1_named: UserProfile = ron::from_str(r#"UserProfile(version:1,name:"Alice")"#).unwrap();
    assert_eq!(from_ron_v1_named, from_v1);
    let from_ron_v1_map: UserProfile = ron::from_str(r#"{"version":1,"name":"Alice"}"#).unwrap();
    assert_eq!(from_ron_v1_map, from_v1);
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
    let u_ron = ron::to_string(&EmptyUnit).unwrap();
    assert_eq!(u_ron, "(version:2)");
    assert_eq!(ron::from_str::<EmptyUnit>(&u_ron).unwrap(), EmptyUnit);
    assert_eq!(ron::from_str::<EmptyUnit>("(version:1)").unwrap(), EmptyUnit);

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

    let b_ron = ron::to_string(&b).unwrap();
    assert!(b_ron.starts_with('(') && b_ron.ends_with(')'), "got: {b_ron}");
    assert_eq!(ron::from_str::<BorrowedWireTarget>(&b_ron).unwrap(), b);
    assert_eq!(
        ron::from_str::<BorrowedWireTarget>(r#"(version:1,raw:"hello")"#).unwrap(),
        b
    );

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

mod tuple_and_scalar_roundtrips {
    use backwards_compat::backwards_compat;
    use serde::{Deserialize, Deserializer, Serializer};

    fn ser_u32_vec_as_csv<S: Serializer>(vals: &[u32], serializer: S) -> Result<S::Ok, S::Error> {
        let joined = vals.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
        serializer.serialize_str(&joined)
    }

    fn de_u32_vec_from_csv<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u32>, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            return Ok(Vec::new());
        }
        s.split(',')
            .map(|part| part.parse::<u32>().map_err(serde::de::Error::custom))
            .collect()
    }

    #[derive(Debug, PartialEq, Eq, Deserialize)]
    #[serde(transparent)]
    pub struct PlainTupleV1(pub Vec<u16>);

    #[backwards_compat(content = "data", versions(_: Self, 1: PlainTupleV1, 2: Self))]
    #[derive(Debug, PartialEq, Eq)]
    pub struct PlainTuple(pub Vec<u32>);

    impl From<PlainTupleV1> for PlainTuple {
        fn from(v: PlainTupleV1) -> Self {
            Self(v.0.into_iter().map(u32::from).collect())
        }
    }

    #[backwards_compat(content = "data", versions(_: Self, 1: Self))]
    #[derive(Debug, PartialEq, Eq)]
    pub struct CustomTuple(
        #[serde(
            serialize_with = "ser_u32_vec_as_csv",
            deserialize_with = "de_u32_vec_from_csv"
        )]
        pub Vec<u32>,
    );

    #[backwards_compat(content = "data", versions(1: Self))]
    #[derive(Debug, PartialEq, Eq)]
    pub struct ScalarTuple(pub u32);

    #[backwards_compat(content = "data", versions(1: Self))]
    #[derive(Debug, PartialEq, Eq)]
    pub struct NestedVecTuple(pub Vec<Vec<u32>>);

    #[test]
    fn test_plain_tuple_roundtrip_json_ron_toml() {
        for val in [
            PlainTuple(vec![]),
            PlainTuple(vec![42]),
            PlainTuple(vec![1, 2, 3]),
        ] {
            let json = serde_json::to_string(&val).unwrap();
            assert_eq!(serde_json::from_str::<PlainTuple>(&json).unwrap(), val);

            let ron_str = ron::to_string(&val).unwrap();
            assert_eq!(ron::from_str::<PlainTuple>(&ron_str).unwrap(), val);

            let toml_str = toml::to_string(&val).unwrap();
            assert_eq!(toml::from_str::<PlainTuple>(&toml_str).unwrap(), val);
        }

        let from_ron_v1: PlainTuple = ron::from_str("(version:1,data:[10,20])").unwrap();
        assert_eq!(from_ron_v1, PlainTuple(vec![10, 20]));

        let from_json_v1: PlainTuple = serde_json::from_str(r#"{"version":1,"data":[10,20]}"#).unwrap();
        assert_eq!(from_json_v1, PlainTuple(vec![10, 20]));

        assert_eq!(
            ron::from_str::<PlainTuple>("[5,6]").unwrap(),
            PlainTuple(vec![5, 6])
        );
        assert_eq!(
            serde_json::from_str::<PlainTuple>("[5,6]").unwrap(),
            PlainTuple(vec![5, 6])
        );

        let _ = serde_json::from_str::<PlainTuple>(r#"{"version":2,"data":["bad"]}"#).unwrap_err();
    }

    #[test]
    fn test_custom_serde_tuple_roundtrip_json_ron_toml() {
        for val in [
            CustomTuple(vec![]),
            CustomTuple(vec![7]),
            CustomTuple(vec![1, 2, 3]),
        ] {
            let json = serde_json::to_string(&val).unwrap();
            assert_eq!(serde_json::from_str::<CustomTuple>(&json).unwrap(), val);

            let ron_str = ron::to_string(&val).unwrap();
            assert_eq!(ron::from_str::<CustomTuple>(&ron_str).unwrap(), val);

            let toml_str = toml::to_string(&val).unwrap();
            assert_eq!(toml::from_str::<CustomTuple>(&toml_str).unwrap(), val);
        }

        assert_eq!(
            ron::from_str::<CustomTuple>(r#""1,2,3""#).unwrap(),
            CustomTuple(vec![1, 2, 3])
        );
    }

    #[test]
    fn test_scalar_and_nested_vec_tuple_roundtrip() {
        let scalar = ScalarTuple(42);
        let scalar_json = serde_json::to_string(&scalar).unwrap();
        assert_eq!(serde_json::from_str::<ScalarTuple>(&scalar_json).unwrap(), scalar);
        let scalar_ron = ron::to_string(&scalar).unwrap();
        assert_eq!(ron::from_str::<ScalarTuple>(&scalar_ron).unwrap(), scalar);

        for nested in [
            NestedVecTuple(vec![]),
            NestedVecTuple(vec![vec![]]),
            NestedVecTuple(vec![vec![1, 2]]),
            NestedVecTuple(vec![vec![1], vec![2, 3]]),
        ] {
            let json = serde_json::to_string(&nested).unwrap();
            assert_eq!(serde_json::from_str::<NestedVecTuple>(&json).unwrap(), nested);

            let ron_str = ron::to_string(&nested).unwrap();
            assert_eq!(ron::from_str::<NestedVecTuple>(&ron_str).unwrap(), nested);
        }
    }
}

// =========================================================================
// 5. RON pretty struct_names(true) emits real domain struct name
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct AdjUserProfileV1 {
    pub handle: String,
}

#[backwards_compat(
    tag = "version",
    content = "data",
    versions(
        1: AdjUserProfileV1,
        2: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjUserProfile {
    pub handle: String,
    pub verified: bool,
}

impl From<AdjUserProfileV1> for AdjUserProfile {
    fn from(v: AdjUserProfileV1) -> Self {
        Self {
            handle: v.handle,
            verified: false,
        }
    }
}

#[test]
fn test_ron_pretty_emits_real_struct_names() {
    let pretty_cfg = ron::ser::PrettyConfig::new().struct_names(true);

    // Internally-tagged struct emits `UserProfile(...)`, never `__SerializeHelper(...)`
    let profile = UserProfile {
        first_name: "Alice".to_owned(),
        last_name: "Smith".to_owned(),
        active: true,
    };
    let profile_ron = ron::ser::to_string_pretty(&profile, pretty_cfg.clone()).unwrap();
    assert!(
        profile_ron.starts_with("UserProfile("),
        "expected UserProfile struct name in RON output, got: {profile_ron}"
    );
    assert!(
        !profile_ron.contains("__SerializeHelper"),
        "RON output must not leak __SerializeHelper: {profile_ron}"
    );
    assert_eq!(ron::from_str::<UserProfile>(&profile_ron).unwrap(), profile);

    // Adjacently-tagged struct emits `AdjUserProfile(...)`, never `__SerializeHelper(...)`
    let adj_profile = AdjUserProfile {
        handle: "alice".to_owned(),
        verified: true,
    };
    let adj_ron = ron::ser::to_string_pretty(&adj_profile, pretty_cfg.clone()).unwrap();
    assert!(
        adj_ron.starts_with("AdjUserProfile("),
        "expected AdjUserProfile struct name in RON output, got: {adj_ron}"
    );
    assert!(
        !adj_ron.contains("__SerializeHelper"),
        "RON output must not leak __SerializeHelper: {adj_ron}"
    );
    let adj_compact_ron = ron::to_string(&adj_profile).unwrap();
    assert_eq!(
        ron::from_str::<AdjUserProfile>(&adj_compact_ron).unwrap(),
        adj_profile
    );

    // Unit struct and tuple struct also emit their real struct names
    let unit_ron = ron::ser::to_string_pretty(&EmptyUnit, pretty_cfg.clone()).unwrap();
    assert!(
        unit_ron.starts_with("EmptyUnit("),
        "expected EmptyUnit struct name in RON output, got: {unit_ron}"
    );
    assert!(
        !unit_ron.contains("__SerializeHelper"),
        "RON output must not leak __SerializeHelper: {unit_ron}"
    );
    assert_eq!(ron::from_str::<EmptyUnit>(&unit_ron).unwrap(), EmptyUnit);

    let point = Point(3, 4);
    let point_ron = ron::ser::to_string_pretty(&point, pretty_cfg).unwrap();
    assert!(
        point_ron.starts_with("Point("),
        "expected Point struct name in RON output, got: {point_ron}"
    );
    assert!(
        !point_ron.contains("__SerializeHelper"),
        "RON output must not leak __SerializeHelper: {point_ron}"
    );
    let point_compact_ron = ron::to_string(&point).unwrap();
    assert_eq!(ron::from_str::<Point>(&point_compact_ron).unwrap(), point);
}
