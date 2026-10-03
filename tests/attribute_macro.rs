use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// Case 1: Target is wire (shadow wire struct generated) with named fields
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UserV1 {
    pub name: String,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: UserV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub age: u32,
}

impl From<UserV1> for User {
    fn from(v1: UserV1) -> Self {
        Self {
            name: v1.name,
            age: 18,
        }
    }
}

// Case 2: Target is wire with tuple struct
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PointV1(pub i32);

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: PointV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point(pub i32, pub i32);

impl From<PointV1> for Point {
    fn from(v1: PointV1) -> Self {
        Self(v1.0, 0)
    }
}

// Case 3: Target is wire with unit struct
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EmptyV1;

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: EmptyV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Empty;

impl From<EmptyV1> for Empty {
    fn from(_: EmptyV1) -> Self {
        Self
    }
}

// Case 4: Target is NOT wire (separate wire struct)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SeparateV1 {
    pub val: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeparateWire {
    pub val: i32,
    pub extra: String,
}

impl From<SeparateV1> for SeparateWire {
    fn from(v1: SeparateV1) -> Self {
        Self {
            val: v1.val,
            extra: "wire".to_owned(),
        }
    }
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: SeparateV1,
        2: SeparateWire,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeparateTarget {
    pub val: i32,
    pub extra: String,
}

impl From<SeparateWire> for SeparateTarget {
    fn from(wire: SeparateWire) -> Self {
        Self {
            val: wire.val,
            extra: wire.extra,
        }
    }
}

impl From<SeparateTarget> for SeparateWire {
    fn from(target: SeparateTarget) -> Self {
        Self {
            val: target.val,
            extra: target.extra,
        }
    }
}

// Case 5: Preserved serde attributes on target struct
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CamelV1 {
    pub first_name: String,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: CamelV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CamelUser {
    pub first_name: String,
    pub last_name: String,
}

impl From<CamelV1> for CamelUser {
    fn from(v1: CamelV1) -> Self {
        Self {
            first_name: v1.first_name,
            last_name: "Doe".to_owned(),
        }
    }
}

// Case 5b: Field-level serde attributes
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FieldV1 {
    pub name: String,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: FieldV1,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldUser {
    #[serde(rename = "user_name")]
    pub name: String,
}

impl From<FieldV1> for FieldUser {
    fn from(v1: FieldV1) -> Self {
        Self { name: v1.name }
    }
}

// Case 6: Fallible migration and DAG transitions
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DagV1 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DagV2 {
    pub count: usize,
}

#[backwards_compat(
    tag = "version",
    version = 3,
    versions(
        #[fallible]
        1: DagV1 => 3,
        2: DagV2 => 3,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagTarget {
    pub count: usize,
}

impl TryFrom<DagV1> for DagTarget {
    type Error = core::num::ParseIntError;
    fn try_from(v1: DagV1) -> Result<Self, Self::Error> {
        let count = v1.raw.parse()?;
        Ok(Self { count })
    }
}

impl From<DagV2> for DagTarget {
    fn from(v2: DagV2) -> Self {
        Self { count: v2.count }
    }
}

#[test]
fn test_attribute_macro_named_struct_shadow_wire() {
    let json_v1 = r#"{"version": "1", "name": "Alice"}"#;
    let user_from_v1: User = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        user_from_v1,
        User {
            name: "Alice".to_owned(),
            age: 18,
        }
    );

    let json_v2 = r#"{"version": "2", "name": "Bob", "age": 30}"#;
    let user_from_v2: User = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        user_from_v2,
        User {
            name: "Bob".to_owned(),
            age: 30,
        }
    );

    let serialized = serde_json::to_string(&user_from_v2).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
    assert!(serialized.contains(r#""name":"Bob""#));
    assert!(serialized.contains(r#""age":30"#));

    let roundtripped: User = serde_json::from_str(&serialized).unwrap();
    assert_eq!(roundtripped, user_from_v2);

    // Direct From conversion
    let direct_user = User::from(UserV1 {
        name: "Carol".to_owned(),
    });
    assert_eq!(
        direct_user,
        User {
            name: "Carol".to_owned(),
            age: 18,
        }
    );
}

#[test]
fn test_attribute_macro_tuple_struct() {
    let pt = Point(10, 20);
    let serialized_res = serde_json::to_string(&pt);
    // In Serde, internally tagged enums cannot serialize tuple structs into maps
    serialized_res.unwrap_err();
}

#[test]
fn test_attribute_macro_unit_struct() {
    let json_v1 = r#"{"version": "1"}"#;
    let empty1: Empty = serde_json::from_str(json_v1).unwrap();
    assert_eq!(empty1, Empty);

    let json_v2 = r#"{"version": "2"}"#;
    let empty2: Empty = serde_json::from_str(json_v2).unwrap();
    assert_eq!(empty2, Empty);

    let serialized = serde_json::to_string(&Empty).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
}

#[test]
fn test_attribute_macro_separate_wire() {
    let json_v1 = r#"{"version": "1", "val": 100}"#;
    let target1: SeparateTarget = serde_json::from_str(json_v1).unwrap();
    assert_eq!(
        target1,
        SeparateTarget {
            val: 100,
            extra: "wire".to_owned(),
        }
    );

    let json_v2 = r#"{"version": "2", "val": 200, "extra": "custom"}"#;
    let target2: SeparateTarget = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        target2,
        SeparateTarget {
            val: 200,
            extra: "custom".to_owned(),
        }
    );

    let serialized = serde_json::to_string(&target2).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
    assert!(serialized.contains(r#""val":200"#));
    assert!(serialized.contains(r#""extra":"custom""#));
}

#[test]
fn test_attribute_macro_preserved_serde_attributes() {
    let json_v2 = r#"{"version": "2", "firstName": "John", "lastName": "Smith"}"#;
    let user: CamelUser = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        user,
        CamelUser {
            first_name: "John".to_owned(),
            last_name: "Smith".to_owned(),
        }
    );

    let serialized = serde_json::to_string(&user).unwrap();
    assert!(serialized.contains(r#""firstName":"John""#));
    assert!(serialized.contains(r#""lastName":"Smith""#));
}

#[test]
fn test_attribute_macro_field_serde_attributes() {
    let json_v2 = r#"{"version": "2", "user_name": "Alice"}"#;
    let user: FieldUser = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        user,
        FieldUser {
            name: "Alice".to_owned(),
        }
    );

    let serialized = serde_json::to_string(&user).unwrap();
    assert!(serialized.contains(r#""user_name":"Alice""#));
}

#[test]
fn test_attribute_macro_fallible_and_dag() {
    let json_v3_ok = r#"{"version": "3", "count": 42}"#;
    let target_v3: DagTarget = serde_json::from_str(json_v3_ok).unwrap();
    assert_eq!(target_v3, DagTarget { count: 42 });

    let json_v2 = r#"{"version": "2", "count": 99}"#;
    let target_v2: DagTarget = serde_json::from_str(json_v2).unwrap();
    assert_eq!(target_v2, DagTarget { count: 99 });

    let json_v1_valid = r#"{"version": "1", "raw": "123"}"#;
    let target_v1: DagTarget = serde_json::from_str(json_v1_valid).unwrap();
    assert_eq!(target_v1, DagTarget { count: 123 });

    let json_v1_invalid = r#"{"version": "1", "raw": "not_a_number"}"#;
    let res: Result<DagTarget, _> = serde_json::from_str(json_v1_invalid);
    res.unwrap_err();
}

// Case 7: Non-Cloneable domain struct when target_is_wire is true
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct NonCloneV1 {
    pub message: String,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: NonCloneV1,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NonCloneTarget {
    pub message: String,
    pub count: usize,
}

impl From<NonCloneV1> for NonCloneTarget {
    fn from(v1: NonCloneV1) -> Self {
        Self {
            message: v1.message,
            count: 0,
        }
    }
}

#[test]
fn test_attribute_macro_non_cloneable_serialization() {
    let target = NonCloneTarget {
        message: "hello_world".to_owned(),
        count: 42,
    };

    let serialized = serde_json::to_string(&target).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
    assert!(serialized.contains(r#""message":"hello_world""#));
    assert!(serialized.contains(r#""count":42"#));

    let deserialized: NonCloneTarget = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, target);

    let v1_json = r#"{"version": "1", "message": "from_v1"}"#;
    let from_v1: NonCloneTarget = serde_json::from_str(v1_json).unwrap();
    assert_eq!(
        from_v1,
        NonCloneTarget {
            message: "from_v1".to_owned(),
            count: 0,
        }
    );
}

// Case 8: Fallible aliases testing #[try], #[try_from], #[try_into], #[falliable]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AliasV1 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AliasV2 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AliasV3 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AliasV4 {
    pub raw: String,
}

#[backwards_compat(
    tag = "version",
    version = 5,
    versions(
        #[r#try]
        1: AliasV1 => 2,
        #[try_from]
        2: AliasV2 => 3,
        #[try_into]
        3: AliasV3 => 4,
        #[falliable]
        4: AliasV4 => 5,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasTarget {
    pub val: u32,
}

impl TryFrom<AliasV1> for AliasV2 {
    type Error = core::num::ParseIntError;
    fn try_from(v: AliasV1) -> Result<Self, Self::Error> {
        let _num: u32 = v.raw.parse()?;
        Ok(Self { raw: v.raw })
    }
}

impl TryFrom<AliasV2> for AliasV3 {
    type Error = core::num::ParseIntError;
    fn try_from(v: AliasV2) -> Result<Self, Self::Error> {
        let _num: u32 = v.raw.parse()?;
        Ok(Self { raw: v.raw })
    }
}

impl TryFrom<AliasV3> for AliasV4 {
    type Error = core::num::ParseIntError;
    fn try_from(v: AliasV3) -> Result<Self, Self::Error> {
        let _num: u32 = v.raw.parse()?;
        Ok(Self { raw: v.raw })
    }
}

impl TryFrom<AliasV4> for AliasTarget {
    type Error = core::num::ParseIntError;
    fn try_from(v: AliasV4) -> Result<Self, Self::Error> {
        let val = v.raw.parse()?;
        Ok(Self { val })
    }
}

#[test]
fn test_attribute_macro_fallible_aliases() {
    let json_v1 = r#"{"version": "1", "raw": "100"}"#;
    let target1: AliasTarget = serde_json::from_str(json_v1).unwrap();
    assert_eq!(target1, AliasTarget { val: 100 });

    let json_v2 = r#"{"version": "2", "raw": "200"}"#;
    let target2: AliasTarget = serde_json::from_str(json_v2).unwrap();
    assert_eq!(target2, AliasTarget { val: 200 });

    let json_v3 = r#"{"version": "3", "raw": "300"}"#;
    let target3: AliasTarget = serde_json::from_str(json_v3).unwrap();
    assert_eq!(target3, AliasTarget { val: 300 });

    let json_v4 = r#"{"version": "4", "raw": "400"}"#;
    let target4: AliasTarget = serde_json::from_str(json_v4).unwrap();
    assert_eq!(target4, AliasTarget { val: 400 });

    let json_v1_err = r#"{"version": "1", "raw": "invalid"}"#;
    let err_res: Result<AliasTarget, _> = serde_json::from_str(json_v1_err);
    err_res.unwrap_err();
}

// Case 9: Adjacent tagging with tuple struct
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjPointV1(pub i32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjPointV2(pub i32, pub i32);

impl From<AdjPointV1> for AdjPointV2 {
    fn from(v1: AdjPointV1) -> Self {
        Self(v1.0, 0)
    }
}

#[backwards_compat(
    tag = "version",
    content = "data",
    version = 2,
    versions(
        1: AdjPointV1 => 2,
        2: AdjPointV2,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjPoint(pub i32, pub i32);

impl From<AdjPointV2> for AdjPoint {
    fn from(v2: AdjPointV2) -> Self {
        Self(v2.0, v2.1)
    }
}

impl From<AdjPoint> for AdjPointV2 {
    fn from(target: AdjPoint) -> Self {
        Self(target.0, target.1)
    }
}

#[test]
fn test_attribute_macro_adjacent_tagging_tuple_struct() {
    let pt = AdjPoint(10, 20);
    let serialized = serde_json::to_string(&pt).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
    assert!(serialized.contains(r#""data":[10,20]"#));

    let deserialized: AdjPoint = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, pt);

    let v1_json = r#"{"version": "1", "data": 42}"#;
    let from_v1: AdjPoint = serde_json::from_str(v1_json).unwrap();
    assert_eq!(from_v1, AdjPoint(42, 0));
}

// Case 10: Explicit wire struct with From<&Target> (target does not implement Clone)
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplicitWireV2 {
    pub message: String,
    pub count: usize,
}

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        2: ExplicitWireV2,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct NonCloneExplicitTarget {
    pub message: String,
    pub count: usize,
}

impl From<ExplicitWireV2> for NonCloneExplicitTarget {
    fn from(wire: ExplicitWireV2) -> Self {
        Self {
            message: wire.message,
            count: wire.count,
        }
    }
}

impl From<&NonCloneExplicitTarget> for ExplicitWireV2 {
    fn from(target: &NonCloneExplicitTarget) -> Self {
        Self {
            message: target.message.clone(),
            count: target.count,
        }
    }
}

#[test]
fn test_attribute_macro_explicit_wire_borrowed_conversion() {
    let target = NonCloneExplicitTarget {
        message: "borrowed_wire_test".to_owned(),
        count: 77,
    };

    let serialized = serde_json::to_string(&target).unwrap();
    assert!(serialized.contains(r#""version":"2""#));
    assert!(serialized.contains(r#""message":"borrowed_wire_test""#));
    assert!(serialized.contains(r#""count":77"#));

    let deserialized: NonCloneExplicitTarget = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, target);
}
