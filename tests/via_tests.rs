use backwards_compat::{BackwardsCompat, Migrated, VersionKey, backwards_compat, backwards_compat_via};
use serde::{Deserialize, Serialize};

// =========================================================================
// 1. Named struct, tuple struct, unit struct, enum + JSON / RON / TOML
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct AppConfigV1 {
    pub name: String,
}

#[backwards_compat(tag = "version", versions(1: AppConfigV1, 2: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfigProxy {
    pub name: String,
    pub port: u16,
}

impl From<AppConfigV1> for AppConfigProxy {
    fn from(v1: AppConfigV1) -> Self {
        Self {
            name: v1.name,
            port: 8080,
        }
    }
}

#[backwards_compat_via(AppConfigProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub name: String,
    pub port: u16,
}

impl From<AppConfigProxy> for AppConfig {
    fn from(p: AppConfigProxy) -> Self {
        Self {
            name: p.name,
            port: p.port,
        }
    }
}

impl From<&AppConfig> for AppConfigProxy {
    fn from(c: &AppConfig) -> Self {
        Self {
            name: c.name.clone(),
            port: c.port,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct UserIdV1(pub u32);

#[backwards_compat(tag = "v", content = "data", versions(1: UserIdV1, 2: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIdProxy(pub u64);

impl From<UserIdV1> for UserIdProxy {
    fn from(v1: UserIdV1) -> Self {
        Self(u64::from(v1.0))
    }
}

#[backwards_compat_via(UserIdProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct UserId(pub u64);

impl From<UserIdProxy> for UserId {
    fn from(p: UserIdProxy) -> Self {
        Self(p.0)
    }
}

impl From<&UserId> for UserIdProxy {
    fn from(id: &UserId) -> Self {
        Self(id.0)
    }
}

#[backwards_compat(tag = "version", content = "data", versions(1: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerProxy;

#[backwards_compat_via(MarkerProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct Marker;

impl From<MarkerProxy> for Marker {
    fn from(MarkerProxy: MarkerProxy) -> Self {
        Self
    }
}

impl From<&Marker> for MarkerProxy {
    fn from(Marker: &Marker) -> Self {
        Self
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct EventV1 {
    pub text: String,
}

#[backwards_compat(tag = "v", content = "payload", versions(1: EventV1, 2: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventProxy {
    Ping,
    Text(String),
    Metric { key: String, value: i64 },
}

impl From<EventV1> for EventProxy {
    fn from(v1: EventV1) -> Self {
        Self::Text(v1.text)
    }
}

#[backwards_compat_via(EventProxy)]
#[derive(Debug, PartialEq, Eq)]
pub enum DomainEvent {
    Ping,
    Text(String),
    Metric { key: String, value: i64 },
}

impl From<EventProxy> for DomainEvent {
    fn from(p: EventProxy) -> Self {
        match p {
            EventProxy::Ping => Self::Ping,
            EventProxy::Text(s) => Self::Text(s),
            EventProxy::Metric { key, value } => Self::Metric { key, value },
        }
    }
}

impl From<&DomainEvent> for EventProxy {
    fn from(e: &DomainEvent) -> Self {
        match e {
            DomainEvent::Ping => Self::Ping,
            DomainEvent::Text(s) => Self::Text(s.clone()),
            DomainEvent::Metric { key, value } => Self::Metric {
                key: key.clone(),
                value: *value,
            },
        }
    }
}

#[test]
fn test_struct_tuple_unit_enum_and_formats() {
    // Named struct across JSON, RON, TOML
    let from_v1: AppConfig = serde_json::from_str(r#"{"version":1,"name":"svc"}"#).expect("v1 json");
    assert_eq!(
        from_v1,
        AppConfig {
            name: "svc".to_owned(),
            port: 8080,
        }
    );

    let json = serde_json::to_string(&from_v1).expect("to json");
    assert!(json.contains(r#""version":2"#), "missing v2 tag: {json}");
    assert_eq!(
        serde_json::from_str::<AppConfig>(&json).expect("from json"),
        from_v1
    );

    let ron_str = ron::to_string(&from_v1).expect("to ron");
    assert_eq!(ron::from_str::<AppConfig>(&ron_str).expect("from ron"), from_v1);

    let toml_str = toml::to_string(&from_v1).expect("to toml");
    assert_eq!(
        toml::from_str::<AppConfig>(&toml_str).expect("from toml"),
        from_v1
    );

    // Tuple struct
    let uid_v1: UserId = serde_json::from_str(r#"{"v":1,"data":42}"#).expect("uid v1");
    assert_eq!(uid_v1, UserId(42));
    let uid_json = serde_json::to_string(&uid_v1).expect("uid to json");
    assert_eq!(uid_json, r#"{"v":2,"data":42}"#);
    assert_eq!(
        serde_json::from_str::<UserId>(&uid_json).expect("uid v2"),
        UserId(42)
    );

    // Unit struct
    let marker_json = serde_json::to_string(&Marker).expect("marker to json");
    assert_eq!(marker_json, r#"{"version":1,"data":null}"#);
    assert_eq!(
        serde_json::from_str::<Marker>(&marker_json).expect("marker from json"),
        Marker
    );

    // Enum
    let ev_v1: DomainEvent = serde_json::from_str(r#"{"v":1,"payload":{"text":"hi"}}"#).expect("event v1");
    assert_eq!(ev_v1, DomainEvent::Text("hi".to_owned()));

    for ev in [
        DomainEvent::Ping,
        DomainEvent::Text("hello".to_owned()),
        DomainEvent::Metric {
            key: "rps".to_owned(),
            value: 99,
        },
    ] {
        let s = serde_json::to_string(&ev).expect("event to json");
        assert_eq!(
            serde_json::from_str::<DomainEvent>(&s).expect("event from json"),
            ev
        );
    }
}

// =========================================================================
// 2 & 3. BackwardsCompat metadata delegation + Migrated<T> tracking
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingV1 {
    pub title: String,
}

impl From<RollingV1> for RollingProxy {
    fn from(v1: RollingV1) -> Self {
        Self {
            title: v1.title,
            rev: 1,
        }
    }
}

fn downgrade_rolling(p: &RollingProxy) -> RollingV1 {
    RollingV1 {
        title: p.title.clone(),
    }
}

#[backwards_compat(
    tag = "version",
    versions(
        _: RollingV1,
        #[serialize(with = downgrade_rolling)] 1: RollingV1,
        #[current] 2: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollingProxy {
    pub title: String,
    pub rev: u32,
}

#[backwards_compat_via(RollingProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct RollingDoc {
    pub title: String,
    pub rev: u32,
}

impl From<RollingProxy> for RollingDoc {
    fn from(p: RollingProxy) -> Self {
        Self {
            title: p.title,
            rev: p.rev,
        }
    }
}

impl From<&RollingDoc> for RollingProxy {
    fn from(d: &RollingDoc) -> Self {
        Self {
            title: d.title.clone(),
            rev: d.rev,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StrV1 {
    pub label: String,
}

#[backwards_compat(tag = "ver", versions("1.0": StrV1, "2.0": Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrProxy {
    pub label: String,
}

impl From<StrV1> for StrProxy {
    fn from(v: StrV1) -> Self {
        Self { label: v.label }
    }
}

#[backwards_compat_via(StrProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct StrDomain {
    pub label: String,
}

impl From<StrProxy> for StrDomain {
    fn from(p: StrProxy) -> Self {
        Self { label: p.label }
    }
}

impl From<&StrDomain> for StrProxy {
    fn from(d: &StrDomain) -> Self {
        Self {
            label: d.label.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct MixedV1 {
    pub n: i32,
}

#[backwards_compat(tag = "v", versions(1 | "1": MixedV1, 2 | "2": Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedProxy {
    pub n: i32,
}

impl From<MixedV1> for MixedProxy {
    fn from(v: MixedV1) -> Self {
        Self { n: v.n }
    }
}

#[backwards_compat_via(MixedProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct MixedDomain {
    pub n: i32,
}

impl From<MixedProxy> for MixedDomain {
    fn from(p: MixedProxy) -> Self {
        Self { n: p.n }
    }
}

impl From<&MixedDomain> for MixedProxy {
    fn from(d: &MixedDomain) -> Self {
        Self { n: d.n }
    }
}

#[test]
fn test_metadata_delegation_and_migrated_tracking() {
    // Integer + untagged + rolling #[serialize]
    assert_eq!(RollingDoc::CURRENT_VERSION, 2);
    assert_eq!(RollingDoc::SERIALIZE_VERSION, 1);
    assert_eq!(RollingDoc::SUPPORTED_VERSIONS, &[1, 2]);
    const {
        assert!(RollingDoc::ACCEPTS_UNTAGGED);
    }

    let untagged: Migrated<RollingDoc> = serde_json::from_str(r#"{"title":"legacy"}"#).expect("untagged");
    assert_eq!(untagged.source_version, None);
    assert!(untagged.was_migrated());
    assert_eq!(
        untagged.into_inner(),
        RollingDoc {
            title: "legacy".to_owned(),
            rev: 1,
        }
    );

    let hist: Migrated<RollingDoc> = serde_json::from_str(r#"{"version":1,"title":"old"}"#).expect("v1");
    assert_eq!(hist.source_version, Some(1));
    assert!(hist.was_migrated());
    assert_eq!(
        hist.into_inner(),
        RollingDoc {
            title: "old".to_owned(),
            rev: 1,
        }
    );

    let curr: Migrated<RollingDoc> =
        serde_json::from_str(r#"{"version":2,"title":"new","rev":5}"#).expect("v2");
    assert_eq!(curr.source_version, Some(2));
    assert!(!curr.was_migrated());
    let doc = curr.into_inner();
    assert_eq!(
        doc,
        RollingDoc {
            title: "new".to_owned(),
            rev: 5,
        }
    );
    // Rolling serialize emits v1 wire format
    assert_eq!(
        serde_json::to_string(&doc).expect("rolling ser"),
        r#"{"version":1,"title":"new"}"#
    );

    // String version proxy
    assert_eq!(StrDomain::CURRENT_VERSION, "2.0");
    assert_eq!(StrDomain::SERIALIZE_VERSION, "2.0");
    assert_eq!(StrDomain::SUPPORTED_VERSIONS, &["1.0", "2.0"]);
    const {
        assert!(!StrDomain::ACCEPTS_UNTAGGED);
    }

    let str_m: Migrated<StrDomain> = serde_json::from_str(r#"{"ver":"1.0","label":"a"}"#).expect("str v1");
    assert_eq!(str_m.source_version, Some("1.0"));
    assert!(str_m.was_migrated());
    assert_eq!(
        str_m.into_inner(),
        StrDomain {
            label: "a".to_owned(),
        }
    );

    // Mixed VersionKey proxy
    assert_eq!(MixedDomain::CURRENT_VERSION, VersionKey::Int(2));
    assert_eq!(MixedDomain::SERIALIZE_VERSION, VersionKey::Int(2));
    assert_eq!(
        MixedDomain::SUPPORTED_VERSIONS,
        &[
            VersionKey::Int(1),
            VersionKey::Str("1"),
            VersionKey::Int(2),
            VersionKey::Str("2"),
        ]
    );
    const {
        assert!(!MixedDomain::ACCEPTS_UNTAGGED);
    }

    let mixed_m: Migrated<MixedDomain> = serde_json::from_str(r#"{"v":"1","n":7}"#).expect("mixed v1 str");
    assert_eq!(mixed_m.source_version, Some(VersionKey::Str("1")));
    assert!(mixed_m.was_migrated());
    assert_eq!(mixed_m.into_inner(), MixedDomain { n: 7 });
}

// =========================================================================
// 4. Default infallible serialize: zero-clone borrowed & Clone fallback
// =========================================================================

#[backwards_compat(tag = "version", versions(1: Self))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleProxy {
    pub value: u32,
}

#[backwards_compat_via(SimpleProxy)]
#[derive(Debug, PartialEq, Eq)]
pub struct NonCloneDomain {
    pub value: u32,
}

impl From<SimpleProxy> for NonCloneDomain {
    fn from(p: SimpleProxy) -> Self {
        Self { value: p.value }
    }
}

impl From<&NonCloneDomain> for SimpleProxy {
    fn from(d: &NonCloneDomain) -> Self {
        Self { value: d.value }
    }
}

#[backwards_compat_via(SimpleProxy)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneFallbackDomain {
    pub value: u32,
}

impl From<SimpleProxy> for CloneFallbackDomain {
    fn from(p: SimpleProxy) -> Self {
        Self { value: p.value }
    }
}

impl From<CloneFallbackDomain> for SimpleProxy {
    fn from(d: CloneFallbackDomain) -> Self {
        Self { value: d.value }
    }
}

#[test]
fn test_infallible_borrowed_and_clone_fallback() {
    let nc = NonCloneDomain { value: 11 };
    let nc_json = serde_json::to_string(&nc).expect("non-clone ser");
    assert_eq!(nc_json, r#"{"version":1,"value":11}"#);
    assert_eq!(
        serde_json::from_str::<NonCloneDomain>(&nc_json).expect("non-clone de"),
        nc
    );

    let cf = CloneFallbackDomain { value: 22 };
    let cf_json = serde_json::to_string(&cf).expect("clone fallback ser");
    assert_eq!(cf_json, r#"{"version":1,"value":22}"#);
    assert_eq!(
        serde_json::from_str::<CloneFallbackDomain>(&cf_json).expect("clone fallback de"),
        cf
    );
}

// =========================================================================
// 5, 6, 7. try_from, try_into, and fallible
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct PortProxyV1 {
    pub port: u32,
}

#[backwards_compat(tag = "version", versions(1: PortProxyV1, 2: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortProxy {
    pub port: u32,
}

impl From<PortProxyV1> for PortProxy {
    fn from(v: PortProxyV1) -> Self {
        Self { port: v.port }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid port: {0}")]
pub struct PortError(pub u32);

#[backwards_compat_via(PortProxy, try_from)]
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPort {
    pub port: u16,
}

impl TryFrom<PortProxy> for ValidatedPort {
    type Error = PortError;

    fn try_from(p: PortProxy) -> Result<Self, Self::Error> {
        let port = u16::try_from(p.port).map_err(|_| PortError(p.port))?;
        if port == 0 {
            return Err(PortError(0));
        }
        Ok(Self { port })
    }
}

impl From<&ValidatedPort> for PortProxy {
    fn from(v: &ValidatedPort) -> Self {
        Self {
            port: u32::from(v.port),
        }
    }
}

#[backwards_compat_via(PortProxy, try_into)]
#[derive(Debug, PartialEq, Eq)]
pub struct TryIntoBorrowed {
    pub port: u32,
}

impl From<PortProxy> for TryIntoBorrowed {
    fn from(p: PortProxy) -> Self {
        Self { port: p.port }
    }
}

impl TryFrom<&TryIntoBorrowed> for PortProxy {
    type Error = PortError;

    fn try_from(v: &TryIntoBorrowed) -> Result<Self, Self::Error> {
        if v.port > 65_535 {
            return Err(PortError(v.port));
        }
        Ok(Self { port: v.port })
    }
}

#[backwards_compat_via(PortProxy, try_into)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryIntoCloneFallback {
    pub port: u32,
}

impl From<PortProxy> for TryIntoCloneFallback {
    fn from(p: PortProxy) -> Self {
        Self { port: p.port }
    }
}

impl TryFrom<TryIntoCloneFallback> for PortProxy {
    type Error = PortError;

    fn try_from(v: TryIntoCloneFallback) -> Result<Self, Self::Error> {
        if v.port > 65_535 {
            return Err(PortError(v.port));
        }
        Ok(Self { port: v.port })
    }
}

#[backwards_compat_via(PortProxy, try_into)]
#[derive(Debug, PartialEq, Eq)]
pub struct TryIntoPreferBorrowed {
    pub port: u32,
}

impl Clone for TryIntoPreferBorrowed {
    fn clone(&self) -> Self {
        panic!("Clone must not be called when borrowed TryInto is available");
    }
}

impl From<PortProxy> for TryIntoPreferBorrowed {
    fn from(p: PortProxy) -> Self {
        Self { port: p.port }
    }
}

impl TryFrom<&TryIntoPreferBorrowed> for PortProxy {
    type Error = &'static str;

    fn try_from(v: &TryIntoPreferBorrowed) -> Result<Self, Self::Error> {
        if v.port > 65_535 {
            return Err("borrowed port overflow");
        }
        Ok(Self { port: v.port })
    }
}

impl TryFrom<TryIntoPreferBorrowed> for PortProxy {
    type Error = &'static str;

    fn try_from(v: TryIntoPreferBorrowed) -> Result<Self, Self::Error> {
        if v.port > 65_535 {
            return Err("owned port overflow");
        }
        Ok(Self { port: v.port })
    }
}

#[backwards_compat_via(PortProxy, fallible)]
#[derive(Debug, PartialEq, Eq)]
pub struct FalliblePort {
    pub port: u32,
}

impl TryFrom<PortProxy> for FalliblePort {
    type Error = PortError;

    fn try_from(p: PortProxy) -> Result<Self, Self::Error> {
        if p.port == 0 {
            return Err(PortError(0));
        }
        Ok(Self { port: p.port })
    }
}

impl TryFrom<&FalliblePort> for PortProxy {
    type Error = PortError;

    fn try_from(v: &FalliblePort) -> Result<Self, Self::Error> {
        if v.port > 65_535 {
            return Err(PortError(v.port));
        }
        Ok(Self { port: v.port })
    }
}

#[test]
fn test_try_from_try_into_and_fallible() {
    // try_from
    let ok_v1: Migrated<ValidatedPort> =
        serde_json::from_str(r#"{"version":1,"port":443}"#).expect("valid v1");
    assert_eq!(ok_v1.source_version, Some(1));
    assert_eq!(ok_v1.into_inner(), ValidatedPort { port: 443 });

    let err_overflow = serde_json::from_str::<ValidatedPort>(r#"{"version":2,"port":70000}"#)
        .expect_err("overflow should fail");
    assert!(
        err_overflow.to_string().contains("invalid port: 70000"),
        "unexpected error: {err_overflow}"
    );

    let err_zero =
        serde_json::from_str::<ValidatedPort>(r#"{"version":1,"port":0}"#).expect_err("zero should fail");
    assert!(
        err_zero.to_string().contains("invalid port: 0"),
        "unexpected error: {err_zero}"
    );

    assert_eq!(
        serde_json::to_string(&ValidatedPort { port: 80 }).expect("ser validated"),
        r#"{"version":2,"port":80}"#
    );

    // try_into (borrowed, non-Clone)
    assert_eq!(
        serde_json::to_string(&TryIntoBorrowed { port: 8080 }).expect("borrowed ok"),
        r#"{"version":2,"port":8080}"#
    );
    let err_b = serde_json::to_string(&TryIntoBorrowed { port: 99_999 }).expect_err("borrowed overflow");
    assert!(
        err_b.to_string().contains("invalid port: 99999"),
        "unexpected error: {err_b}"
    );

    // try_into (Clone fallback)
    assert_eq!(
        serde_json::to_string(&TryIntoCloneFallback { port: 8080 }).expect("clone ok"),
        r#"{"version":2,"port":8080}"#
    );
    let err_c = serde_json::to_string(&TryIntoCloneFallback { port: 88_888 }).expect_err("clone overflow");
    assert!(
        err_c.to_string().contains("invalid port: 88888"),
        "unexpected error: {err_c}"
    );

    // try_into (prefers borrowed over Clone)
    assert_eq!(
        serde_json::to_string(&TryIntoPreferBorrowed { port: 443 }).expect("prefer borrowed ok"),
        r#"{"version":2,"port":443}"#
    );
    let prefer_borrowed_err =
        serde_json::to_string(&TryIntoPreferBorrowed { port: 70_000 }).expect_err("prefer borrowed err");
    assert!(
        prefer_borrowed_err.to_string().contains("borrowed port overflow"),
        "unexpected error: {prefer_borrowed_err}"
    );

    // fallible shorthand
    let fp: FalliblePort = serde_json::from_str(r#"{"version":2,"port":8080}"#).expect("fallible de ok");
    assert_eq!(fp, FalliblePort { port: 8080 });
    assert_eq!(
        serde_json::to_string(&fp).expect("fallible ser ok"),
        r#"{"version":2,"port":8080}"#
    );
    let fp_de_err =
        serde_json::from_str::<FalliblePort>(r#"{"version":2,"port":0}"#).expect_err("fallible de err");
    assert!(
        fp_de_err.to_string().contains("invalid port: 0"),
        "unexpected error: {fp_de_err}"
    );
    let fp_ser_err = serde_json::to_string(&FalliblePort { port: 70_000 }).expect_err("fallible ser err");
    assert!(
        fp_ser_err.to_string().contains("invalid port: 70000"),
        "unexpected error: {fp_ser_err}"
    );
}

// =========================================================================
// 8. Custom converters: from = / from(...) / to = / to(...)
// =========================================================================

mod conv {
    use super::{CustomFallible, CustomInfallible, CustomTryFrom, CustomTryInto, PortError, SimpleProxy};

    pub const fn from_infallible(p: SimpleProxy) -> CustomInfallible {
        CustomInfallible { value: p.value }
    }

    pub const fn to_infallible(c: &CustomInfallible) -> SimpleProxy {
        SimpleProxy { value: c.value }
    }

    pub const fn try_from_custom(p: SimpleProxy) -> Result<CustomTryFrom, PortError> {
        if p.value == 0 {
            Err(PortError(0))
        } else {
            Ok(CustomTryFrom { value: p.value })
        }
    }

    pub const fn to_try_from_custom(c: &CustomTryFrom) -> SimpleProxy {
        SimpleProxy { value: c.value }
    }

    pub const fn from_try_into_custom(p: SimpleProxy) -> CustomTryInto {
        CustomTryInto { value: p.value }
    }

    pub const fn try_to_custom(c: &CustomTryInto) -> Result<SimpleProxy, PortError> {
        if c.value > 100 {
            Err(PortError(c.value))
        } else {
            Ok(SimpleProxy { value: c.value })
        }
    }

    pub const fn try_from_fallible(p: SimpleProxy) -> Result<CustomFallible, PortError> {
        if p.value == 0 {
            Err(PortError(0))
        } else {
            Ok(CustomFallible { value: p.value })
        }
    }

    pub const fn try_to_fallible(c: &CustomFallible) -> Result<SimpleProxy, PortError> {
        if c.value > 100 {
            Err(PortError(c.value))
        } else {
            Ok(SimpleProxy { value: c.value })
        }
    }
}

#[backwards_compat_via(SimpleProxy, from = conv::from_infallible, to(conv::to_infallible))]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomInfallible {
    pub value: u32,
}

#[backwards_compat_via(SimpleProxy, try_from, from(conv::try_from_custom), to = conv::to_try_from_custom)]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomTryFrom {
    pub value: u32,
}

#[backwards_compat_via(SimpleProxy, try_into, from = conv::from_try_into_custom, to(conv::try_to_custom))]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomTryInto {
    pub value: u32,
}

#[backwards_compat_via(SimpleProxy, fallible, from(conv::try_from_fallible), to = conv::try_to_fallible)]
#[derive(Debug, PartialEq, Eq)]
pub struct CustomFallible {
    pub value: u32,
}

#[test]
fn test_custom_from_and_to_converters() {
    // Infallible
    let ci = CustomInfallible { value: 5 };
    let ci_json = serde_json::to_string(&ci).expect("ci ser");
    assert_eq!(ci_json, r#"{"version":1,"value":5}"#);
    assert_eq!(
        serde_json::from_str::<CustomInfallible>(&ci_json).expect("ci de"),
        ci
    );

    // try_from + custom fns
    let ctf: CustomTryFrom = serde_json::from_str(r#"{"version":1,"value":10}"#).expect("ctf de ok");
    assert_eq!(ctf, CustomTryFrom { value: 10 });
    assert_eq!(
        serde_json::to_string(&ctf).expect("ctf ser"),
        r#"{"version":1,"value":10}"#
    );
    assert!(
        serde_json::from_str::<CustomTryFrom>(r#"{"version":1,"value":0}"#)
            .expect_err("ctf de err")
            .to_string()
            .contains("invalid port: 0")
    );

    // try_into + custom fns
    let cti: CustomTryInto = serde_json::from_str(r#"{"version":1,"value":50}"#).expect("cti de");
    assert_eq!(
        serde_json::to_string(&cti).expect("cti ser ok"),
        r#"{"version":1,"value":50}"#
    );
    assert!(
        serde_json::to_string(&CustomTryInto { value: 101 })
            .expect_err("cti ser err")
            .to_string()
            .contains("invalid port: 101")
    );

    // fallible + custom fns
    let cf: CustomFallible = serde_json::from_str(r#"{"version":1,"value":42}"#).expect("cf de ok");
    assert_eq!(
        serde_json::to_string(&cf).expect("cf ser ok"),
        r#"{"version":1,"value":42}"#
    );
    assert!(
        serde_json::from_str::<CustomFallible>(r#"{"version":1,"value":0}"#)
            .expect_err("cf de err")
            .to_string()
            .contains("invalid port: 0")
    );
    assert!(
        serde_json::to_string(&CustomFallible { value: 200 })
            .expect_err("cf ser err")
            .to_string()
            .contains("invalid port: 200")
    );
}

// =========================================================================
// 9. Generics, lifetimes, and where clauses
// =========================================================================

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GenericProxyV1<'a, T> {
    pub name: &'a str,
    pub payload: T,
}

#[backwards_compat(tag = "version", versions(1: GenericProxyV1<'a, T>, 2: Self))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericProxy<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    pub name: &'a str,
    pub payload: T,
    pub extra: bool,
}

impl<'a, T> From<GenericProxyV1<'a, T>> for GenericProxy<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    fn from(v1: GenericProxyV1<'a, T>) -> Self {
        Self {
            name: v1.name,
            payload: v1.payload,
            extra: true,
        }
    }
}

#[backwards_compat_via(GenericProxy<'a, T>)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericDomain<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    pub name: &'a str,
    pub payload: T,
    pub extra: bool,
}

impl<'a, T> From<GenericProxy<'a, T>> for GenericDomain<'a, T>
where
    T: Clone + core::fmt::Debug + Default,
{
    fn from(p: GenericProxy<'a, T>) -> Self {
        Self {
            name: p.name,
            payload: p.payload,
            extra: p.extra,
        }
    }
}

impl<'a, T> From<&GenericDomain<'a, T>> for GenericProxy<'a, T>
where
    T: Clone + core::fmt::Debug,
{
    fn from(d: &GenericDomain<'a, T>) -> Self {
        Self {
            name: d.name,
            payload: d.payload.clone(),
            extra: d.extra,
        }
    }
}

#[test]
fn test_generics_lifetimes_and_where_clauses() {
    let v1_json = r#"{"version":1,"name":"borrowed","payload":42}"#;
    let decoded: GenericDomain<'_, u32> = serde_json::from_str(v1_json).expect("generic v1");
    assert_eq!(
        decoded,
        GenericDomain {
            name: "borrowed",
            payload: 42,
            extra: true,
        }
    );

    let encoded = serde_json::to_string(&decoded).expect("generic ser");
    assert!(encoded.contains(r#""version":2"#), "missing version 2: {encoded}");
    let roundtrip: GenericDomain<'_, u32> = serde_json::from_str(&encoded).expect("generic v2 roundtrip");
    assert_eq!(roundtrip, decoded);
}

// =========================================================================
// 10. Stripping conflicting #[derive(..., Serialize, Deserialize)]
// =========================================================================

#[backwards_compat_via(AppConfigProxy)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrippedStruct {
    pub name: String,
    pub port: u16,
}

impl From<AppConfigProxy> for StrippedStruct {
    fn from(p: AppConfigProxy) -> Self {
        Self {
            name: p.name,
            port: p.port,
        }
    }
}

impl From<&StrippedStruct> for AppConfigProxy {
    fn from(s: &StrippedStruct) -> Self {
        Self {
            name: s.name.clone(),
            port: s.port,
        }
    }
}

#[backwards_compat_via(EventProxy)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StrippedEnum {
    Ping,
    Text(String),
}

impl From<EventProxy> for StrippedEnum {
    fn from(p: EventProxy) -> Self {
        match p {
            EventProxy::Ping => Self::Ping,
            EventProxy::Text(s) => Self::Text(s),
            EventProxy::Metric { key, .. } => Self::Text(key),
        }
    }
}

impl From<&StrippedEnum> for EventProxy {
    fn from(e: &StrippedEnum) -> Self {
        match e {
            StrippedEnum::Ping => Self::Ping,
            StrippedEnum::Text(s) => Self::Text(s.clone()),
        }
    }
}

#[test]
fn test_conflicting_serde_derives_are_stripped() {
    let s: StrippedStruct =
        serde_json::from_str(r#"{"version":1,"name":"stripped"}"#).expect("stripped struct v1");
    assert_eq!(
        s,
        StrippedStruct {
            name: "stripped".to_owned(),
            port: 8080,
        }
    );
    let s_json = serde_json::to_string(&s).expect("stripped struct ser");
    assert!(
        s_json.contains(r#""version":2"#),
        "expected proxy version tag: {s_json}"
    );

    let e: StrippedEnum =
        serde_json::from_str(r#"{"v":1,"payload":{"text":"ok"}}"#).expect("stripped enum v1");
    assert_eq!(e, StrippedEnum::Text("ok".to_owned()));
    let e_json = serde_json::to_string(&e).expect("stripped enum ser");
    assert_eq!(e_json, r#"{"v":2,"payload":{"Text":"ok"}}"#);
}
