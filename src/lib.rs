//! # backwards-compat
//!
//! Declarative, compile-time verified schema versioning and backwards compatibility for Serde and domain models.
//!
//! ## Overview
//!
//! In applications with evolving data formats, schemas change across versions.
//! `backwards-compat` streamlines schema evolution by:
//! 1. Automatically implementing `serde::Serialize`, `serde::Deserialize`, [`BackwardsCompat`], and [`BackwardsCompatDeserialize`] directly for your target `struct` or `enum` model.
//! 2. Emitting native integer (`"version": 2`) or string (`"version": "2.0"`) tags with strict type matching and rich error messages.
//! 3. Automatically implementing `From<V>` (or `TryFrom<V>`) for your target model from multi-step historical versions `V`, enabling effortless in-code migrations.
//! 4. Implicitly generating zero-allocation shadow wire structs/enums, avoiding manual serialization boilerplate for the latest version.
//! 5. Supporting generics, lifetimes, adjacent tagging, DAG upgrades, inline `|` tag aliases, `#[with(fn_path)]`, rolling deployments (`#[serialize]`), feature-gated versions (`#[cfg(...)]`), and opt-in [`Migrated<T>`] tracking.
//!
//! You apply the `#[backwards_compat(...)]` attribute directly on your domain model:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! // 1. Define historical schemas (only Deserialize is required)
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct ConfigV1 {
//!     pub name: String,
//! }
//!
//! // The target domain model Config does NOT need #[derive(Serialize, Deserialize)].
//! // Declare `2: Self` to make version 2 the active schema using Config itself.
//! #[backwards_compat(tag = "version", versions(1: ConfigV1, 2: Self))]
//! #[derive(Debug, PartialEq)]
//! pub struct Config {
//!     pub name: String,
//!     pub port: u16,
//! }
//!
//! // 2. Implement the migration from previous versions
//! impl From<ConfigV1> for Config {
//!     fn from(v1: ConfigV1) -> Self {
//!         Self {
//!             name: v1.name,
//!             port: 8080,
//!         }
//!     }
//! }
//!
//! // 1. Deserializing legacy serialized data upgrades automatically (native integer tag):
//! let json_v1 = r#"{"version": 1, "name": "my-app"}"#;
//! let config: Config = serde_json::from_str(json_v1).unwrap();
//! assert_eq!(config.port, 8080);
//!
//! // 2. Serializing Config automatically tags it with the current version as a native integer:
//! let serialized = serde_json::to_string(&config).unwrap();
//! assert!(serialized.contains(r#""version":2"#));
//!
//! // 3. In-code manual conversion using From / Into:
//! let v1 = ConfigV1 { name: "my-app".into() };
//! let from_v1 = Config::from(v1.clone());
//! let into_config: Config = v1.into();
//! assert_eq!(from_v1, config);
//! assert_eq!(into_config, config);
//! ```
//!
//! ## Fallible Migrations (`#[fallible]`)
//!
//! When schema transitions are marked `#[fallible]`, `backwards_compat` uses `TryFrom<V>` (or a
//! `Result`-returning `#[with(...)]` function) with an optional custom `error = ...` type:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! #[derive(Debug, PartialEq)]
//! pub struct CustomError(pub String);
//!
//! impl std::fmt::Display for CustomError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "{}", self.0)
//!     }
//! }
//!
//! #[derive(Debug, Clone, Deserialize)]
//! pub struct ServerV1 { pub port: u32 }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     error = CustomError,
//!     versions(#[fallible] 1: ServerV1, 2: Self)
//! )]
//! #[derive(Debug, PartialEq)]
//! pub struct Server { pub port: u16 }
//!
//! impl TryFrom<ServerV1> for Server {
//!     type Error = CustomError;
//!     fn try_from(v1: ServerV1) -> Result<Self, Self::Error> {
//!         let port = u16::try_from(v1.port).map_err(|_| CustomError("overflow".into()))?;
//!         Ok(Self { port })
//!     }
//! }
//!
//! // Manual fallible conversion using TryFrom / TryInto:
//! let v1_valid = ServerV1 { port: 8080 };
//! let server: Server = Server::try_from(v1_valid.clone()).unwrap();
//! let server2: Server = v1_valid.try_into().unwrap();
//! assert_eq!(server, server2);
//!
//! let v1_invalid = ServerV1 { port: 70000 };
//! assert!(Server::try_from(v1_invalid).is_err());
//! ```
//!
//! ## Adjacent Tagging (`content = "..."`) & `enum` Domain Models
//!
//! For tuple structs, scalar types, newtypes, and `enum` domain models that cannot be flattened into a map,
//! configure adjacent tagging with `content = "..."`:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! #[derive(Debug, PartialEq, Deserialize)]
//! pub struct EventV1 {
//!     pub text: String,
//! }
//!
//! #[backwards_compat(
//!     tag = "v",
//!     content = "data",
//!     versions(1: EventV1, 2: Self)
//! )]
//! #[derive(Debug, PartialEq)]
//! pub enum Event {
//!     Ping,
//!     Message(String),
//! }
//!
//! impl From<EventV1> for Event {
//!     fn from(v1: EventV1) -> Self {
//!         Self::Message(v1.text)
//!     }
//! }
//!
//! let v1_json = r#"{"v": 1, "data": {"text": "hello"}}"#;
//! let event: Event = serde_json::from_str(v1_json).unwrap();
//! assert_eq!(event, Event::Message("hello".into()));
//!
//! let serialized = serde_json::to_string(&event).unwrap();
//! assert_eq!(serialized, r#"{"v":2,"data":{"Message":"hello"}}"#);
//! ```
//!
//! ## Generics and Lifetimes
//!
//! `#[backwards_compat]` fully supports generic parameters, lifetime parameters, and `where` clauses:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! #[derive(Debug, PartialEq, Deserialize)]
//! pub struct GenericRecordV1<'a, T> {
//!     pub name: &'a str,
//!     pub payload: T,
//! }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(1: GenericRecordV1<'a, T>, 2: Self)
//! )]
//! #[derive(Debug, PartialEq)]
//! pub struct GenericRecord<'a, T: Clone>
//! where
//!     T: std::fmt::Debug,
//! {
//!     pub name: &'a str,
//!     pub payload: T,
//!     pub extra: bool,
//! }
//!
//! impl<'a, T: Clone> From<GenericRecordV1<'a, T>> for GenericRecord<'a, T>
//! where
//!     T: std::fmt::Debug,
//! {
//!     fn from(v1: GenericRecordV1<'a, T>) -> Self {
//!         Self {
//!             name: v1.name,
//!             payload: v1.payload,
//!             extra: true,
//!         }
//!     }
//! }
//! ```
//!
//! ## DAG Transitions (`=> tag` / `=> Self`) & Custom Converters (`#[with(fn_path)]`)
//!
//! By default, versions upgrade sequentially (`1 -> 2 -> ... -> N`). You can also define custom
//! upgrade graphs with `=> <next_version>` or `=> Self`, and specify custom conversion functions
//! with `#[with(fn_path)]` instead of implementing `From`/`TryFrom`:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct V1 { pub val: i32 }
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct V2 { pub val: i32 }
//!
//! fn upgrade_v1(v: V1) -> V3 { V3 { val: v.val * 10 } }
//! impl From<V2> for V3 { fn from(v: V2) -> Self { Self { val: v.val } } }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(
//!         #[with(upgrade_v1)] 1: V1 => Self,
//!         2: V2 => Self,
//!         3: Self,
//!     )
//! )]
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct V3 { pub val: i32 }
//!
//! let from_v1: V3 = serde_json::from_str(r#"{"version": 1, "val": 4}"#).unwrap();
//! assert_eq!(from_v1, V3 { val: 40 });
//! ```
//!
//! ## String Version Keys & Inline Tag Aliases (`|`)
//!
//! You can use string keys (such as `SemVer` strings) or combine multiple wire tags (including
//! migrating from 1.x stringified integer tags to 2.0 native integer tags) with `|`:
//!
//! ```rust
//! use backwards_compat::{BackwardsCompat, VersionKey, backwards_compat};
//! use serde::Deserialize;
//!
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct V1 { pub val: i32 }
//! impl From<V1> for V2 { fn from(v: V1) -> Self { Self { val: v.val } } }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(
//!         1 | "1": V1,
//!         2 | "2": Self,
//!     )
//! )]
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct V2 { pub val: i32 }
//!
//! // Both native integer and legacy string tags deserialize cleanly:
//! assert_eq!(serde_json::from_str::<V2>(r#"{"version": 1, "val": 7}"#).unwrap(), V2 { val: 7 });
//! assert_eq!(serde_json::from_str::<V2>(r#"{"version": "1", "val": 7}"#).unwrap(), V2 { val: 7 });
//! // Mixed integer/string tags use `VersionKey`, and the first tag in `2 | "2"` is canonical:
//! assert_eq!(V2::CURRENT_VERSION, VersionKey::Int(2));
//! ```
//!
//! ## Metadata (`BackwardsCompat`), Migration Tracking (`Migrated<T>`), & Rolling Deployments (`#[serialize]`)
//!
//! Every `#[backwards_compat]` type implements [`BackwardsCompat`] and [`BackwardsCompatDeserialize`].
//! Deserialize into [`Migrated<T>`] to inspect which version a payload came from, and use `#[serialize]`
//! (or `#[serialize(with = fn_path)]`) to temporarily emit an older schema during rolling deployments:
//!
//! ```rust
//! use backwards_compat::{BackwardsCompat, Migrated, backwards_compat};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct DocV1 { pub title: String }
//!
//! impl From<DocV1> for Doc {
//!     fn from(v1: DocV1) -> Self { Self { title: v1.title, rev: 1 } }
//! }
//! fn downgrade_to_v1(doc: &Doc) -> DocV1 {
//!     DocV1 { title: doc.title.clone() }
//! }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(
//!         #[serialize(with = downgrade_to_v1)] 1: DocV1,
//!         #[current] 2: Self,
//!     )
//! )]
//! #[derive(Debug, PartialEq)]
//! pub struct Doc { pub title: String, pub rev: u32 }
//!
//! assert_eq!(Doc::CURRENT_VERSION, 2);
//! assert_eq!(Doc::SERIALIZE_VERSION, 1);
//!
//! let migrated: Migrated<Doc> = serde_json::from_str(r#"{"version": 1, "title": "hi"}"#).unwrap();
//! assert!(migrated.was_migrated());
//! assert_eq!(migrated.source_version, Some(1));
//! assert_eq!(migrated.into_inner(), Doc { title: "hi".into(), rev: 1 });
//!
//! // Serializing emits version 1 while rolling out v2 readers:
//! let wire = serde_json::to_string(&Doc { title: "hi".into(), rev: 5 }).unwrap();
//! assert_eq!(wire, r#"{"version":1,"title":"hi"}"#);
//! ```
//!
//! ## Multiple Versions Sharing the Same Type
//!
//! Multiple historical version tags can map to the exact same Rust data type (for example, when the wire format was unchanged across minor releases or when a previous data type is reused).
//!
//! When multiple versions share a type (such as `versions(1: V1, 2: V1, 3: V2)` or `versions(1: V1, 2: V2, 3: V1)`), transitions automatically branch and route conversions from the *latest* occurrence of each type in the migration graph:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct PayloadV1 {
//!     pub data: String,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct PayloadV2 {
//!     pub data: String,
//!     pub extra: u32,
//! }
//!
//! impl From<PayloadV1> for PayloadV2 {
//!     fn from(v1: PayloadV1) -> Self {
//!         Self {
//!             data: v1.data,
//!             extra: 0,
//!         }
//!     }
//! }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(1: PayloadV1, 2: PayloadV1, 3: PayloadV2)
//! )]
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct Payload {
//!     pub data: String,
//!     pub extra: u32,
//! }
//!
//! impl From<PayloadV2> for Payload {
//!     fn from(v2: PayloadV2) -> Self {
//!         Self {
//!             data: v2.data,
//!             extra: v2.extra,
//!         }
//!     }
//! }
//!
//! impl From<Payload> for PayloadV2 {
//!     fn from(p: Payload) -> Self {
//!         Self {
//!             data: p.data,
//!             extra: p.extra,
//!         }
//!     }
//! }
//!
//! let v1_json = r#"{"version": 1, "data": "hello"}"#;
//! let p1: Payload = serde_json::from_str(v1_json).unwrap();
//! assert_eq!(p1, Payload { data: "hello".into(), extra: 0 });
//!
//! let v2_json = r#"{"version": 2, "data": "world"}"#;
//! let p2: Payload = serde_json::from_str(v2_json).unwrap();
//! assert_eq!(p2, Payload { data: "world".into(), extra: 0 });
//! ```
//!
//! ## Pre-Backwards-Compat (Unversioned) Schemas (`_: Type` / `_: Self`)
//!
//! When adopting `#[backwards_compat]` on an existing system that previously serialized payloads
//! without a version tag field, use `_` as the version key in `versions(...)`. Multiple `_` entries
//! are supported and tried in declaration order (like `#[serde(untagged)]`):
//!
//! ```rust
//! use backwards_compat::{BackwardsCompat, Migrated, backwards_compat};
//! use serde::Deserialize;
//!
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct ConfigV0 {
//!     pub name: String,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Deserialize)]
//! pub struct ConfigV1 {
//!     pub name: String,
//!     pub host: String,
//! }
//!
//! impl From<ConfigV0> for ConfigV1 {
//!     fn from(v0: ConfigV0) -> Self {
//!         Self { name: v0.name, host: "localhost".into() }
//!     }
//! }
//!
//! impl From<ConfigV1> for Config {
//!     fn from(v1: ConfigV1) -> Self {
//!         Self { name: v1.name, host: v1.host, port: 8080 }
//!     }
//! }
//!
//! #[backwards_compat(
//!     tag = "version",
//!     versions(
//!         _: ConfigV1,
//!         _: ConfigV0,
//!         1: ConfigV1,
//!         2: Self,
//!     )
//! )]
//! #[derive(Debug, PartialEq)]
//! pub struct Config {
//!     pub name: String,
//!     pub host: String,
//!     pub port: u16,
//! }
//!
//! assert!(Config::ACCEPTS_UNTAGGED);
//!
//! // Deserializing an unversioned legacy payload tries ConfigV1 then ConfigV0:
//! let unversioned: Migrated<Config> = serde_json::from_str(r#"{"name": "legacy-app"}"#).unwrap();
//! assert_eq!(unversioned.source_version, None);
//! assert!(unversioned.was_migrated());
//! assert_eq!(unversioned.into_inner(), Config { name: "legacy-app".into(), host: "localhost".into(), port: 8080 });
//! ```
//!
//! If the schema has not changed yet and you simply want to start emitting a version tag while still accepting
//! unversioned payloads, use `_: Self`:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//!
//! #[backwards_compat(tag = "version", versions(_: Self, 1: Self))]
//! #[derive(Debug, PartialEq)]
//! pub struct AppSettings {
//!     pub debug: bool,
//! }
//!
//! let legacy = r#"{"debug": true}"#;
//! let tagged = r#"{"version": 1, "debug": true}"#;
//! assert_eq!(
//!     serde_json::from_str::<AppSettings>(legacy).unwrap(),
//!     serde_json::from_str::<AppSettings>(tagged).unwrap()
//! );
//! ```
//!
//! ## Feature-Gated Versions (`#[cfg(...)]`)
//!
//! Historical and unversioned (`_`) version entries can be conditionally compiled with `#[cfg(...)]`.
//! When a predicate is disabled, the version's wire type, `From`/`TryFrom` impls, and deserialization
//! arm are completely omitted from the generated code.
//!
//! - Multiple `#[cfg(...)]` predicates on a single entry or chained along a migration path are combined with `all(...)`.
//! - Neither the active `#[current]` version nor the `#[serialize]` version may be `#[cfg(...)]`-gated.
//! - Unconditional versions cannot migrate through a `#[cfg(...)]`-gated version (add `#[cfg(...)]` or jump with `=>`).
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::Deserialize;
//!
//! #[cfg(all())]
//! #[derive(Debug, PartialEq, Deserialize)]
//! pub struct ConfigV1 {
//!     pub name: String,
//! }
//!
//! #[cfg(all())]
//! impl From<ConfigV1> for Config {
//!     fn from(v1: ConfigV1) -> Self {
//!         Self { name: v1.name, port: 8080 }
//!     }
//! }
//!
//! // Notice `NonExistentV0` does not exist at all when `#[cfg(any())]` is false:
//! #[backwards_compat(
//!     tag = "version",
//!     versions(
//!         #[cfg(any())] 0: NonExistentV0,
//!         #[cfg(all())] 1: ConfigV1,
//!         2: Self,
//!     )
//! )]
//! #[derive(Debug, PartialEq)]
//! pub struct Config {
//!     pub name: String,
//!     pub port: u16,
//! }
//!
//! // Enabled gated version deserializes and upgrades normally:
//! let v1_json = r#"{"version": 1, "name": "app"}"#;
//! let cfg: Config = serde_json::from_str(v1_json).unwrap();
//! assert_eq!(cfg, Config { name: "app".into(), port: 8080 });
//!
//! // Disabled gated version is rejected at runtime with a helpful hint:
//! let v0_json = r#"{"version": 0, "name": "old"}"#;
//! let err = serde_json::from_str::<Config>(v0_json).unwrap_err();
//! assert!(err.to_string().contains("feature flags"));
//! ```
pub use backwards_compat_derive::backwards_compat;

mod content;

/// Represents a schema version key for models that mix integer and string version tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VersionKey {
    /// An integer version tag (e.g. `1`, `2`).
    Int(u64),
    /// A string version tag (e.g. `"1.0"`, `"v2"`).
    Str(&'static str),
}

impl core::fmt::Display for VersionKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Str(s) => f.write_str(s),
        }
    }
}

impl PartialEq<u64> for VersionKey {
    fn eq(&self, other: &u64) -> bool {
        matches!(*self, Self::Int(n) if n == *other)
    }
}

impl PartialEq<&str> for VersionKey {
    fn eq(&self, other: &&str) -> bool {
        matches!(*self, Self::Str(s) if s == *other)
    }
}

/// Metadata trait implemented for domain models annotated with `#[backwards_compat(...)]`.
pub trait BackwardsCompat: Sized {
    /// The type representing version identifiers (`u64`, `&'static str`, or [`VersionKey`]).
    type Version: Copy + Eq + core::fmt::Debug + core::fmt::Display + 'static;

    /// The current active schema version.
    const CURRENT_VERSION: Self::Version;

    /// The schema version emitted when serializing this type.
    const SERIALIZE_VERSION: Self::Version;

    /// All supported schema versions compiled into this binary.
    const SUPPORTED_VERSIONS: &'static [Self::Version];

    /// Whether unversioned (`_`) payloads without a version tag field are accepted.
    const ACCEPTS_UNTAGGED: bool = false;
}

/// Extension of [`BackwardsCompat`] supporting deserialization with source-version tracking.
pub trait BackwardsCompatDeserialize<'de>: BackwardsCompat {
    /// Deserializes `Self` while recording the source schema version the payload was decoded from.
    fn deserialize_migrated<D>(deserializer: D) -> Result<Migrated<Self>, D::Error>
    where
        D: serde::Deserializer<'de>;
}

/// Wrapper returned when deserializing a [`BackwardsCompat`] type together with its source version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migrated<T: BackwardsCompat> {
    /// The deserialized (and possibly upgraded) domain value.
    pub value: T,
    /// The schema version found in the serialized payload, or `None` if decoded from an unversioned (`_`) payload.
    pub source_version: Option<T::Version>,
}

impl<T: BackwardsCompat> Migrated<T> {
    /// Returns `true` if the payload was migrated from an older or unversioned schema rather than
    /// decoded directly from [`BackwardsCompat::CURRENT_VERSION`].
    #[must_use]
    pub fn was_migrated(&self) -> bool {
        self.source_version != Some(T::CURRENT_VERSION)
    }

    /// Consumes this wrapper and returns the inner domain value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<'de, T: BackwardsCompatDeserialize<'de>> serde::Deserialize<'de> for Migrated<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize_migrated(deserializer)
    }
}

#[doc(hidden)]
pub mod __private {
    pub use crate::content::{
        Content, ContentDeserializer, ExtractedPayload, TagValue, VariantContentDeserializer,
        unknown_version_error,
    };

    pub struct WireConvert<'b, T: ?Sized>(pub &'b T);

    pub trait BorrowedWireConvert<W> {
        fn convert_wire(&self) -> W;
    }

    impl<'b, T: ?Sized, W> BorrowedWireConvert<W> for &WireConvert<'b, T>
    where
        &'b T: ::core::convert::Into<W>,
    {
        #[inline]
        fn convert_wire(&self) -> W {
            ::core::convert::Into::into(self.0)
        }
    }

    pub trait FallbackWireConvert<W> {
        fn convert_wire(&self) -> W;
    }

    impl<T, W> FallbackWireConvert<W> for WireConvert<'_, T>
    where
        T: ::core::clone::Clone + ::core::convert::Into<W>,
    {
        #[inline]
        fn convert_wire(&self) -> W {
            ::core::convert::Into::into(::core::clone::Clone::clone(self.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MockModel(u64);

    impl BackwardsCompat for MockModel {
        type Version = u64;
        const CURRENT_VERSION: Self::Version = 2;
        const SERIALIZE_VERSION: Self::Version = 2;
        const SUPPORTED_VERSIONS: &'static [Self::Version] = &[1, 2];
        const ACCEPTS_UNTAGGED: bool = true;
    }

    impl<'de> BackwardsCompatDeserialize<'de> for MockModel {
        fn deserialize_migrated<D>(deserializer: D) -> Result<Migrated<Self>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let tag = Option::<u64>::deserialize(deserializer)?;
            Ok(Migrated {
                value: Self(tag.unwrap_or(0)),
                source_version: tag,
            })
        }
    }

    use serde::Deserialize as _;

    #[test]
    fn test_version_key_display_and_equality() {
        let int_key = VersionKey::Int(42);
        let str_key = VersionKey::Str("v1.2");

        assert_eq!(int_key.to_string(), "42");
        assert_eq!(str_key.to_string(), "v1.2");

        // PartialEq<u64>
        assert_eq!(int_key, 42);
        assert_ne!(int_key, 7);
        assert_ne!(VersionKey::Str("42"), 42);

        // PartialEq<&str>
        assert_eq!(str_key, "v1.2");
        assert_ne!(str_key, "v2.0");
        assert_ne!(VersionKey::Int(1), "1");
    }

    #[test]
    fn test_migrated_was_migrated_and_into_inner() {
        let current = Migrated {
            value: MockModel(20),
            source_version: Some(2),
        };
        assert!(!current.was_migrated());
        assert_eq!(current.into_inner(), MockModel(20));

        let older = Migrated {
            value: MockModel(10),
            source_version: Some(1),
        };
        assert!(older.was_migrated());
        assert_eq!(older.into_inner(), MockModel(10));

        let untagged = Migrated {
            value: MockModel(0),
            source_version: None,
        };
        assert!(untagged.was_migrated());
        assert_eq!(untagged.into_inner(), MockModel(0));
    }

    #[test]
    fn test_migrated_deserialize_delegates_to_backwards_compat_deserialize() {
        let current: Migrated<MockModel> = serde_json::from_str("2").unwrap();
        assert_eq!(current.source_version, Some(2));
        assert!(!current.was_migrated());
        assert_eq!(current.into_inner(), MockModel(2));

        let older: Migrated<MockModel> = serde_json::from_str("1").unwrap();
        assert_eq!(older.source_version, Some(1));
        assert!(older.was_migrated());
        assert_eq!(older.into_inner(), MockModel(1));

        let untagged: Migrated<MockModel> = serde_json::from_str("null").unwrap();
        assert_eq!(untagged.source_version, None);
        assert!(untagged.was_migrated());
        assert_eq!(untagged.into_inner(), MockModel(0));
    }
}
