use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Implicit shadow wire `enum` (unit, tuple, struct variants, non-Clone,
//    adjacent tagging, and #[cfg] on enum variants)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub enum EventV1 {
    Ping,
    Click(u32),
}

#[backwards_compat(
    content = "payload",
    versions(
        1: EventV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Ping,
    Click(u32, u32),
    Message {
        text: String,
    },
    #[cfg(test)]
    TestOnly(u8),
    #[cfg(any())]
    DisabledVariant(String),
}

impl From<EventV1> for Event {
    fn from(v: EventV1) -> Self {
        match v {
            EventV1::Ping => Self::Ping,
            EventV1::Click(x) => Self::Click(x, 0),
        }
    }
}

#[test]
fn test_enum_shadow_wire_variants_and_cfg() {
    // Upgrade from v1 enum
    let ping: Event = serde_json::from_str(r#"{"version":1,"payload":"Ping"}"#).unwrap();
    assert_eq!(ping, Event::Ping);

    let click: Event = serde_json::from_str(r#"{"version":1,"payload":{"Click":5}}"#).unwrap();
    assert_eq!(click, Event::Click(5, 0));

    // Roundtrip v2 non-Clone enum variants (unit, tuple, struct, #[cfg(test)])
    for ev in [
        Event::Ping,
        Event::Click(1, 2),
        Event::Message {
            text: "hi".to_owned(),
        },
        Event::TestOnly(7),
    ] {
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""version":2"#));
        let de: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(de, ev);
    }
}

// =========================================================================
// 2. Explicit wire enum
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModeWireV1 {
    Fast,
    Safe,
}

#[backwards_compat(
    content = "mode",
    versions(
        1: ModeWireV1,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Fast,
    Safe,
}

impl From<ModeWireV1> for Mode {
    fn from(w: ModeWireV1) -> Self {
        match w {
            ModeWireV1::Fast => Self::Fast,
            ModeWireV1::Safe => Self::Safe,
        }
    }
}

impl From<&Mode> for ModeWireV1 {
    fn from(m: &Mode) -> Self {
        match m {
            Mode::Fast => Self::Fast,
            Mode::Safe => Self::Safe,
        }
    }
}

#[test]
fn test_explicit_wire_enum() {
    let json = serde_json::to_string(&Mode::Fast).unwrap();
    assert_eq!(json, r#"{"version":1,"mode":"Fast"}"#);
    assert_eq!(serde_json::from_str::<Mode>(&json).unwrap(), Mode::Fast);
}

// =========================================================================
// 3. Generic + lifetime enum
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub enum PacketV1<'a, T> {
    Raw(&'a str),
    Data(T),
}

#[backwards_compat(
    content = "body",
    versions(
        1: PacketV1<'a, T>,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum Packet<'a, T: Clone> {
    Raw(&'a str),
    Data { value: T, verified: bool },
}

impl<'a, T: Clone> From<PacketV1<'a, T>> for Packet<'a, T> {
    fn from(v: PacketV1<'a, T>) -> Self {
        match v {
            PacketV1::Raw(s) => Self::Raw(s),
            PacketV1::Data(value) => Self::Data {
                value,
                verified: false,
            },
        }
    }
}

#[test]
fn test_generic_lifetime_enum() {
    let raw: Packet<'_, i32> = serde_json::from_str(r#"{"version":1,"body":{"Raw":"hello"}}"#).unwrap();
    assert_eq!(raw, Packet::Raw("hello"));

    let data: Packet<'_, i32> = serde_json::from_str(r#"{"version":1,"body":{"Data":42}}"#).unwrap();
    assert_eq!(
        data,
        Packet::Data {
            value: 42,
            verified: false,
        }
    );

    let json = serde_json::to_string(&raw).unwrap();
    assert!(json.contains(r#""version":2"#));
    let roundtrip: Packet<'_, i32> = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, raw);
}

// =========================================================================
// 4. Generic/lifetime struct with where clause + adjacent scalar tagging
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenericRecordV1<'a, T> {
    pub label: &'a str,
    pub item: T,
}

#[backwards_compat(
    tag = "v",
    content = "data",
    versions(
        1: GenericRecordV1<'a, T>,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericRecord<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    pub label: &'a str,
    pub item: T,
    pub active: bool,
}

impl<'a, T> From<GenericRecordV1<'a, T>> for GenericRecord<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    fn from(v: GenericRecordV1<'a, T>) -> Self {
        Self {
            label: v.label,
            item: v.item,
            active: true,
        }
    }
}

#[test]
fn test_generic_struct_with_where_clause_and_adjacent_tagging() {
    let v1: GenericRecord<'_, u32> =
        serde_json::from_str(r#"{"v":1,"data":{"label":"sensor","item":99}}"#).unwrap();
    assert_eq!(
        v1,
        GenericRecord {
            label: "sensor",
            item: 99,
            active: true,
        }
    );

    let json = serde_json::to_string(&v1).unwrap();
    assert!(json.contains(r#""v":2"#));
    assert!(json.contains(r#""data":{"#));
    let roundtrip: GenericRecord<'_, u32> = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, v1);
}

// =========================================================================
// 4. External tagging with generics, lifetimes, where clause, and #[cfg]
// =========================================================================

#[backwards_compat(
    external,
    versions(
        #[cfg(any())]
        "v0": GenericRecordV1<'a, T> => "v2",
        #[cfg(test)]
        "v1": GenericRecordV1<'a, T> => "v2",
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ExtGenericRecord<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    pub label: &'a str,
    pub item: T,
    pub active: bool,
}

impl<'a, T> From<GenericRecordV1<'a, T>> for ExtGenericRecord<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    fn from(v: GenericRecordV1<'a, T>) -> Self {
        Self {
            label: v.label,
            item: v.item,
            active: true,
        }
    }
}

#[test]
fn test_external_tagging_generics_cfg_and_where_clause() {
    let v1: ExtGenericRecord<'_, u32> =
        serde_json::from_str(r#"{"v1":{"label":"sensor","item":42}}"#).unwrap();
    assert_eq!(
        v1,
        ExtGenericRecord {
            label: "sensor",
            item: 42,
            active: true,
        }
    );

    let json = serde_json::to_string(&v1).unwrap();
    assert_eq!(json, r#"{"v2":{"label":"sensor","item":42,"active":true}}"#);
    let roundtrip: ExtGenericRecord<'_, u32> = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, v1);

    // Disabled #[cfg(any())] version "v0" must be rejected with feature-flag hint
    let err =
        serde_json::from_str::<ExtGenericRecord<'_, u32>>(r#"{"v0":{"label":"old","item":1}}"#).unwrap_err();
    assert!(
        err.to_string().contains("disabled by feature flags"),
        "unexpected gated external error: {err}"
    );
}
