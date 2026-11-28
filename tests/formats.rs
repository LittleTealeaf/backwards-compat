use backwards_compat::{BackwardsCompatSerialize, Migrated, backwards_compat};
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Internally-tagged multi-version + untagged `_` across CBOR & MessagePack
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordLegacy {
    pub raw_title: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordV1 {
    pub title: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordV2 {
    pub title: String,
    pub revision: u32,
}

impl From<RecordLegacy> for RecordV1 {
    fn from(v: RecordLegacy) -> Self {
        Self { title: v.raw_title }
    }
}

impl From<RecordV1> for RecordV2 {
    fn from(v: RecordV1) -> Self {
        Self {
            title: v.title,
            revision: 1,
        }
    }
}

#[backwards_compat(
    versions(
        _: RecordLegacy => 1,
        #[serializable]
        1: RecordV1,
        #[serializable]
        2: RecordV2,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub title: String,
    pub revision: u32,
    pub archived: bool,
}

impl From<RecordV2> for Record {
    fn from(v: RecordV2) -> Self {
        Self {
            title: v.title,
            revision: v.revision,
            archived: false,
        }
    }
}

impl From<&Record> for RecordV1 {
    fn from(r: &Record) -> Self {
        Self {
            title: r.title.clone(),
        }
    }
}

impl From<&Record> for RecordV2 {
    fn from(r: &Record) -> Self {
        Self {
            title: r.title.clone(),
            revision: r.revision,
        }
    }
}

#[derive(Serialize)]
struct TaggedWireV1<'a> {
    version: u64,
    title: &'a str,
}

#[derive(Serialize)]
struct TaggedWireV2<'a> {
    version: u64,
    title: &'a str,
    revision: u32,
}

fn cbor_encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf).unwrap();
    buf
}

fn cbor_decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
    ciborium::from_reader(bytes).unwrap()
}

#[test]
fn test_cbor_and_msgpack_named_internal_upgrades_and_untagged() {
    let current = Record {
        title: "release-notes".to_owned(),
        revision: 4,
        archived: true,
    };

    // CBOR current round-trip
    let cbor_cur = cbor_encode(&current);
    let dec_cbor_cur: Migrated<Record> = cbor_decode(&cbor_cur);
    assert_eq!(dec_cbor_cur.source_version(), Some(3));
    assert!(
        !dec_cbor_cur.was_migrated(),
        "current CBOR must not report migrated"
    );
    assert_eq!(*dec_cbor_cur, current);

    // MessagePack named current round-trip
    let rmp_cur = rmp_serde::to_vec_named(&current).unwrap();
    let dec_rmp_cur: Migrated<Record> = rmp_serde::from_slice(&rmp_cur).unwrap();
    assert_eq!(dec_rmp_cur.source_version(), Some(3));
    assert!(
        !dec_rmp_cur.was_migrated(),
        "current rmp must not report migrated"
    );
    assert_eq!(*dec_rmp_cur, current);

    // Multi-version upgrades (v1 -> v2 -> v3, v2 -> v3)
    let wire_v1 = TaggedWireV1 {
        version: 1,
        title: "draft",
    };
    let expected_v1 = Record {
        title: "draft".to_owned(),
        revision: 1,
        archived: false,
    };
    let cbor_v1: Migrated<Record> = cbor_decode(&cbor_encode(&wire_v1));
    assert_eq!(cbor_v1.source_version(), Some(1));
    assert!(cbor_v1.was_migrated(), "v1 CBOR must report migrated");
    assert_eq!(*cbor_v1, expected_v1);

    let rmp_v1: Migrated<Record> =
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&wire_v1).unwrap()).unwrap();
    assert_eq!(rmp_v1.source_version(), Some(1));
    assert_eq!(*rmp_v1, expected_v1);

    let wire_v2 = TaggedWireV2 {
        version: 2,
        title: "beta",
        revision: 7,
    };
    let expected_v2 = Record {
        title: "beta".to_owned(),
        revision: 7,
        archived: false,
    };
    let cbor_v2: Migrated<Record> = cbor_decode(&cbor_encode(&wire_v2));
    assert_eq!(cbor_v2.source_version(), Some(2));
    assert_eq!(*cbor_v2, expected_v2);

    let rmp_v2: Migrated<Record> =
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&wire_v2).unwrap()).unwrap();
    assert_eq!(rmp_v2.source_version(), Some(2));
    assert_eq!(*rmp_v2, expected_v2);

    // Untagged `_` fallback in CBOR and named MessagePack
    let legacy = RecordLegacy {
        raw_title: "legacy-doc".to_owned(),
    };
    let expected_legacy = Record {
        title: "legacy-doc".to_owned(),
        revision: 1,
        archived: false,
    };

    let cbor_legacy: Migrated<Record> = cbor_decode(&cbor_encode(&legacy));
    assert!(cbor_legacy.is_untagged(), "expected untagged CBOR payload");
    assert_eq!(cbor_legacy.source_version(), None);
    assert_eq!(*cbor_legacy, expected_legacy);

    let rmp_legacy: Migrated<Record> =
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&legacy).unwrap()).unwrap();
    assert!(rmp_legacy.is_untagged(), "expected untagged MessagePack payload");
    assert_eq!(rmp_legacy.source_version(), None);
    assert_eq!(*rmp_legacy, expected_legacy);

    // BackwardsCompatSerialize::as_version across CBOR & named MessagePack
    let downgraded_cbor = cbor_encode(&current.as_version(2));
    let dec_downgraded_cbor: Migrated<Record> = cbor_decode(&downgraded_cbor);
    assert_eq!(dec_downgraded_cbor.source_version(), Some(2));
    assert_eq!(
        *dec_downgraded_cbor,
        Record {
            title: "release-notes".to_owned(),
            revision: 4,
            archived: false,
        }
    );

    let downgraded_rmp = rmp_serde::to_vec_named(&current.as_version(1)).unwrap();
    let dec_downgraded_rmp: Migrated<Record> = rmp_serde::from_slice(&downgraded_rmp).unwrap();
    assert_eq!(dec_downgraded_rmp.source_version(), Some(1));
    assert_eq!(
        *dec_downgraded_rmp,
        Record {
            title: "release-notes".to_owned(),
            revision: 1,
            archived: false,
        }
    );
}

// =========================================================================
// 2. Adjacently-tagged struct & enum across CBOR, MessagePack named,
//    and MessagePack positional (`rmp_serde::to_vec`)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobEventV1 {
    Started(String),
    Done,
}

#[backwards_compat(
    tag = "v",
    content = "payload",
    versions(
        #[serializable]
        1: JobEventV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum JobEvent {
    Started { job_id: String, attempt: u32 },
    Done,
}

impl From<JobEventV1> for JobEvent {
    fn from(v: JobEventV1) -> Self {
        match v {
            JobEventV1::Started(job_id) => Self::Started { job_id, attempt: 1 },
            JobEventV1::Done => Self::Done,
        }
    }
}

impl From<&JobEvent> for JobEventV1 {
    fn from(e: &JobEvent) -> Self {
        match e {
            JobEvent::Started { job_id, .. } => Self::Started(job_id.clone()),
            JobEvent::Done => Self::Done,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeV1 {
    pub count: u16,
}

#[backwards_compat(
    tag = "schema",
    content = "body",
    versions(
        #[serializable]
        1: EnvelopeV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct Envelope {
    pub count: u32,
    pub tag: String,
}

impl From<EnvelopeV1> for Envelope {
    fn from(v: EnvelopeV1) -> Self {
        Self {
            count: u32::from(v.count),
            tag: "default".to_owned(),
        }
    }
}

impl From<&Envelope> for EnvelopeV1 {
    fn from(e: &Envelope) -> Self {
        Self {
            count: u16::try_from(e.count).unwrap_or(u16::MAX),
        }
    }
}

#[derive(Serialize)]
struct AdjWire<T> {
    v: u64,
    payload: T,
}

#[derive(Serialize)]
struct AdjEnvelopeWire<T> {
    schema: u64,
    body: T,
}

#[test]
fn test_adjacent_tagging_cbor_msgpack_named_and_positional() {
    let ev = JobEvent::Started {
        job_id: "job-99".to_owned(),
        attempt: 3,
    };

    // CBOR round-trip & v1 upgrade
    let cbor_ev = cbor_encode(&ev);
    assert_eq!(cbor_decode::<JobEvent>(&cbor_ev), ev);
    let cbor_ev_v1 = cbor_encode(&AdjWire {
        v: 1,
        payload: JobEventV1::Started("job-1".to_owned()),
    });
    assert_eq!(
        cbor_decode::<JobEvent>(&cbor_ev_v1),
        JobEvent::Started {
            job_id: "job-1".to_owned(),
            attempt: 1,
        }
    );

    // MessagePack named round-trip & v1 upgrade
    let rmp_named_ev = rmp_serde::to_vec_named(&ev).unwrap();
    assert_eq!(rmp_serde::from_slice::<JobEvent>(&rmp_named_ev).unwrap(), ev);
    let rmp_named_ev_v1 = rmp_serde::to_vec_named(&AdjWire {
        v: 1,
        payload: JobEventV1::Done,
    })
    .unwrap();
    assert_eq!(
        rmp_serde::from_slice::<JobEvent>(&rmp_named_ev_v1).unwrap(),
        JobEvent::Done
    );

    // MessagePack positional (`rmp_serde::to_vec`, serialized as 2-element seq)
    let rmp_pos_ev = rmp_serde::to_vec(&ev).unwrap();
    let dec_pos_ev: Migrated<JobEvent> = rmp_serde::from_slice(&rmp_pos_ev).unwrap();
    assert_eq!(dec_pos_ev.source_version(), Some(2));
    assert_eq!(*dec_pos_ev, ev);

    let rmp_pos_ev_v1 = rmp_serde::to_vec(&ev.as_version(1)).unwrap();
    let dec_pos_ev_v1: Migrated<JobEvent> = rmp_serde::from_slice(&rmp_pos_ev_v1).unwrap();
    assert_eq!(dec_pos_ev_v1.source_version(), Some(1));
    assert_eq!(
        *dec_pos_ev_v1,
        JobEvent::Started {
            job_id: "job-99".to_owned(),
            attempt: 1,
        }
    );

    // Adjacently-tagged struct across CBOR, named MessagePack, and positional MessagePack
    let env = Envelope {
        count: 42,
        tag: "prod".to_owned(),
    };
    assert_eq!(cbor_decode::<Envelope>(&cbor_encode(&env)), env);
    assert_eq!(
        rmp_serde::from_slice::<Envelope>(&rmp_serde::to_vec_named(&env).unwrap()).unwrap(),
        env
    );
    assert_eq!(
        rmp_serde::from_slice::<Envelope>(&rmp_serde::to_vec(&env).unwrap()).unwrap(),
        env
    );

    let env_v1_pos = rmp_serde::to_vec(&AdjEnvelopeWire {
        schema: 1,
        body: EnvelopeV1 { count: 7 },
    })
    .unwrap();
    assert_eq!(
        rmp_serde::from_slice::<Envelope>(&env_v1_pos).unwrap(),
        Envelope {
            count: 7,
            tag: "default".to_owned(),
        }
    );
}

// =========================================================================
// 3. Positional MessagePack untagged `_` fallback (sequence wire payload)
// =========================================================================

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqPointV0(pub i32, pub i32);

#[backwards_compat(
    versions(
        _: SeqPointV0 => 1,
        1: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct SeqPoint {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl From<SeqPointV0> for SeqPoint {
    fn from(v: SeqPointV0) -> Self {
        Self { x: v.0, y: v.1, z: 0 }
    }
}

#[test]
fn test_msgpack_positional_untagged_fallback() {
    let legacy_bytes = rmp_serde::to_vec(&SeqPointV0(10, 20)).unwrap();
    let migrated: Migrated<SeqPoint> = rmp_serde::from_slice(&legacy_bytes).unwrap();
    assert!(
        migrated.is_untagged(),
        "positional legacy tuple should decode as untagged"
    );
    assert_eq!(migrated.source_version(), None);
    assert_eq!(*migrated, SeqPoint { x: 10, y: 20, z: 0 });
}

// =========================================================================
// 4. Shared models for positional binary and self-describing format matrix
// =========================================================================

// 4a. Adjacently-tagged with string tags (struct & enum)
#[backwards_compat(
    tag = "v",
    content = "body",
    versions(
        #[serializable]
        "v1": EnvelopeV1,
        "v2": Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjStrEnvelope {
    pub count: u32,
    pub tag: String,
}

impl From<EnvelopeV1> for AdjStrEnvelope {
    fn from(v: EnvelopeV1) -> Self {
        Self {
            count: u32::from(v.count),
            tag: "default".to_owned(),
        }
    }
}

impl From<&AdjStrEnvelope> for EnvelopeV1 {
    fn from(e: &AdjStrEnvelope) -> Self {
        Self {
            count: u16::try_from(e.count).unwrap_or(u16::MAX),
        }
    }
}

#[backwards_compat(
    tag = "v",
    content = "payload",
    versions(
        #[serializable]
        "v1": JobEventV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum AdjStrJobEvent {
    Started { job_id: String, attempt: u32 },
    Done,
}

impl From<JobEventV1> for AdjStrJobEvent {
    fn from(v: JobEventV1) -> Self {
        match v {
            JobEventV1::Started(job_id) => Self::Started { job_id, attempt: 1 },
            JobEventV1::Done => Self::Done,
        }
    }
}

impl From<&AdjStrJobEvent> for JobEventV1 {
    fn from(e: &AdjStrJobEvent) -> Self {
        match e {
            AdjStrJobEvent::Started { job_id, .. } => Self::Started(job_id.clone()),
            AdjStrJobEvent::Done => Self::Done,
        }
    }
}

// 4b. Field-level versioning (adjacent default + external) for positional binary
fn widen_port(p: u16) -> u32 {
    u32::from(p)
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldAdjContainer {
    pub label: String,
    #[backwards_compat(versions(
        #[migrate(with = widen_port)] 1: u16,
        2: Self,
    ))]
    pub port: u32,
    #[backwards_compat(versions(
        "v1": EnvelopeV1,
        "v2": Self,
    ))]
    pub env: AdjStrEnvelope,
}

#[backwards_compat]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldExtContainer {
    pub label: String,
    #[backwards_compat(external, versions(
        #[migrate(with = widen_port)] 1: u16,
        2: Self,
    ))]
    pub port: u32,
    #[backwards_compat(external, versions(
        "v1": EnvelopeV1,
        "v2": Self,
    ))]
    pub env: AdjStrEnvelope,
}

// 4c. Externally-tagged struct & enum (int tags + aliases, str tags + aliases)
#[backwards_compat(
    external,
    versions(
        #[serializable]
        1 | 10: EnvelopeV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ExtIntEnvelope {
    pub count: u32,
    pub tag: String,
}

impl From<EnvelopeV1> for ExtIntEnvelope {
    fn from(v: EnvelopeV1) -> Self {
        Self {
            count: u32::from(v.count),
            tag: "default".to_owned(),
        }
    }
}

impl From<&ExtIntEnvelope> for EnvelopeV1 {
    fn from(e: &ExtIntEnvelope) -> Self {
        Self {
            count: u16::try_from(e.count).unwrap_or(u16::MAX),
        }
    }
}

#[backwards_compat(
    external,
    versions(
        #[serializable]
        "v1" | "legacy_v1": EnvelopeV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct ExtStrEnvelope {
    pub count: u32,
    pub tag: String,
}

impl From<EnvelopeV1> for ExtStrEnvelope {
    fn from(v: EnvelopeV1) -> Self {
        Self {
            count: u32::from(v.count),
            tag: "default".to_owned(),
        }
    }
}

impl From<&ExtStrEnvelope> for EnvelopeV1 {
    fn from(e: &ExtStrEnvelope) -> Self {
        Self {
            count: u16::try_from(e.count).unwrap_or(u16::MAX),
        }
    }
}

#[backwards_compat(
    external,
    versions(
        #[serializable]
        1 | 10: JobEventV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum ExtIntJobEvent {
    Started { job_id: String, attempt: u32 },
    Done,
}

impl From<JobEventV1> for ExtIntJobEvent {
    fn from(v: JobEventV1) -> Self {
        match v {
            JobEventV1::Started(job_id) => Self::Started { job_id, attempt: 1 },
            JobEventV1::Done => Self::Done,
        }
    }
}

impl From<&ExtIntJobEvent> for JobEventV1 {
    fn from(e: &ExtIntJobEvent) -> Self {
        match e {
            ExtIntJobEvent::Started { job_id, .. } => Self::Started(job_id.clone()),
            ExtIntJobEvent::Done => Self::Done,
        }
    }
}

#[backwards_compat(
    external,
    versions(
        #[serializable]
        "v1" | "legacy_v1": JobEventV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub enum ExtStrJobEvent {
    Started { job_id: String, attempt: u32 },
    Done,
}

impl From<JobEventV1> for ExtStrJobEvent {
    fn from(v: JobEventV1) -> Self {
        match v {
            JobEventV1::Started(job_id) => Self::Started { job_id, attempt: 1 },
            JobEventV1::Done => Self::Done,
        }
    }
}

impl From<&ExtStrJobEvent> for JobEventV1 {
    fn from(e: &ExtStrJobEvent) -> Self {
        match e {
            ExtStrJobEvent::Started { job_id, .. } => Self::Started(job_id.clone()),
            ExtStrJobEvent::Done => Self::Done,
        }
    }
}

// 4d. `via` proxy over external type
#[backwards_compat(via = ExtIntEnvelope)]
#[derive(Debug, PartialEq, Eq)]
pub struct ViaExtEnvelope {
    pub count: u32,
    pub tag: String,
}

impl From<ExtIntEnvelope> for ViaExtEnvelope {
    fn from(p: ExtIntEnvelope) -> Self {
        Self {
            count: p.count,
            tag: p.tag,
        }
    }
}

impl From<&ViaExtEnvelope> for ExtIntEnvelope {
    fn from(d: &ViaExtEnvelope) -> Self {
        Self {
            count: d.count,
            tag: d.tag.clone(),
        }
    }
}

// 4e. Flat internally-tagged structs (both int & str tags) for `serde_urlencoded`, `serde_qs`, etc.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatConfigV1 {
    pub host: String,
}

#[backwards_compat(
    versions(
        #[serializable]
        1: FlatConfigV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct FlatIntConfig {
    pub host: String,
    pub port: u16,
}

impl From<FlatConfigV1> for FlatIntConfig {
    fn from(v: FlatConfigV1) -> Self {
        Self {
            host: v.host,
            port: 8080,
        }
    }
}

impl From<&FlatIntConfig> for FlatConfigV1 {
    fn from(c: &FlatIntConfig) -> Self {
        Self { host: c.host.clone() }
    }
}

#[backwards_compat(
    versions(
        #[serializable]
        "v1": FlatConfigV1,
        "v2": Self,
    )
)]
#[derive(Debug, PartialEq, Eq)]
pub struct FlatStrConfig {
    pub host: String,
    pub port: u16,
}

impl From<FlatConfigV1> for FlatStrConfig {
    fn from(v: FlatConfigV1) -> Self {
        Self {
            host: v.host,
            port: 8080,
        }
    }
}

impl From<&FlatStrConfig> for FlatConfigV1 {
    fn from(c: &FlatStrConfig) -> Self {
        Self { host: c.host.clone() }
    }
}

// =========================================================================
// 5. Positional Binary Formats: bincode 2.x (standard & legacy), postcard,
//    bitcode, and rmp-serde positional
// =========================================================================

trait PositionalCodec {
    fn name() -> &'static str;
    fn encode<T: Serialize>(value: &T) -> Vec<u8>;
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T;
}

struct BincodeStandard;
impl PositionalCodec for BincodeStandard {
    fn name() -> &'static str {
        "bincode_standard"
    }
    fn encode<T: Serialize>(value: &T) -> Vec<u8> {
        bincode::serde::encode_to_vec(value, bincode::config::standard()).unwrap()
    }
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .unwrap()
            .0
    }
}

struct BincodeLegacy;
impl PositionalCodec for BincodeLegacy {
    fn name() -> &'static str {
        "bincode_legacy"
    }
    fn encode<T: Serialize>(value: &T) -> Vec<u8> {
        bincode::serde::encode_to_vec(value, bincode::config::legacy()).unwrap()
    }
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
        bincode::serde::decode_from_slice(bytes, bincode::config::legacy())
            .unwrap()
            .0
    }
}

struct PostcardCodec;
impl PositionalCodec for PostcardCodec {
    fn name() -> &'static str {
        "postcard"
    }
    fn encode<T: Serialize>(value: &T) -> Vec<u8> {
        postcard::to_allocvec(value).unwrap()
    }
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
        postcard::from_bytes(bytes).unwrap()
    }
}

struct BitcodeCodec;
impl PositionalCodec for BitcodeCodec {
    fn name() -> &'static str {
        "bitcode"
    }
    fn encode<T: Serialize>(value: &T) -> Vec<u8> {
        bitcode::serialize(value).unwrap()
    }
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
        bitcode::deserialize(bytes).unwrap()
    }
}

struct RmpPositionalCodec;
impl PositionalCodec for RmpPositionalCodec {
    fn name() -> &'static str {
        "rmp_positional"
    }
    fn encode<T: Serialize>(value: &T) -> Vec<u8> {
        rmp_serde::to_vec(value).unwrap()
    }
    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
        rmp_serde::from_slice(bytes).unwrap()
    }
}

fn run_positional_suite<C: PositionalCodec>() {
    let name = C::name();

    // Adjacent int struct
    let env = Envelope {
        count: 42,
        tag: "prod".to_owned(),
    };
    let dec: Migrated<Envelope> = C::decode(&C::encode(&env));
    assert_eq!(dec.source_version(), Some(2), "{name}: adj int struct ver");
    assert!(!dec.was_migrated(), "{name}: adj int struct migrated");
    assert_eq!(*dec, env, "{name}: adj int struct val");
    let dec_v1: Migrated<Envelope> = C::decode(&C::encode(&env.as_version(1)));
    assert_eq!(dec_v1.source_version(), Some(1), "{name}: adj int v1 ver");
    assert!(dec_v1.was_migrated(), "{name}: adj int v1 migrated");
    assert_eq!(
        *dec_v1,
        Envelope {
            count: 42,
            tag: "default".to_owned(),
        },
        "{name}: adj int v1 val"
    );

    // Adjacent str struct
    let str_env = AdjStrEnvelope {
        count: 99,
        tag: "west".to_owned(),
    };
    let dec_str_env: Migrated<AdjStrEnvelope> = C::decode(&C::encode(&str_env));
    assert_eq!(
        dec_str_env.source_version(),
        Some("v2"),
        "{name}: adj str struct ver"
    );
    assert_eq!(*dec_str_env, str_env, "{name}: adj str struct val");
    let dec_str_env_v1: Migrated<AdjStrEnvelope> = C::decode(&C::encode(&str_env.as_version("v1")));
    assert_eq!(
        dec_str_env_v1.source_version(),
        Some("v1"),
        "{name}: adj str v1 ver"
    );
    assert_eq!(
        *dec_str_env_v1,
        AdjStrEnvelope {
            count: 99,
            tag: "default".to_owned(),
        },
        "{name}: adj str v1 val"
    );

    // Adjacent int enum & str enum
    let job = JobEvent::Started {
        job_id: "j-7".to_owned(),
        attempt: 3,
    };
    let dec_job: Migrated<JobEvent> = C::decode(&C::encode(&job));
    assert_eq!(dec_job.source_version(), Some(2), "{name}: adj int enum ver");
    assert_eq!(*dec_job, job, "{name}: adj int enum val");
    let dec_job_v1: Migrated<JobEvent> = C::decode(&C::encode(&job.as_version(1)));
    assert_eq!(
        dec_job_v1.source_version(),
        Some(1),
        "{name}: adj int enum v1 ver"
    );
    assert_eq!(
        *dec_job_v1,
        JobEvent::Started {
            job_id: "j-7".to_owned(),
            attempt: 1,
        },
        "{name}: adj int enum v1 val"
    );

    let str_job = AdjStrJobEvent::Started {
        job_id: "j-9".to_owned(),
        attempt: 5,
    };
    let dec_str_job: Migrated<AdjStrJobEvent> = C::decode(&C::encode(&str_job));
    assert_eq!(
        dec_str_job.source_version(),
        Some("v2"),
        "{name}: adj str enum ver"
    );
    assert_eq!(*dec_str_job, str_job, "{name}: adj str enum val");
    let dec_str_job_v1: Migrated<AdjStrJobEvent> = C::decode(&C::encode(&str_job.as_version("v1")));
    assert_eq!(
        dec_str_job_v1.source_version(),
        Some("v1"),
        "{name}: adj str enum v1 ver"
    );
    assert_eq!(
        *dec_str_job_v1,
        AdjStrJobEvent::Started {
            job_id: "j-9".to_owned(),
            attempt: 1,
        },
        "{name}: adj str enum v1 val"
    );

    // Field-level adjacent & external
    let field_adj = FieldAdjContainer {
        label: "svc".to_owned(),
        port: 8443,
        env: str_env.clone(),
    };
    assert_eq!(
        C::decode::<FieldAdjContainer>(&C::encode(&field_adj)),
        field_adj,
        "{name}: field adj"
    );

    let field_ext = FieldExtContainer {
        label: "svc".to_owned(),
        port: 8443,
        env: str_env,
    };
    assert_eq!(
        C::decode::<FieldExtContainer>(&C::encode(&field_ext)),
        field_ext,
        "{name}: field ext"
    );

    // External int struct
    let ext_int = ExtIntEnvelope {
        count: 55,
        tag: "east".to_owned(),
    };
    let dec_ext_int: Migrated<ExtIntEnvelope> = C::decode(&C::encode(&ext_int));
    assert_eq!(
        dec_ext_int.source_version(),
        Some(2),
        "{name}: ext int struct ver"
    );
    assert_eq!(*dec_ext_int, ext_int, "{name}: ext int struct val");
    let dec_ext_int_v1: Migrated<ExtIntEnvelope> = C::decode(&C::encode(&ext_int.as_version(1)));
    assert_eq!(dec_ext_int_v1.source_version(), Some(1), "{name}: ext int v1 ver");
    assert_eq!(
        *dec_ext_int_v1,
        ExtIntEnvelope {
            count: 55,
            tag: "default".to_owned(),
        },
        "{name}: ext int v1 val"
    );

    // External str struct
    let ext_str = ExtStrEnvelope {
        count: 77,
        tag: "north".to_owned(),
    };
    let dec_ext_str: Migrated<ExtStrEnvelope> = C::decode(&C::encode(&ext_str));
    assert_eq!(
        dec_ext_str.source_version(),
        Some("v2"),
        "{name}: ext str struct ver"
    );
    assert_eq!(*dec_ext_str, ext_str, "{name}: ext str struct val");
    let dec_ext_str_v1: Migrated<ExtStrEnvelope> = C::decode(&C::encode(&ext_str.as_version("v1")));
    assert_eq!(
        dec_ext_str_v1.source_version(),
        Some("v1"),
        "{name}: ext str v1 ver"
    );
    assert_eq!(
        *dec_ext_str_v1,
        ExtStrEnvelope {
            count: 77,
            tag: "default".to_owned(),
        },
        "{name}: ext str v1 val"
    );

    // External int enum & str enum
    let ext_int_job = ExtIntJobEvent::Started {
        job_id: "ej-1".to_owned(),
        attempt: 4,
    };
    let dec_ext_int_job: Migrated<ExtIntJobEvent> = C::decode(&C::encode(&ext_int_job));
    assert_eq!(
        dec_ext_int_job.source_version(),
        Some(2),
        "{name}: ext int enum ver"
    );
    assert_eq!(*dec_ext_int_job, ext_int_job, "{name}: ext int enum val");
    let dec_ext_int_job_v1: Migrated<ExtIntJobEvent> = C::decode(&C::encode(&ext_int_job.as_version(1)));
    assert_eq!(
        dec_ext_int_job_v1.source_version(),
        Some(1),
        "{name}: ext int enum v1 ver"
    );
    assert_eq!(
        *dec_ext_int_job_v1,
        ExtIntJobEvent::Started {
            job_id: "ej-1".to_owned(),
            attempt: 1,
        },
        "{name}: ext int enum v1 val"
    );

    let ext_str_job = ExtStrJobEvent::Started {
        job_id: "ej-2".to_owned(),
        attempt: 6,
    };
    let dec_ext_str_job: Migrated<ExtStrJobEvent> = C::decode(&C::encode(&ext_str_job));
    assert_eq!(
        dec_ext_str_job.source_version(),
        Some("v2"),
        "{name}: ext str enum ver"
    );
    assert_eq!(*dec_ext_str_job, ext_str_job, "{name}: ext str enum val");
    let dec_ext_str_job_v1: Migrated<ExtStrJobEvent> = C::decode(&C::encode(&ext_str_job.as_version("v1")));
    assert_eq!(
        dec_ext_str_job_v1.source_version(),
        Some("v1"),
        "{name}: ext str enum v1 ver"
    );
    assert_eq!(
        *dec_ext_str_job_v1,
        ExtStrJobEvent::Started {
            job_id: "ej-2".to_owned(),
            attempt: 1,
        },
        "{name}: ext str enum v1 val"
    );

    // Via proxy over external
    let via_ext = ViaExtEnvelope {
        count: 11,
        tag: "proxy".to_owned(),
    };
    let dec_via: Migrated<ViaExtEnvelope> = C::decode(&C::encode(&via_ext));
    assert_eq!(dec_via.source_version(), Some(2), "{name}: via ext ver");
    assert_eq!(*dec_via, via_ext, "{name}: via ext val");
    let dec_via_v1: Migrated<ViaExtEnvelope> = C::decode(&C::encode(&via_ext.as_version(1)));
    assert_eq!(dec_via_v1.source_version(), Some(1), "{name}: via ext v1 ver");
    assert_eq!(
        *dec_via_v1,
        ViaExtEnvelope {
            count: 11,
            tag: "default".to_owned(),
        },
        "{name}: via ext v1 val"
    );
}

#[test]
fn test_positional_binary_formats_bincode_postcard_bitcode_msgpack() {
    run_positional_suite::<BincodeStandard>();
    run_positional_suite::<BincodeLegacy>();
    run_positional_suite::<PostcardCodec>();
    run_positional_suite::<BitcodeCodec>();
    run_positional_suite::<RmpPositionalCodec>();
}

// =========================================================================
// 6. Self-describing formats: JSON, TOML, RON, YAML, BSON, JSON5,
//    FlexBuffers, quick-xml, serde_urlencoded, serde_qs
// =========================================================================

#[test]
fn test_self_describing_yaml_json5_flexbuffers_bson() {
    let rec = Record {
        title: "notes".to_owned(),
        revision: 3,
        archived: true,
    };
    let adj = AdjStrEnvelope {
        count: 12,
        tag: "edge".to_owned(),
    };
    let ext_int = ExtIntEnvelope {
        count: 34,
        tag: "core".to_owned(),
    };
    let ext_str = ExtStrEnvelope {
        count: 56,
        tag: "leaf".to_owned(),
    };
    let flat_int = FlatIntConfig {
        host: "db.local".to_owned(),
        port: 5432,
    };
    let flat_str = FlatStrConfig {
        host: "cache.local".to_owned(),
        port: 6379,
    };

    // serde_yaml_ng (internal, adjacent, external int & str, aliases)
    let yaml_rec = serde_yaml_ng::to_string(&rec).unwrap();
    assert_eq!(serde_yaml_ng::from_str::<Record>(&yaml_rec).unwrap(), rec);
    let yaml_rec_v1 = serde_yaml_ng::to_string(&rec.as_version(1)).unwrap();
    let dec_yaml_v1: Migrated<Record> = serde_yaml_ng::from_str(&yaml_rec_v1).unwrap();
    assert_eq!(dec_yaml_v1.source_version(), Some(1));

    let yaml_adj = serde_yaml_ng::to_string(&adj).unwrap();
    assert_eq!(serde_yaml_ng::from_str::<AdjStrEnvelope>(&yaml_adj).unwrap(), adj);
    let yaml_ext_i = serde_yaml_ng::to_string(&ext_int).unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<ExtIntEnvelope>(&yaml_ext_i).unwrap(),
        ext_int
    );
    let yaml_ext_s = serde_yaml_ng::to_string(&ext_str).unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<ExtStrEnvelope>(&yaml_ext_s).unwrap(),
        ext_str
    );
    let yaml_alias: Migrated<ExtStrEnvelope> = serde_yaml_ng::from_str("!legacy_v1\ncount: 8\n").unwrap();
    assert_eq!(yaml_alias.source_version(), Some("legacy_v1"));
    assert_eq!(
        *yaml_alias,
        ExtStrEnvelope {
            count: 8,
            tag: "default".to_owned(),
        }
    );

    // json5 (round-trip + JSON5 comments / trailing commas)
    let j5_rec = json5::to_string(&rec).unwrap();
    assert_eq!(json5::from_str::<Record>(&j5_rec).unwrap(), rec);
    let j5_adj = json5::to_string(&adj).unwrap();
    assert_eq!(json5::from_str::<AdjStrEnvelope>(&j5_adj).unwrap(), adj);
    let j5_ext = json5::to_string(&ext_str).unwrap();
    assert_eq!(json5::from_str::<ExtStrEnvelope>(&j5_ext).unwrap(), ext_str);

    let j5_commented = r"{
        // legacy v1 payload in JSON5
        version: 1,
        title: 'json5-doc',
    }";
    let dec_j5_v1: Migrated<Record> = json5::from_str(j5_commented).unwrap();
    assert_eq!(dec_j5_v1.source_version(), Some(1));
    assert_eq!(dec_j5_v1.title, "json5-doc");

    // flexbuffers
    let fb_rec = flexbuffers::to_vec(&rec).unwrap();
    assert_eq!(flexbuffers::from_slice::<Record>(&fb_rec).unwrap(), rec);
    let fb_adj = flexbuffers::to_vec(&adj).unwrap();
    assert_eq!(flexbuffers::from_slice::<AdjStrEnvelope>(&fb_adj).unwrap(), adj);
    let fb_ext_i = flexbuffers::to_vec(&ext_int).unwrap();
    assert_eq!(
        flexbuffers::from_slice::<ExtIntEnvelope>(&fb_ext_i).unwrap(),
        ext_int
    );
    let fb_ext_s = flexbuffers::to_vec(&ext_str).unwrap();
    assert_eq!(
        flexbuffers::from_slice::<ExtStrEnvelope>(&fb_ext_s).unwrap(),
        ext_str
    );

    // bson (root document structs)
    let bson_rec = bson::serialize_to_vec(&rec).unwrap();
    assert_eq!(bson::deserialize_from_slice::<Record>(&bson_rec).unwrap(), rec);
    let bson_rec_v1 = bson::serialize_to_vec(&rec.as_version(1)).unwrap();
    let dec_bson_v1: Migrated<Record> = bson::deserialize_from_slice(&bson_rec_v1).unwrap();
    assert_eq!(dec_bson_v1.source_version(), Some(1));

    let bson_adj = bson::serialize_to_vec(&adj).unwrap();
    assert_eq!(
        bson::deserialize_from_slice::<AdjStrEnvelope>(&bson_adj).unwrap(),
        adj
    );
    let bson_ext_i = bson::serialize_to_vec(&ext_int).unwrap();
    assert_eq!(
        bson::deserialize_from_slice::<ExtIntEnvelope>(&bson_ext_i).unwrap(),
        ext_int
    );
    let bson_ext_s = bson::serialize_to_vec(&ext_str).unwrap();
    assert_eq!(
        bson::deserialize_from_slice::<ExtStrEnvelope>(&bson_ext_s).unwrap(),
        ext_str
    );
    let bson_flat_i = bson::serialize_to_vec(&flat_int).unwrap();
    assert_eq!(
        bson::deserialize_from_slice::<FlatIntConfig>(&bson_flat_i).unwrap(),
        flat_int
    );
    let bson_flat_s = bson::serialize_to_vec(&flat_str).unwrap();
    assert_eq!(
        bson::deserialize_from_slice::<FlatStrConfig>(&bson_flat_s).unwrap(),
        flat_str
    );
}

#[test]
fn test_self_describing_toml_ron_json_and_external_aliases() {
    let ext_str = ExtStrEnvelope {
        count: 88,
        tag: "alpha".to_owned(),
    };
    let ext_int = ExtIntEnvelope {
        count: 21,
        tag: "beta".to_owned(),
    };

    // JSON (internal, adjacent, external int & str, aliases)
    let j_ext_i = serde_json::to_string(&ext_int).unwrap();
    assert_eq!(j_ext_i, r#"{"2":{"count":21,"tag":"beta"}}"#);
    assert_eq!(serde_json::from_str::<ExtIntEnvelope>(&j_ext_i).unwrap(), ext_int);
    let j_ext_alias: Migrated<ExtIntEnvelope> = serde_json::from_str(r#"{"10":{"count":9}}"#).unwrap();
    assert_eq!(j_ext_alias.source_version(), Some(10));
    assert_eq!(
        *j_ext_alias,
        ExtIntEnvelope {
            count: 9,
            tag: "default".to_owned(),
        }
    );

    // TOML (external str & int, adjacent, internal)
    let toml_ext_s = toml::to_string(&ext_str).unwrap();
    assert_eq!(toml::from_str::<ExtStrEnvelope>(&toml_ext_s).unwrap(), ext_str);
    let toml_ext_i = toml::to_string(&ext_int).unwrap();
    assert_eq!(toml::from_str::<ExtIntEnvelope>(&toml_ext_i).unwrap(), ext_int);

    // RON (external str & int, adjacent, internal)
    let ron_ext_s = ron::to_string(&ext_str).unwrap();
    assert_eq!(ron::from_str::<ExtStrEnvelope>(&ron_ext_s).unwrap(), ext_str);
    let ron_ext_i = ron::to_string(&ext_int).unwrap();
    assert_eq!(ron::from_str::<ExtIntEnvelope>(&ron_ext_i).unwrap(), ext_int);
}

#[test]
fn test_string_valued_formats_xml_urlencoded_and_qs() {
    // 1. serde_urlencoded: flat internally-tagged structs with BOTH integer and string tags
    //    (unlocked by direct typed u64 / StrTag deserialization without deserialize_any)
    let flat_int = FlatIntConfig {
        host: "api.example.com".to_owned(),
        port: 8443,
    };
    let url_int = serde_urlencoded::to_string(&flat_int).unwrap();
    assert_eq!(
        serde_urlencoded::from_str::<FlatIntConfig>(&url_int).unwrap(),
        flat_int
    );
    let url_int_v1 = serde_urlencoded::to_string(flat_int.as_version(1)).unwrap();
    let dec_url_int_v1: Migrated<FlatIntConfig> = serde_urlencoded::from_str(&url_int_v1).unwrap();
    assert_eq!(dec_url_int_v1.source_version(), Some(1));
    assert_eq!(
        *dec_url_int_v1,
        FlatIntConfig {
            host: "api.example.com".to_owned(),
            port: 8080,
        }
    );

    let flat_str = FlatStrConfig {
        host: "auth.example.com".to_owned(),
        port: 9000,
    };
    let url_str = serde_urlencoded::to_string(&flat_str).unwrap();
    assert_eq!(
        serde_urlencoded::from_str::<FlatStrConfig>(&url_str).unwrap(),
        flat_str
    );
    let url_str_v1 = serde_urlencoded::to_string(flat_str.as_version("v1")).unwrap();
    let dec_url_str_v1: Migrated<FlatStrConfig> = serde_urlencoded::from_str(&url_str_v1).unwrap();
    assert_eq!(dec_url_str_v1.source_version(), Some("v1"));
    assert_eq!(
        *dec_url_str_v1,
        FlatStrConfig {
            host: "auth.example.com".to_owned(),
            port: 8080,
        }
    );

    // 2. serde_qs: adjacent and external (int & str tags; serde_qs 0.15 requires struct/enum at top level)
    let adj_int = Envelope {
        count: 15,
        tag: "qs".to_owned(),
    };
    let qs_adj_int = serde_qs::to_string(&adj_int).unwrap();
    assert_eq!(serde_qs::from_str::<Envelope>(&qs_adj_int).unwrap(), adj_int);

    let adj_str = AdjStrEnvelope {
        count: 16,
        tag: "qs".to_owned(),
    };
    let qs_adj_str = serde_qs::to_string(&adj_str).unwrap();
    assert_eq!(
        serde_qs::from_str::<AdjStrEnvelope>(&qs_adj_str).unwrap(),
        adj_str
    );

    let ext_str = ExtStrEnvelope {
        count: 25,
        tag: "qs".to_owned(),
    };
    let qs_ext_str = serde_qs::to_string(&ext_str).unwrap();
    assert_eq!(
        serde_qs::from_str::<ExtStrEnvelope>(&qs_ext_str).unwrap(),
        ext_str
    );
    let dec_qs_ext_alias: Migrated<ExtStrEnvelope> = serde_qs::from_str("legacy_v1[count]=9").unwrap();
    assert_eq!(dec_qs_ext_alias.source_version(), Some("legacy_v1"));
    assert_eq!(
        *dec_qs_ext_alias,
        ExtStrEnvelope {
            count: 9,
            tag: "default".to_owned(),
        }
    );

    // 3. quick-xml: internal (int & str tags), adjacent (int & str tags), external (XML-valid str tags)
    let xml_flat_int = quick_xml::se::to_string(&flat_int).unwrap();
    assert_eq!(
        quick_xml::de::from_str::<FlatIntConfig>(&xml_flat_int).unwrap(),
        flat_int
    );
    let xml_flat_int_v1 = quick_xml::se::to_string(&flat_int.as_version(1)).unwrap();
    let dec_xml_flat_v1: Migrated<FlatIntConfig> = quick_xml::de::from_str(&xml_flat_int_v1).unwrap();
    assert_eq!(dec_xml_flat_v1.source_version(), Some(1));

    let xml_flat_str = quick_xml::se::to_string(&flat_str).unwrap();
    assert_eq!(
        quick_xml::de::from_str::<FlatStrConfig>(&xml_flat_str).unwrap(),
        flat_str
    );

    let xml_adj_int = quick_xml::se::to_string(&adj_int).unwrap();
    assert_eq!(
        quick_xml::de::from_str::<Envelope>(&xml_adj_int).unwrap(),
        adj_int
    );
    let xml_adj_str = quick_xml::se::to_string(&adj_str).unwrap();
    assert_eq!(
        quick_xml::de::from_str::<AdjStrEnvelope>(&xml_adj_str).unwrap(),
        adj_str
    );

    let xml_ext_str = quick_xml::se::to_string(&ext_str).unwrap();
    assert_eq!(
        quick_xml::de::from_str::<ExtStrEnvelope>(&xml_ext_str).unwrap(),
        ext_str
    );
    let xml_ext_v1 = quick_xml::se::to_string(&ext_str.as_version("v1")).unwrap();
    let dec_xml_ext_v1: Migrated<ExtStrEnvelope> = quick_xml::de::from_str(&xml_ext_v1).unwrap();
    assert_eq!(dec_xml_ext_v1.source_version(), Some("v1"));
    assert_eq!(
        *dec_xml_ext_v1,
        ExtStrEnvelope {
            count: 25,
            tag: "default".to_owned(),
        }
    );
}
