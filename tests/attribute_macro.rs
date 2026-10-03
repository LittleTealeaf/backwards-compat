use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// Case 1: Target is wire (shadow wire struct generated) with named fields
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PointV1(pub i32);

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: PointV1,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Point(pub i32, pub i32);

impl From<PointV1> for Point {
    fn from(v1: PointV1) -> Self {
        Point(v1.0, 0)
    }
}

// Case 3: Target is wire with unit struct
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmptyV1;

#[backwards_compat(
    tag = "version",
    version = 2,
    versions(
        1: EmptyV1,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Empty;

impl From<EmptyV1> for Empty {
    fn from(_: EmptyV1) -> Self {
        Empty
    }
}

// Case 4: Target is NOT wire (separate wire struct)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeparateV1 {
    pub val: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeparateWire {
    pub val: i32,
    pub extra: String,
}

impl From<SeparateV1> for SeparateWire {
    fn from(v1: SeparateV1) -> Self {
        Self {
            val: v1.val,
            extra: "wire".to_string(),
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CamelUser {
    pub first_name: String,
    pub last_name: String,
}

impl From<CamelV1> for CamelUser {
    fn from(v1: CamelV1) -> Self {
        Self {
            first_name: v1.first_name,
            last_name: "Doe".to_string(),
        }
    }
}

// Case 5b: Field-level serde attributes
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DagV1 {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct DagTarget {
    pub count: usize,
}

impl TryFrom<DagV1> for DagTarget {
    type Error = std::num::ParseIntError;
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
            name: "Alice".to_string(),
            age: 18,
        }
    );

    let json_v2 = r#"{"version": "2", "name": "Bob", "age": 30}"#;
    let user_from_v2: User = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        user_from_v2,
        User {
            name: "Bob".to_string(),
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
        name: "Carol".to_string(),
    });
    assert_eq!(
        direct_user,
        User {
            name: "Carol".to_string(),
            age: 18,
        }
    );
}

#[test]
fn test_attribute_macro_tuple_struct() {
    let pt = Point(10, 20);
    let serialized_res = serde_json::to_string(&pt);
    // In Serde, internally tagged enums cannot serialize tuple structs into maps
    assert!(serialized_res.is_err());
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
            extra: "wire".to_string(),
        }
    );

    let json_v2 = r#"{"version": "2", "val": 200, "extra": "custom"}"#;
    let target2: SeparateTarget = serde_json::from_str(json_v2).unwrap();
    assert_eq!(
        target2,
        SeparateTarget {
            val: 200,
            extra: "custom".to_string(),
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
            first_name: "John".to_string(),
            last_name: "Smith".to_string(),
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
            name: "Alice".to_string(),
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
    assert!(res.is_err());
}
