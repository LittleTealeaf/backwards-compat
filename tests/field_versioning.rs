use core::fmt;

use backwards_compat::{BackwardsCompat, Migrated, backwards_compat};
use serde::{Deserialize, Serialize};

fn assert_borrowed_within(borrowed: &str, input: &str) {
    let input_start = input.as_ptr() as usize;
    let input_end = input_start + input.len();
    let borrowed_start = borrowed.as_ptr() as usize;
    let borrowed_end = borrowed_start + borrowed.len();
    assert!(
        borrowed_start >= input_start && borrowed_end <= input_end,
        "expected `{borrowed}` ({borrowed_start:#x}..{borrowed_end:#x}) to be zero-copy borrowed from input ({input_start:#x}..{input_end:#x})"
    );
}

// =========================================================================
// 1. Scalars (u16 with heterogeneous Display-only fallible errors + #[with],
//    bool), nested structs, Vec<T>, Option<T> (None/Some + skip_serializing_if)
// =========================================================================

// Intentionally implements ONLY core::fmt::Display (not std::error::Error).
struct ParsePortErr(String);

impl fmt::Display for ParsePortErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid port string `{}`", self.0)
    }
}

// Second distinct Display-only error type in the same chain.
struct RangePortErr(u32);

impl fmt::Display for RangePortErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "port {} out of range (1..=65535)", self.0)
    }
}

fn parse_port_str(s: String) -> Result<u32, ParsePortErr> {
    s.parse::<u32>().map_err(|_| ParsePortErr(s))
}

fn narrow_port(p: u32) -> Result<u16, RangePortErr> {
    if p == 0 {
        return Err(RangePortErr(p));
    }
    u16::try_from(p).map_err(|_| RangePortErr(p))
}

const fn bool_from_flag(flag: u8) -> bool {
    flag != 0
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "#[with] converter takes owned source type"
)]
fn tags_from_csv(csv: String) -> Vec<String> {
    if csv.is_empty() {
        Vec::new()
    } else {
        csv.split(',').map(|s| s.trim().to_owned()).collect()
    }
}

fn opt_label_from_str(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct NestedV1 {
    host: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Nested {
    host: String,
    tls: bool,
}

impl From<NestedV1> for Nested {
    fn from(v1: NestedV1) -> Self {
        Self {
            host: v1.host,
            tls: false,
        }
    }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ServiceConfig {
    #[backwards_compat(versions(
        #[fallible] #[with(parse_port_str)] 1: String,
        #[fallible] #[with(narrow_port)] 2: u32,
        3: Self,
    ))]
    port: u16,

    #[backwards_compat(versions(
        #[with(bool_from_flag)] 1: u8,
        2: Self,
    ))]
    enabled: bool,

    #[backwards_compat(versions(
        1: NestedV1,
        2: Self,
    ))]
    upstream: Nested,

    #[backwards_compat(versions(
        #[with(tags_from_csv)] 1: String,
        2: Self,
    ))]
    tags: Vec<String>,

    #[backwards_compat(versions(
        #[with(opt_label_from_str)] 1: String,
        2: Self,
    ))]
    note: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[backwards_compat(versions(
        #[with(opt_label_from_str)] 1: String,
        2: Self,
    ))]
    optional_banner: Option<String>,
}

#[test]
fn test_scalars_nested_vec_and_option_fields() {
    // Migrate every field from v1 (including 2-step String -> u32 -> u16 chain)
    let legacy_json = r#"{
        "port": {"version": 1, "data": "8080"},
        "enabled": {"version": 1, "data": 1},
        "upstream": {"version": 1, "data": {"host": "api.local"}},
        "tags": {"version": 1, "data": "prod, web"},
        "note": {"version": 1, "data": ""},
        "optional_banner": {"version": 1, "data": "welcome"}
    }"#;
    let cfg: ServiceConfig = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(
        cfg,
        ServiceConfig {
            port: 8080,
            enabled: true,
            upstream: Nested {
                host: "api.local".to_owned(),
                tls: false,
            },
            tags: vec!["prod".to_owned(), "web".to_owned()],
            note: None,
            optional_banner: Some("welcome".to_owned()),
        }
    );

    // Roundtrip with Some note and None optional_banner (verifying skip_serializing_if)
    let cfg_none_banner = ServiceConfig {
        port: 443,
        enabled: false,
        upstream: Nested {
            host: "secure.local".to_owned(),
            tls: true,
        },
        tags: vec!["edge".to_owned()],
        note: Some("primary".to_owned()),
        optional_banner: None,
    };
    let serialized = serde_json::to_string(&cfg_none_banner).unwrap();
    assert!(!serialized.contains("optional_banner"));
    assert!(serialized.contains(r#""port":{"version":3,"data":443}"#));
    let roundtripped: ServiceConfig = serde_json::from_str(&serialized).unwrap();
    assert_eq!(roundtripped, cfg_none_banner);

    // Heterogeneous Display-only error 1: ParsePortErr (step 1 -> 2)
    let bad_parse = r#"{
        "port": {"version": 1, "data": "not-a-number"},
        "enabled": {"version": 2, "data": true},
        "upstream": {"version": 2, "data": {"host": "x", "tls": false}},
        "tags": {"version": 2, "data": []},
        "note": {"version": 2, "data": null}
    }"#;
    let err1 = serde_json::from_str::<ServiceConfig>(bad_parse).unwrap_err();
    assert!(err1.to_string().contains("invalid port string `not-a-number`"));

    // Heterogeneous Display-only error 2: RangePortErr (step 2 -> 3)
    let bad_range = r#"{
        "port": {"version": 1, "data": "70000"},
        "enabled": {"version": 2, "data": true},
        "upstream": {"version": 2, "data": {"host": "x", "tls": false}},
        "tags": {"version": 2, "data": []},
        "note": {"version": 2, "data": null}
    }"#;
    let err2 = serde_json::from_str::<ServiceConfig>(bad_range).unwrap_err();
    assert!(err2.to_string().contains("port 70000 out of range"));
}

// =========================================================================
// 2. Multiple fields on the same struct having the same FieldTy with
//    different migration chains (zero trait collisions, independent defaults)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Endpoint {
    host: String,
    port: u16,
}

const fn http_endpoint_from_host(host: String) -> Endpoint {
    Endpoint { host, port: 80 }
}

const fn https_endpoint_from_host(host: String) -> Endpoint {
    Endpoint { host, port: 443 }
}

fn https_endpoint_from_tuple((host, port): (String, u16)) -> Endpoint {
    Endpoint { host, port }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DualEndpoints {
    #[backwards_compat(versions(
        #[with(http_endpoint_from_host)] 1: String,
        2: Self,
    ))]
    http: Endpoint,

    #[backwards_compat(versions(
        #[with(https_endpoint_from_host)] 1: String => Self,
        #[with(https_endpoint_from_tuple)] 2: (String, u16) => Self,
        3: Self,
    ))]
    https: Endpoint,
}

#[test]
fn test_multiple_fields_same_type_different_chains() {
    let legacy = r#"{
        "http": {"version": 1, "data": "example.com"},
        "https": {"version": 1, "data": "secure.example.com"}
    }"#;
    let dual: DualEndpoints = serde_json::from_str(legacy).unwrap();
    assert_eq!(
        dual.http,
        Endpoint {
            host: "example.com".to_owned(),
            port: 80,
        }
    );
    assert_eq!(
        dual.https,
        Endpoint {
            host: "secure.example.com".to_owned(),
            port: 443,
        }
    );

    let v2_https = r#"{
        "http": {"version": 2, "data": {"host": "a", "port": 8080}},
        "https": {"version": 2, "data": ["b", 8443]}
    }"#;
    let dual2: DualEndpoints = serde_json::from_str(v2_https).unwrap();
    assert_eq!(
        dual2.https,
        Endpoint {
            host: "b".to_owned(),
            port: 8443,
        }
    );

    let json = serde_json::to_string(&dual).unwrap();
    assert!(json.contains(r#""http":{"version":2"#));
    assert!(json.contains(r#""https":{"version":3"#));
    let rt: DualEndpoints = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, dual);
}

// =========================================================================
// 3. Generics, lifetimes (&'a str zero-copy), const generics ([u8; N]),
//    where clauses, and unused outer generic parameters
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct BorrowedPairV1<'a, T> {
    label: &'a str,
    item: T,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BorrowedPair<'a, T> {
    label: &'a str,
    item: T,
    rev: u8,
}

impl<'a, T> From<BorrowedPairV1<'a, T>> for BorrowedPair<'a, T> {
    fn from(v1: BorrowedPairV1<'a, T>) -> Self {
        Self {
            label: v1.label,
            item: v1.item,
            rev: 1,
        }
    }
}

fn port_from_u32(v: u32) -> Result<u16, &'static str> {
    u16::try_from(v).map_err(|_| "port overflow")
}

// `U` is an outer generic parameter NOT used by either versioned field.
// `tag_str` uses only `'a`.
// `payload` uses `'a` and `T` (with `where T: fmt::Debug`).
// `port` uses NO outer generic parameters at all.
#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Envelope<'a, T, U>
where
    T: fmt::Debug,
    U: Default,
{
    #[backwards_compat(versions(
        1: &'a str,
        2: Self,
    ))]
    #[serde(borrow)]
    tag_str: &'a str,

    #[backwards_compat(versions(
        1: BorrowedPairV1<'a, T>,
        2: Self,
    ))]
    #[serde(
        borrow,
        bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>")
    )]
    payload: BorrowedPair<'a, T>,

    #[backwards_compat(versions(
        #[fallible] #[with(port_from_u32)] 1: u32,
        2: Self,
    ))]
    port: u16,

    extra: U,
}

#[test]
fn test_generics_lifetimes_zero_copy_and_unused_outer_params() {
    let raw = r#"{
        "tag_str": {"version": 1, "data": "zero-copy-tag"},
        "payload": {"version": 1, "data": {"label": "borrowed-label", "item": 42}},
        "port": {"version": 1, "data": 9000},
        "extra": "meta"
    }"#;

    let env: Envelope<'_, i32, String> = serde_json::from_str(raw).unwrap();
    assert_eq!(env.tag_str, "zero-copy-tag");
    assert_eq!(
        env.payload,
        BorrowedPair {
            label: "borrowed-label",
            item: 42,
            rev: 1,
        }
    );
    assert_eq!(env.port, 9000);
    assert_eq!(env.extra, "meta");

    // Verify true zero-copy borrowing directly into `raw`
    assert_borrowed_within(env.tag_str, raw);
    assert_borrowed_within(env.payload.label, raw);

    // Roundtrip at current versions and verify zero-copy again
    let serialized = serde_json::to_string(&env).unwrap();
    let rt: Envelope<'_, i32, String> = serde_json::from_str(&serialized).unwrap();
    assert_eq!(rt, env);
    assert_borrowed_within(rt.tag_str, &serialized);
    assert_borrowed_within(rt.payload.label, &serialized);
}

// Const generics: `[u8; N]` migrated from `Vec<u8>`
fn fixed_bytes_from_vec<const N: usize>(v: Vec<u8>) -> Result<[u8; N], String> {
    v.try_into()
        .map_err(|bad: Vec<u8>| format!("expected {N} bytes, got {}", bad.len()))
}

fn serialize_fixed_bytes_v2<const N: usize>(arr: &[u8; N]) -> Vec<u8> {
    arr.to_vec()
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FixedFrame<const N: usize> {
    #[backwards_compat(versions(
        #[fallible] #[with(fixed_bytes_from_vec::<N>)] 1: Vec<u8>,
        #[serialize(with = serialize_fixed_bytes_v2::<N>)] #[fallible] #[with(fixed_bytes_from_vec::<N>)] #[current] 2: Vec<u8>,
    ))]
    bytes: [u8; N],
}

#[test]
fn test_const_generic_array_field() {
    let v1_json = r#"{"bytes":{"version":1,"data":[1,2,3,4]}}"#;
    let frame: FixedFrame<4> = serde_json::from_str(v1_json).unwrap();
    assert_eq!(frame.bytes, [1, 2, 3, 4]);

    let encoded = serde_json::to_string(&frame).unwrap();
    assert_eq!(encoded, r#"{"bytes":{"version":2,"data":[1,2,3,4]}}"#);
    let rt: FixedFrame<4> = serde_json::from_str(&encoded).unwrap();
    assert_eq!(rt, frame);

    let wrong_len = r#"{"bytes":{"version":1,"data":[1,2]}}"#;
    let err = serde_json::from_str::<FixedFrame<4>>(wrong_len).unwrap_err();
    assert!(err.to_string().contains("expected 4 bytes, got 2"));
}

// =========================================================================
// 4. Enums with named and unnamed variant fields + #[cfg(...)] on a variant
// =========================================================================

fn widen_u16_to_u32(v: u16) -> u32 {
    u32::from(v)
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Action {
    Idle,
    Move(
        #[backwards_compat(versions(
            #[with(widen_u16_to_u32)] 1: u16,
            2: Self,
        ))]
        u32,
        i32,
    ),
    Resize {
        #[backwards_compat(versions(
            #[with(widen_u16_to_u32)] 1: u16,
            2: Self,
        ))]
        width: u32,
        height: u32,
    },
    #[cfg(test)]
    DebugPing(
        #[backwards_compat(versions(
            1: NestedV1,
            2: Self,
        ))]
        Nested,
    ),
    #[cfg(any())]
    DisabledVariant {
        #[backwards_compat(versions(
            1: NonExistentDisabledType,
            2: Self,
        ))]
        never_compiled: u32,
    },
}

#[test]
fn test_enum_named_unnamed_and_cfg_variants() {
    let idle: Action = serde_json::from_str(r#""Idle""#).unwrap();
    assert_eq!(idle, Action::Idle);

    let moved: Action = serde_json::from_str(r#"{"Move":[{"version":1,"data":12},-5]}"#).unwrap();
    assert_eq!(moved, Action::Move(12, -5));

    let resized: Action =
        serde_json::from_str(r#"{"Resize":{"width":{"version":1,"data":640},"height":480}}"#).unwrap();
    assert_eq!(
        resized,
        Action::Resize {
            width: 640,
            height: 480,
        }
    );

    let ping: Action = serde_json::from_str(r#"{"DebugPing":{"version":1,"data":{"host":"dbg"}}}"#).unwrap();
    assert_eq!(
        ping,
        Action::DebugPing(Nested {
            host: "dbg".to_owned(),
            tls: false,
        })
    );

    for action in [idle, moved, resized, ping] {
        let json = serde_json::to_string(&action).unwrap();
        let rt: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, action);
    }
}

// =========================================================================
// 5. Tuple structs (single-field, multi-field, and Mode 3 single-field
//    with #[serde(transparent)] composition)
// =========================================================================

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SingleTuple(
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    u32,
);

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MultiTuple(
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    u32,
    #[backwards_compat(versions(
        1: NestedV1,
        2: Self,
    ))]
    Nested,
);

#[derive(Debug, Deserialize)]
struct Mode3TupleV1(u16);

impl From<Mode3TupleV1> for Mode3SingleTuple {
    fn from(v1: Mode3TupleV1) -> Self {
        Self(u32::from(v1.0))
    }
}

// Mode 3: outer versioned single-field tuple struct (transparent shadow wire)
// + field-level versioning on field 0. Intentionally non-Clone.
#[backwards_compat(
    content = "payload",
    versions(
        1: Mode3TupleV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct Mode3SingleTuple(
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    u32,
);

#[test]
fn test_tuple_structs_bare_and_mode3_transparent() {
    // Bare single-field tuple struct
    let s1: SingleTuple = serde_json::from_str(r#"{"version":1,"data":7}"#).unwrap();
    assert_eq!(s1, SingleTuple(7));
    let s1_json = serde_json::to_string(&s1).unwrap();
    assert_eq!(s1_json, r#"{"version":2,"data":7}"#);
    assert_eq!(serde_json::from_str::<SingleTuple>(&s1_json).unwrap(), s1);

    // Bare multi-field tuple struct
    let m1: MultiTuple =
        serde_json::from_str(r#"[{"version":1,"data":9},{"version":1,"data":{"host":"h"}}]"#).unwrap();
    assert_eq!(
        m1,
        MultiTuple(
            9,
            Nested {
                host: "h".to_owned(),
                tls: false,
            }
        )
    );
    let m1_json = serde_json::to_string(&m1).unwrap();
    assert_eq!(serde_json::from_str::<MultiTuple>(&m1_json).unwrap(), m1);

    // Mode 3 single-field tuple struct: outer v1 -> v2
    let from_outer_v1: Migrated<Mode3SingleTuple> =
        serde_json::from_str(r#"{"version":1,"payload":55}"#).unwrap();
    assert!(from_outer_v1.was_migrated());
    assert_eq!(from_outer_v1.source_version, Some(1));
    assert_eq!(from_outer_v1.into_inner(), Mode3SingleTuple(55));

    // Mode 3 single-field tuple struct: outer v2 with inner field v1 -> v2
    let from_inner_v1: Mode3SingleTuple =
        serde_json::from_str(r#"{"version":2,"payload":{"version":1,"data":88}}"#).unwrap();
    assert_eq!(from_inner_v1, Mode3SingleTuple(88));

    // Mode 3 serialization (zero-clone + transparent shadow wire)
    let encoded = serde_json::to_string(&from_inner_v1).unwrap();
    assert_eq!(encoded, r#"{"version":2,"payload":{"version":2,"data":88}}"#);
    assert_eq!(
        serde_json::from_str::<Mode3SingleTuple>(&encoded).unwrap(),
        from_inner_v1
    );
}

// =========================================================================
// 6. Combined outer #[backwards_compat(...)] + field-level #[backwards_compat(...)]
//    on both non-Clone struct and non-Clone enum (zero-clone via serialize_ref)
// =========================================================================

#[derive(Debug, Deserialize)]
struct ServerOuterV1 {
    name: String,
    port: u16,
}

// Intentionally NOT Clone — verifies zero-clone serialization via serialize_ref.
#[backwards_compat(
    tag = "schema",
    versions(
        1: ServerOuterV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
struct ServerOuter {
    name: String,
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    port: u32,
}

impl From<ServerOuterV1> for ServerOuter {
    fn from(v1: ServerOuterV1) -> Self {
        Self {
            name: v1.name,
            port: u32::from(v1.port),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OuterEnumV1 {
    host: String,
}

// Intentionally NOT Clone.
#[backwards_compat(
    content = "body",
    versions(
        1: OuterEnumV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
enum OuterEnum {
    Connect {
        #[backwards_compat(versions(
            1: NestedV1,
            2: Self,
        ))]
        target: Nested,
    },
    Disconnect(
        #[backwards_compat(versions(
            #[with(widen_u16_to_u32)] 1: u16,
            2: Self,
        ))]
        u32,
    ),
}

impl From<OuterEnumV1> for OuterEnum {
    fn from(v1: OuterEnumV1) -> Self {
        Self::Connect {
            target: Nested {
                host: v1.host,
                tls: false,
            },
        }
    }
}

#[test]
fn test_combined_outer_and_field_versioning_non_clone() {
    assert_eq!(ServerOuter::CURRENT_VERSION, 2);

    // Struct: migrate outer v1 -> v2
    let m_outer: Migrated<ServerOuter> =
        serde_json::from_str(r#"{"schema":1,"name":"web","port":80}"#).unwrap();
    assert!(m_outer.was_migrated());
    assert_eq!(m_outer.source_version, Some(1));
    assert_eq!(
        m_outer.into_inner(),
        ServerOuter {
            name: "web".to_owned(),
            port: 80,
        }
    );

    // Struct: outer v2 + inner field v1 -> v2
    let m_inner: Migrated<ServerOuter> =
        serde_json::from_str(r#"{"schema":2,"name":"api","port":{"version":1,"data":8080}}"#).unwrap();
    assert!(!m_inner.was_migrated());
    let server = m_inner.into_inner();
    assert_eq!(
        server,
        ServerOuter {
            name: "api".to_owned(),
            port: 8080,
        }
    );

    // Zero-clone serialization
    let encoded = serde_json::to_string(&server).unwrap();
    assert_eq!(
        encoded,
        r#"{"schema":2,"name":"api","port":{"version":2,"data":8080}}"#
    );
    assert_eq!(serde_json::from_str::<ServerOuter>(&encoded).unwrap(), server);

    // Enum: outer v1 -> v2
    let e_outer: OuterEnum = serde_json::from_str(r#"{"version":1,"body":{"host":"old.local"}}"#).unwrap();
    assert_eq!(
        e_outer,
        OuterEnum::Connect {
            target: Nested {
                host: "old.local".to_owned(),
                tls: false,
            },
        }
    );

    // Enum: outer v2 + inner named field v1 -> v2
    let e_named: OuterEnum = serde_json::from_str(
        r#"{"version":2,"body":{"Connect":{"target":{"version":1,"data":{"host":"inner.local"}}}}}"#,
    )
    .unwrap();
    assert_eq!(
        e_named,
        OuterEnum::Connect {
            target: Nested {
                host: "inner.local".to_owned(),
                tls: false,
            },
        }
    );

    // Enum: outer v2 + inner tuple field v1 -> v2
    let e_tuple: OuterEnum =
        serde_json::from_str(r#"{"version":2,"body":{"Disconnect":{"version":1,"data":500}}}"#).unwrap();
    assert_eq!(e_tuple, OuterEnum::Disconnect(500));

    // Non-Clone enum roundtrip
    for val in [e_named, e_tuple] {
        let json = serde_json::to_string(&val).unwrap();
        let rt: OuterEnum = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, val);
    }
}

// =========================================================================
// 7. Field-level features: custom content="val", tag="v", #[cfg(...)],
//    #[serialize] / #[serialize(with = ...)] (including generic field),
//    | tag aliases, DAG => jumps, and single/multiple _ untagged entries
// =========================================================================

fn dag_jump_v1(x: u8) -> u64 {
    u64::from(x) * 100
}

fn dag_jump_v2(x: u16) -> u64 {
    u64::from(x) * 10
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GenBoxV1<T> {
    inner: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GenBox<T> {
    inner: T,
    extra: u8,
}

impl<T> From<GenBoxV1<T>> for GenBox<T> {
    fn from(v1: GenBoxV1<T>) -> Self {
        Self {
            inner: v1.inner,
            extra: 0,
        }
    }
}

fn downgrade_gen_box<T: Clone>(b: &GenBox<T>) -> GenBoxV1<T> {
    GenBoxV1 {
        inner: b.inner.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct LegacyPair {
    a: u32,
    b: u32,
}

const fn sum_legacy_pair(p: LegacyPair) -> u64 {
    p.a as u64 + p.b as u64
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "#[with] converter takes owned source type"
)]
fn u64_from_str(s: String) -> Result<u64, core::num::ParseIntError> {
    s.parse::<u64>()
}

const fn default_renamed_count() -> u32 {
    99
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FeatureMatrix<T: Clone> {
    // Custom tag + custom content + | aliases + #[cfg] on version entries
    #[backwards_compat(
        tag = "v",
        content = "val",
        versions(
            #[cfg(any())] 0: String,
            #[cfg(test)] #[with(widen_u16_to_u32)] 1 | 10: u16,
            2 | 20: Self,
        )
    )]
    custom_tagged: u32,

    // DAG => jumps
    #[backwards_compat(versions(
        #[with(dag_jump_v1)] 1: u8 => Self,
        #[with(dag_jump_v2)] 2: u16 => Self,
        3: Self,
    ))]
    dag_field: u64,

    // #[serialize(with = ...)] on a generic versioned field
    #[backwards_compat(versions(
        #[serialize(with = downgrade_gen_box::<T>)] 1: GenBoxV1<T>,
        #[current] 2: Self,
    ))]
    #[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
    rolling_generic: GenBox<T>,

    // Single _ untagged entry (e.g. raw unversioned u16)
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] _: u16,
        1: Self,
    ))]
    single_untagged: u32,

    // Multiple _ untagged entries tried in declaration order
    #[backwards_compat(versions(
        #[with(sum_legacy_pair)] _: LegacyPair => Self,
        #[fallible] #[with(u64_from_str)] _: String => Self,
        1: Self,
    ))]
    multi_untagged: u64,

    // Composing with #[serde(default = ...)] and #[serde(rename = "...")]
    #[serde(default = "default_renamed_count", rename = "itemCount")]
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    count: u32,
}

#[test]
fn test_field_level_features_and_serde_composition() {
    // Test aliases (10), DAG jump (1 => Self), rolling generic v2 input,
    // single untagged, multi untagged (LegacyPair), and omitted #[serde(default)] field.
    let input1 = r#"{
        "custom_tagged": {"v": 10, "val": 15},
        "dag_field": {"version": 1, "data": 3},
        "rolling_generic": {"version": 2, "data": {"inner": "hello", "extra": 7}},
        "single_untagged": 42,
        "multi_untagged": {"a": 10, "b": 25}
    }"#;
    let fm: FeatureMatrix<String> = serde_json::from_str(input1).unwrap();
    assert_eq!(fm.custom_tagged, 15);
    assert_eq!(fm.dag_field, 300);
    assert_eq!(
        fm.rolling_generic,
        GenBox {
            inner: "hello".to_owned(),
            extra: 7,
        }
    );
    assert_eq!(fm.single_untagged, 42);
    assert_eq!(fm.multi_untagged, 35);
    assert_eq!(fm.count, 99);

    // Serialization should emit:
    // - custom_tagged with "v": 2, "val": 15
    // - rolling_generic downgraded to version 1 via #[serialize(with = ...)]
    // - count renamed to "itemCount"
    let encoded = serde_json::to_string(&fm).unwrap();
    assert!(encoded.contains(r#""custom_tagged":{"v":2,"val":15}"#));
    assert!(encoded.contains(r#""rolling_generic":{"version":1,"data":{"inner":"hello"}}"#));
    assert!(encoded.contains(r#""itemCount":{"version":2,"data":99}"#));

    // Test second _ untagged arm (String -> u64), DAG jump 2 => Self, and explicit renamed field
    let input2 = r#"{
        "custom_tagged": {"v": 1, "val": 20},
        "dag_field": {"version": 2, "data": 4},
        "rolling_generic": {"version": 1, "data": {"inner": "world"}},
        "single_untagged": {"version": 1, "data": 100},
        "multi_untagged": "12345",
        "itemCount": {"version": 1, "data": 5}
    }"#;
    let fm2: FeatureMatrix<String> = serde_json::from_str(input2).unwrap();
    assert_eq!(fm2.custom_tagged, 20);
    assert_eq!(fm2.dag_field, 40);
    assert_eq!(
        fm2.rolling_generic,
        GenBox {
            inner: "world".to_owned(),
            extra: 0,
        }
    );
    assert_eq!(fm2.single_untagged, 100);
    assert_eq!(fm2.multi_untagged, 12345);
    assert_eq!(fm2.count, 5);

    // Disabled #[cfg(any())] version 0 must be rejected at runtime
    let disabled_v0 = r#"{
        "custom_tagged": {"v": 0, "val": "nope"},
        "dag_field": {"version": 3, "data": 0},
        "rolling_generic": {"version": 1, "data": {"inner": "x"}},
        "single_untagged": 1,
        "multi_untagged": "1"
    }"#;
    let _err = serde_json::from_str::<FeatureMatrix<String>>(disabled_v0).unwrap_err();
}

// Also test bare #[serialize] (using From<&FieldTy> / Into) on a field
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RollTargetV1 {
    x: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RollTarget {
    x: u32,
    y: u32,
}

impl From<RollTargetV1> for RollTarget {
    fn from(v1: RollTargetV1) -> Self {
        Self { x: v1.x, y: 0 }
    }
}

impl From<&RollTarget> for RollTargetV1 {
    fn from(t: &RollTarget) -> Self {
        Self { x: t.x }
    }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BareSerializeOverrideHolder {
    #[backwards_compat(versions(
        #[serialize] 1: RollTargetV1,
        #[current] 2: Self,
    ))]
    target: RollTarget,
}

#[test]
fn test_field_bare_serialize_override() {
    let holder = BareSerializeOverrideHolder {
        target: RollTarget { x: 11, y: 22 },
    };
    let json = serde_json::to_string(&holder).unwrap();
    assert_eq!(json, r#"{"target":{"version":1,"data":{"x":11}}}"#);
    let rt: BareSerializeOverrideHolder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.target, RollTarget { x: 11, y: 0 });
}

// =========================================================================
// 8. Function-local structs + local #[with(local_fn)] inside a fn body
// =========================================================================

#[test]
fn test_function_local_struct_and_local_with_fn() {
    fn double_u16(x: u16) -> u32 {
        u32::from(x) * 2
    }

    #[backwards_compat]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct LocalConfig {
        #[backwards_compat(versions(
            #[with(double_u16)] 1: u16,
            2: Self,
        ))]
        limit: u32,
    }

    let cfg: LocalConfig = serde_json::from_str(r#"{"limit":{"version":1,"data":25}}"#).unwrap();
    assert_eq!(cfg, LocalConfig { limit: 50 });

    let json = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json, r#"{"limit":{"version":2,"data":50}}"#);
    assert_eq!(serde_json::from_str::<LocalConfig>(&json).unwrap(), cfg);
}

// =========================================================================
// 9. Multi-format round-trips: JSON (serde_json), TOML (toml), and RON (ron)
// =========================================================================

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MultiFormatDoc {
    title: String,
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    timeout_ms: u32,
    #[backwards_compat(versions(
        1: NestedV1,
        2: Self,
    ))]
    endpoint: Nested,
}

#[test]
fn test_multi_format_roundtrips_json_toml_ron() {
    let expected = MultiFormatDoc {
        title: "demo".to_owned(),
        timeout_ms: 5000,
        endpoint: Nested {
            host: "db.internal".to_owned(),
            tls: false,
        },
    };

    // --- JSON ---
    let legacy_json = r#"{
        "title": "demo",
        "timeout_ms": {"version": 1, "data": 5000},
        "endpoint": {"version": 1, "data": {"host": "db.internal"}}
    }"#;
    let from_json: MultiFormatDoc = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(from_json, expected);
    let json_out = serde_json::to_string(&from_json).unwrap();
    assert_eq!(
        serde_json::from_str::<MultiFormatDoc>(&json_out).unwrap(),
        expected
    );

    // --- TOML ---
    let legacy_toml = r#"
title = "demo"

[timeout_ms]
version = 1
data = 5000

[endpoint]
version = 1

[endpoint.data]
host = "db.internal"
"#;
    let from_toml: MultiFormatDoc = toml::from_str(legacy_toml).unwrap();
    assert_eq!(from_toml, expected);
    let toml_out = toml::to_string(&from_toml).unwrap();
    let rt_toml: MultiFormatDoc = toml::from_str(&toml_out).unwrap();
    assert_eq!(rt_toml, expected);

    // --- RON ---
    let legacy_ron = r#"MultiFormatDoc(
        title: "demo",
        timeout_ms: (version: 1, data: 5000),
        endpoint: (version: 1, data: (host: "db.internal")),
    )"#;
    let from_ron: MultiFormatDoc = ron::from_str(legacy_ron).unwrap();
    assert_eq!(from_ron, expected);
    let ron_out = ron::to_string(&from_ron).unwrap();
    let rt_ron: MultiFormatDoc = ron::from_str(&ron_out).unwrap();
    assert_eq!(rt_ron, expected);
}

// =========================================================================
// 10. Sibling #[serde(with / serialize_with / deserialize_with)] coexistence
// =========================================================================

mod csv_u32_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(vals: &[u32], serializer: S) -> Result<S::Ok, S::Error> {
        let joined = vals.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
        serializer.serialize_str(&joined)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u32>, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            return Ok(Vec::new());
        }
        s.split(',')
            .map(|part| part.parse::<u32>().map_err(serde::de::Error::custom))
            .collect()
    }
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BareWithSiblingSerde {
    #[serde(with = "csv_u32_serde")]
    tags_with: Vec<u32>,
    #[serde(
        serialize_with = "csv_u32_serde::serialize",
        deserialize_with = "csv_u32_serde::deserialize"
    )]
    tags_split: Vec<u32>,
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    port: u32,
}

#[backwards_compat(tag = "schema", versions(1: Self))]
#[derive(Debug, PartialEq, Eq)]
struct CombinedWithSiblingSerde {
    #[serde(
        serialize_with = "csv_u32_serde::serialize",
        deserialize_with = "csv_u32_serde::deserialize"
    )]
    tags: Vec<u32>,
    #[backwards_compat(versions(
        #[with(widen_u16_to_u32)] 1: u16,
        2: Self,
    ))]
    port: u32,
}

#[test]
fn test_sibling_serde_with_serialize_deserialize_with_coexistence() {
    let bare_json = r#"{"tags_with":"1,2,3","tags_split":"4,5","port":{"version":1,"data":8080}}"#;
    let bare: BareWithSiblingSerde = serde_json::from_str(bare_json).unwrap();
    assert_eq!(
        bare,
        BareWithSiblingSerde {
            tags_with: vec![1, 2, 3],
            tags_split: vec![4, 5],
            port: 8080,
        }
    );
    let bare_out = serde_json::to_string(&bare).unwrap();
    assert_eq!(
        bare_out,
        r#"{"tags_with":"1,2,3","tags_split":"4,5","port":{"version":2,"data":8080}}"#
    );

    let combined_json = r#"{"schema":1,"tags":"7,8","port":{"version":1,"data":443}}"#;
    let combined: CombinedWithSiblingSerde = serde_json::from_str(combined_json).unwrap();
    assert_eq!(
        combined,
        CombinedWithSiblingSerde {
            tags: vec![7, 8],
            port: 443,
        }
    );
    let combined_out = serde_json::to_string(&combined).unwrap();
    assert_eq!(
        combined_out,
        r#"{"schema":1,"tags":"7,8","port":{"version":2,"data":443}}"#
    );
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FieldExternalConfig {
    name: String,
    #[backwards_compat(external, versions(
        #[with(widen_u16_to_u32)] 1 | 10: u16,
        2: Self,
    ))]
    port: u32,
}

#[test]
fn test_field_level_external_tagging() {
    let v1: FieldExternalConfig = serde_json::from_str(r#"{"name":"web","port":{"1":8080}}"#).unwrap();
    assert_eq!(
        v1,
        FieldExternalConfig {
            name: "web".to_owned(),
            port: 8080,
        }
    );

    let alias: FieldExternalConfig = serde_json::from_str(r#"{"name":"web","port":{"10":9090}}"#).unwrap();
    assert_eq!(alias.port, 9090);

    let encoded = serde_json::to_string(&v1).unwrap();
    assert_eq!(encoded, r#"{"name":"web","port":{"2":8080}}"#);
    let decoded: FieldExternalConfig = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, v1);
}
