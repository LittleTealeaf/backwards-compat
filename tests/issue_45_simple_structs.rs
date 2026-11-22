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

    // Historical V1 version upgrade in RON and JSON
    let from_ron_v1: PlainTuple = ron::from_str("(version:1,data:[10,20])").unwrap();
    assert_eq!(from_ron_v1, PlainTuple(vec![10, 20]));

    let from_json_v1: PlainTuple = serde_json::from_str(r#"{"version":1,"data":[10,20]}"#).unwrap();
    assert_eq!(from_json_v1, PlainTuple(vec![10, 20]));

    // Untagged pre-versioned payloads in RON and JSON
    assert_eq!(
        ron::from_str::<PlainTuple>("[5,6]").unwrap(),
        PlainTuple(vec![5, 6])
    );
    assert_eq!(
        serde_json::from_str::<PlainTuple>("[5,6]").unwrap(),
        PlainTuple(vec![5, 6])
    );

    // Malformed 1-element array still fails
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

    // Untagged RON custom value
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
