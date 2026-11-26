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
    assert!(!dec_cbor_cur.was_migrated());
    assert_eq!(*dec_cbor_cur, current);

    // MessagePack named current round-trip
    let rmp_cur = rmp_serde::to_vec_named(&current).unwrap();
    let dec_rmp_cur: Migrated<Record> = rmp_serde::from_slice(&rmp_cur).unwrap();
    assert_eq!(dec_rmp_cur.source_version(), Some(3));
    assert!(!dec_rmp_cur.was_migrated());
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
    assert!(cbor_v1.was_migrated());
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
