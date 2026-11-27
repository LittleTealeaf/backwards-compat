# backwards-compat 

[![Crates.io](https://img.shields.io/crates/v/backwards-compat.svg)](https://crates.io/crates/backwards-compat)
[![Documentation](https://img.shields.io/badge/docs-github_pages-blue.svg)](https://littletealeaf.github.io/backwards-compat/backwards_compat/)
[![CI](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml/badge.svg)](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Declarative, compile-time verified schema versioning and backwards compatibility for [Serde](https://serde.rs/).

---

## Overview

In applications with evolving data formats (such as configuration files, databases, event logs, or network APIs), schemas change over time. Manually maintaining backwards compatibility usually requires writing verbose intermediary enums, custom deserializers, and error-prone migration glue code.

`backwards-compat` provides the `#[backwards_compat]` attribute macro to declaratively define schema transitions on `struct` and `enum` domain models—or on individual fields. It automatically generates `serde::Serialize`, `serde::Deserialize`, `BackwardsCompat`, and `BackwardsCompatDeserialize` implementations with streaming-first deserialization and always-inlined migration chains:
- **Streaming-First Deserialization**: Streams directly into the matched version's deserializer with zero buffering when the tag appears first, buffers only the prefix when the tag is out-of-order, fails fast on unknown/future/disabled versions, and runs the inlined migration chain to produce your target model.
- **Serialization**: Encodes your target model with zero-copy shadow wire borrowing and emits the native version tag automatically (with optional `#[serialize]` overrides for rolling deployments).
- **Field-Level Versioning**: Annotate individual struct or enum fields with `#[backwards_compat(...)]` (using bare `#[backwards_compat]` on an unversioned container, or combined with outer container versioning) with automatic adjacent tagging (`content = "data"` by default) and `#[with(...)]` support for primitives and foreign types.
- **Proxy Redirection**: Use `#[backwards_compat(via = ProxyType, ...)]` to decouple a domain model from its versioned serialized DTO while inheriting all `BackwardsCompat` and `Migrated<T>` capabilities automatically.
- **Introspection & Migration Tracking**: Exposes schema metadata via `BackwardsCompat`, supports opt-in source-version tracking via `Migrated<T>`, and inlines multi-step `From`, `TryFrom`, and `#[with(...)]` migrations directly into deserialization arms.

---

## Features

- **Streaming-First Deserialization**: Zero-buffer fast path when the version tag is first in the input, prefix-only buffering when out-of-order, direct version dispatch, single-pass untagged fallback, and fail-fast unknown/disabled version errors.
- **Structs & Enums**: Apply `#[backwards_compat]` to named structs, tuple structs, unit structs, or `enum` domain models.
- **Field-Level Versioning**: Version individual fields of structs or enum variants with `#[backwards_compat(...)]`, either standalone (with bare `#[backwards_compat]` on the container) or combined with outer container versioning—even when multiple fields share the same Rust type with different migration chains.
- **Unified Proxy Redirection (`#[backwards_compat(via = ...)]`)**: Redirect `Serialize`, `Deserialize`, `BackwardsCompat`, and `BackwardsCompatDeserialize` through a separate versioned proxy type with infallible or fallible (`try_from`, `try_into`, `fallible`) conversions, custom `from` / `to` functions, and zero-clone `&Self` support.
- **Native Integer & String Tags**: Emits native integer (`"version": 2`) or string (`"version": "2.0"`) tags with strict type matching, homogeneous inline `|` aliases (`"1" | "v1": ModelV1`, `1 | 10: ModelV1`), and `external` tagging for positional binary formats (`bincode`, `postcard`, `bitcode`).
- **Zero-Boilerplate & Zero-Copy**: Your target type does not need `#[derive(Serialize, Deserialize)]` or `Clone`—serialization borrows fields directly without cloning.
- **Metadata & Opt-In Migration Tracking**: Implements `BackwardsCompat` (`CURRENT_VERSION`, `SERIALIZE_VERSION`, `SUPPORTED_VERSIONS`, `ACCEPTS_UNTAGGED`, `TAGGING`) and `Migrated<T>` (`Deref`, `DerefMut`, `AsRef`, `is_untagged()`, `source_version()`, `map(...)`) to detect and inspect upgraded payloads.
- **Rolling Deployments & Runtime Version Selection**: Override the default wire version with `#[serialize]` / `#[serialize(with = ...)]`, or opt older versions into runtime selection with `#[serializable]` / `BackwardsCompatSerialize::as_version(...)`.
- **`no_std` + `alloc` & Optional `schemars`**: Compiles with `default-features = false` for `no_std` + `alloc` targets, and optionally derives `schemars::JsonSchema` (`features = ["schemars"]`).
- **Custom Converters (`#[with(fn_path)]`)**: Supply a custom conversion function per step instead of implementing `From` / `TryFrom`.
- **Generics & Lifetimes**: Full support for structs and enums with type parameters, lifetimes, const generics, and `where` clauses.
- **Adjacent Tagging**: Use `content = "..."` for tuple structs, scalar types, newtypes, and enums (field-level `#[backwards_compat(...)]` defaults to `content = "data"`).
- **Linear & DAG Migrations**: Support sequential version upgrades (`1 -> 2 -> ... -> N`) or DAG jumps (`1 => 3`, `1 => ModelV3`, `1 => Self`).
- **Always-Inlined Infallible & Fallible Migrations**: Seamless support for `From`, `#[fallible]` `TryFrom`, and `#[with(...)]` migrations inlined directly per version arm; fallible error types only need to implement `core::fmt::Display` (heterogeneous error types across steps require no unified error enum).
- **Multiple Unversioned (`_`) Schemas**: Accept one or more pre-backwards-compat unversioned schemas tried in declaration order (`#[serde(untagged)]`).
- **Feature-Gated Versions (`#[cfg(...)]`)**: Conditionally compile historical or unversioned (`_`) schema entries behind Cargo features or `#[cfg(...)]` predicates.
- **Format Agnostic**: Works out of the box with JSON, TOML, RON, YAML, and any other format supported by Serde.

---

## Installation

Add `backwards-compat` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
backwards-compat = "3.0"
serde = { version = "1.0", features = ["derive"] }
```

Optional Cargo features:
- `default-features = false`: Enables `#![no_std]` + `alloc` support (disables the default `std` feature).
- `features = ["schemars"]`: Enables [`schemars`](https://docs.rs/schemars) `JsonSchema` generation when `#[derive(..., JsonSchema)]` is used alongside `#[backwards_compat(...)]`.

---

## Quick Start: Sequential Infallible Migrations

Define previous versions with `Deserialize`, declare the active version using `N: Self` inside `versions(...)`, and implement `From`:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

// 1. Define historical schemas (only Deserialize is required)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
}

// 2. Define the current domain model (no #[derive(Serialize, Deserialize)] or Clone needed)
#[backwards_compat(tag = "version", versions(1: ConfigV1, 2: Self))]
#[derive(Debug, PartialEq)]
pub struct Config {
    pub name: String,
    pub port: u16,
}

// 3. Declare the migration from the historical version
impl From<ConfigV1> for Config {
    fn from(v1: ConfigV1) -> Self {
        Self {
            name: v1.name,
            port: 8080, // Default port for upgraded v1 configs
        }
    }
}

fn main() {
    // Deserializing an old v1 payload automatically upgrades it to Config:
    let legacy_json = r#"{"version": 1, "name": "my-service"}"#;
    let config: Config = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(config.port, 8080);

    // Serializing Config automatically tags it with native integer version 2:
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""version":2"#));

    // In-code manual conversion using From / Into:
    let v1 = ConfigV1 { name: "my-service".into() };
    let config_from: Config = Config::from(v1.clone());
    let config_into: Config = v1.into();
    assert_eq!(config_from, config);
    assert_eq!(config_into, config);
}
```

---

## Advanced Usage

### Field-Level Versioning

You can version individual fields of a `struct` or `enum` without versioning the entire container by placing a bare `#[backwards_compat]` on the container (alongside `#[derive(Serialize, Deserialize)]`) and `#[backwards_compat(...)]` on specific fields. Field-level attributes automatically default to adjacent tagging with `content = "data"` (overridable via `content = "..."`), which makes versioning primitives, foreign types (with `#[with(...)]`), `Option<T>`, `Vec<T>`, and nested structs seamless:

```rust
use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

fn parse_port(s: String) -> Result<u16, core::num::ParseIntError> {
    s.parse()
}

fn u8_to_bool(n: u8) -> bool {
    n != 0
}

#[backwards_compat]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub name: String,
    // Default adjacent tagging: {"version": 2, "data": 8080}
    #[backwards_compat(versions(
        #[fallible] #[with(parse_port)] 1: String,
        2: Self,
    ))]
    pub port: u16,
    // Custom tag and content field names: {"v": 2, "val": true}
    #[backwards_compat(
        tag = "v",
        content = "val",
        versions(
            #[with(u8_to_bool)] 1: u8,
            2: Self,
        )
    )]
    pub enabled: bool,
}

# fn main() {
let legacy = r#"{
    "name": "billing",
    "port": {"version": 1, "data": "8080"},
    "enabled": {"v": 1, "val": 1}
}"#;
let settings: AppSettings = serde_json::from_str(legacy).unwrap();
assert_eq!(settings, AppSettings { name: "billing".into(), port: 8080, enabled: true });

let json = serde_json::to_string(&settings).unwrap();
assert!(json.contains(r#""port":{"version":2,"data":8080}"#));
assert!(json.contains(r#""enabled":{"v":2,"val":true}"#));
# }
```

Field-level `#[backwards_compat(...)]` also composes with an outer versioned `#[backwards_compat(...)]` container (without requiring `Clone` on the outer type):

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ServerV1 {
    pub host: String,
    pub port: u16,
}

#[backwards_compat(tag = "schema", versions(1: ServerV1, 2: Self))]
#[derive(Debug, PartialEq)]
pub struct Server {
    pub host: String,
    #[backwards_compat(versions(
        #[with(u32::from)] 1: u16,
        2: Self,
    ))]
    pub port: u32,
}

impl From<ServerV1> for Server {
    fn from(v1: ServerV1) -> Self {
        Self { host: v1.host, port: u32::from(v1.port) }
    }
}

# fn main() {
// Upgrade outer v1 payload:
let from_v1: Server = serde_json::from_str(r#"{"schema":1,"host":"db","port":5432}"#).unwrap();
assert_eq!(from_v1, Server { host: "db".into(), port: 5432 });

// Upgrade inner field v1 inside outer v2 payload:
let from_v2: Server = serde_json::from_str(
    r#"{"schema":2,"host":"db","port":{"version":1,"data":5432}}"#,
).unwrap();
assert_eq!(from_v2, from_v1);
# }
```

### Proxy Redirection (`#[backwards_compat(via = ...)]`)

When you want to keep your domain model decoupled from its versioned wire representation (for example, keeping DTOs in a dedicated serialization module or enforcing domain invariants), apply `#[backwards_compat(via = ProxyType, ...)]` (or `#[backwards_compat(via(ProxyType), ...)]`). Your domain type automatically inherits `Serialize`, `Deserialize`, `BackwardsCompat`, `BackwardsCompatDeserialize`, and `Migrated<T>` from `ProxyType`:

```rust
use backwards_compat::{BackwardsCompat, Migrated, backwards_compat};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TournamentWireV1 {
    pub title: String,
}

#[backwards_compat(tag = "version", versions(1: TournamentWireV1, 2: Self))]
pub struct SerializedTournament {
    pub title: String,
    pub max_players: u32,
}

impl From<TournamentWireV1> for SerializedTournament {
    fn from(v1: TournamentWireV1) -> Self {
        Self { title: v1.title, max_players: 16 }
    }
}

// Domain model does not need #[derive(Clone, Serialize, Deserialize)].
#[backwards_compat(via = SerializedTournament)]
#[derive(Debug, PartialEq)]
pub struct Tournament {
    pub title: String,
    pub max_players: u32,
}

impl From<SerializedTournament> for Tournament {
    fn from(w: SerializedTournament) -> Self {
        Self { title: w.title, max_players: w.max_players }
    }
}

// Zero-clone serialization via From<&Tournament> (falls back to Clone + Into if omitted):
impl From<&Tournament> for SerializedTournament {
    fn from(t: &Tournament) -> Self {
        Self { title: t.title.clone(), max_players: t.max_players }
    }
}

# fn main() {
// Metadata and Migrated<T> delegate directly to SerializedTournament:
assert_eq!(Tournament::CURRENT_VERSION, 2);

let migrated: Migrated<Tournament> =
    serde_json::from_str(r#"{"version": 1, "title": "Spring Cup"}"#).unwrap();
assert!(migrated.was_migrated());
assert_eq!(migrated.source_version, Some(1));
let tournament = migrated.into_inner();
assert_eq!(tournament, Tournament { title: "Spring Cup".into(), max_players: 16 });

let json = serde_json::to_string(&tournament).unwrap();
assert!(json.contains(r#""version":2"#));
# }
```

`#[backwards_compat(via = ...)]` also supports fallible conversions (`try_from`, `try_into`, or `fallible` for both) and custom conversion functions (`from = fn_path` / `to = fn_path`):

```rust
use backwards_compat::backwards_compat;

#[backwards_compat(tag = "version", versions(1: Self))]
pub struct PortWire {
    pub port: u32,
}

fn decode_port(w: PortWire) -> Result<ValidPort, &'static str> {
    let port = u16::try_from(w.port).map_err(|_| "port out of range")?;
    if port == 0 {
        return Err("port cannot be zero");
    }
    Ok(ValidPort(port))
}

fn encode_port(p: &ValidPort) -> PortWire {
    PortWire { port: u32::from(p.0) }
}

#[backwards_compat(via = PortWire, try_from, from = decode_port, to = encode_port)]
#[derive(Debug, PartialEq)]
pub struct ValidPort(u16);

# fn main() {
let ok: ValidPort = serde_json::from_str(r#"{"version": 1, "port": 8080}"#).unwrap();
assert_eq!(ok, ValidPort(8080));
assert!(serde_json::from_str::<ValidPort>(r#"{"version": 1, "port": 0}"#).is_err());
# }
```

See the `#[backwards_compat]` macro documentation for the full option reference and conversion rules.

### Fallible Migrations (`#[fallible]`)

When migrations can fail validation, mark steps as `#[fallible]` and implement `TryFrom` (or a `Result`-returning `#[with(...)]` function); error types only need to implement `core::fmt::Display`:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error("Port {0} is out of range")]
    InvalidPort(u32),
    #[error("Validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfigV1 {
    pub port: u32,
}

#[backwards_compat(
    tag = "version",
    versions(
        #[fallible] 1: ServerConfigV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct ServerConfig {
    pub port: u16,
}

impl TryFrom<ServerConfigV1> for ServerConfig {
    type Error = ConfigError;

    fn try_from(v1: ServerConfigV1) -> Result<Self, Self::Error> {
        let port: u16 = v1.port.try_into().map_err(|_| ConfigError::InvalidPort(v1.port))?;
        if port == 0 {
            return Err(ConfigError::Validation("Port cannot be 0".into()));
        }
        Ok(Self { port })
    }
}
```

### `enum` Domain Models & Adjacent Tagging (`content = "..."`)

For tuple structs, scalar types, newtypes, and `enum` domain models, configure adjacent tagging with `content = "..."`:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
pub struct EventV1 {
    pub text: String,
}

#[backwards_compat(
    tag = "type",
    content = "payload",
    versions(1: EventV1, 2: Self)
)]
#[derive(Debug, PartialEq)]
pub enum Event {
    Ping,
    Message(String),
}

impl From<EventV1> for Event {
    fn from(v1: EventV1) -> Self {
        Self::Message(v1.text)
    }
}
```

### Custom Converters (`#[with(fn_path)]`) & DAG Transitions (`=> tag` / `=> Type` / `=> Self`)

Use `=> <target_version>`, `=> <TargetType>`, or `=> Self` to jump directly across versions, and `#[with(fn_path)]` to supply a custom conversion function instead of `From` / `TryFrom`. A type target resolves to the matching declared version's type (or the domain struct itself). Duplicate entries sharing a type (such as an unversioned `_` entry alongside a versioned entry) automatically inherit that type's explicit `=>` transition regardless of declaration order:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppConfigV1 {
    pub val: i32,
}

#[derive(Deserialize)]
pub struct AppConfigV2 {
    pub val: i32,
}

#[derive(Deserialize)]
pub struct AppConfigV3 {
    pub val: i32,
}

fn upgrade_v1(v1: AppConfigV1) -> AppConfigV3 {
    AppConfigV3 { val: v1.val * 10 }
}

impl From<AppConfigV2> for AppConfig {
    fn from(v2: AppConfigV2) -> Self {
        Self { val: v2.val + 1 }
    }
}

impl From<AppConfigV3> for AppConfig {
    fn from(v3: AppConfigV3) -> Self {
        Self { val: v3.val }
    }
}

#[backwards_compat(
    tag = "version",
    versions(
        _: AppConfigV1,                        // inherits `=> AppConfigV3` and `#[with(upgrade_v1)]` from `1`
        #[with(upgrade_v1)] 1: AppConfigV1 => AppConfigV3,
        2: AppConfigV2 => Self,
        3: AppConfigV3 => AppConfig,           // equivalent to `=> Self`
        4: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct AppConfig {
    pub val: i32,
}

# fn main() {
let from_untagged: AppConfig = serde_json::from_str(r#"{"val": 3}"#).unwrap();
assert_eq!(from_untagged, AppConfig { val: 30 });

let from_v1: AppConfig = serde_json::from_str(r#"{"version": 1, "val": 3}"#).unwrap();
assert_eq!(from_v1, AppConfig { val: 30 });

let from_v2: AppConfig = serde_json::from_str(r#"{"version": 2, "val": 3}"#).unwrap();
assert_eq!(from_v2, AppConfig { val: 4 });
# }
```

### Metadata (`BackwardsCompat`), Migration Tracking (`Migrated<T>`), & Rolling Deployments (`#[serialize]`)

Every `#[backwards_compat]` type implements `BackwardsCompat` and `BackwardsCompatDeserialize`. Deserialize into `Migrated<T>` to inspect the source version and detect whether a migration occurred, and mark an older version with `#[serialize]` (or `#[serialize(with = fn_path)]`) during rolling deployments:

```rust
use backwards_compat::{BackwardsCompat, Migrated, Tagging, backwards_compat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocV1 {
    pub title: String,
}

impl From<DocV1> for Doc {
    fn from(v1: DocV1) -> Self {
        Self { title: v1.title, rev: 1 }
    }
}

fn downgrade_to_v1(doc: &Doc) -> DocV1 {
    DocV1 { title: doc.title.clone() }
}

#[backwards_compat(
    tag = "version",
    versions(
        #[serialize(with = downgrade_to_v1)] 1: DocV1,
        #[current] 2: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct Doc {
    pub title: String,
    pub rev: u32,
}

// Introspect metadata:
assert_eq!(Doc::CURRENT_VERSION, 2);
assert_eq!(Doc::SERIALIZE_VERSION, 1);
assert_eq!(Doc::TAGGING, Tagging::Internal { tag: "version" });

// Track whether an incoming payload was upgraded:
let migrated: Migrated<Doc> = serde_json::from_str(r#"{"version": 1, "title": "hi"}"#).unwrap();
assert!(migrated.was_migrated());
assert!(!migrated.is_untagged());
assert_eq!(migrated.source_version(), Some(1));
assert_eq!(migrated.title, "hi"); // Deref / DerefMut / AsRef<Doc>
let upper = migrated.map(|mut d| {
    d.title = d.title.to_uppercase();
    d
});
```

### Runtime Version Selection (`BackwardsCompatSerialize` & `#[serializable]`)

When you want to keep serializing at `CURRENT_VERSION` by default while allowing callers to emit older wire versions on demand (for example, per-client API negotiation or targeted exports), annotate older version entries with `#[serializable]`, `#[serializable(with = fn_path)]`, or `#[serializable(try_with = fn_path)]`. This generates a `BackwardsCompatSerialize` implementation with `SERIALIZABLE_VERSIONS`, `serialize_as_version`, and `as_version(...)`:

```rust
use backwards_compat::{BackwardsCompatSerialize, backwards_compat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemV1 {
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemV2 {
    pub label: String,
    pub weight: u32,
}

impl From<ItemV1> for ItemV2 {
    fn from(v1: ItemV1) -> Self {
        Self { label: v1.label, weight: 1 }
    }
}

impl From<ItemV2> for Item {
    fn from(v2: ItemV2) -> Self {
        Self { label: v2.label, weight: v2.weight, active: true }
    }
}

fn to_v1(item: &Item) -> ItemV1 {
    ItemV1 { label: item.label.clone() }
}

fn try_to_v2(item: &Item) -> Result<ItemV2, &'static str> {
    if !item.active {
        return Err("inactive items cannot be downgraded to v2");
    }
    Ok(ItemV2 { label: item.label.clone(), weight: item.weight })
}

#[backwards_compat(
    tag = "version",
    versions(
        #[serializable(with = to_v1)] 1: ItemV1,
        #[serializable(try_with = try_to_v2)] 2: ItemV2,
        3: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub weight: u32,
    pub active: bool,
}

# fn main() {
let item = Item { label: "widget".into(), weight: 5, active: true };
assert_eq!(Item::SERIALIZABLE_VERSIONS, &[1, 2, 3]);

// Default Serialize still writes version 3, while as_version(...) picks v1 or v2 at runtime:
let v1_json = serde_json::to_string(&item.as_version(1)).unwrap();
assert_eq!(v1_json, r#"{"version":1,"label":"widget"}"#);

let v2_json = serde_json::to_string(&item.as_version(2)).unwrap();
assert_eq!(v2_json, r#"{"version":2,"label":"widget","weight":5}"#);
# }
```

- `#[serializable]` converts using `From<&Self>` (or `Clone` + `From<Self>`), `#[serializable(with = fn_path)]` calls an infallible `fn(&Self) -> OldType`, and `#[serializable(try_with = fn_path)]` calls a fallible `fn(&Self) -> Result<OldType, E>` where `E: core::fmt::Display`.

### One-Way Serialization or Deserialization (`serialize = false` / `deserialize = false`)

Both `serialize` and `deserialize` default to `true` on struct/enum-level `#[backwards_compat(...)]` (including `via = ...`). Set one to `false` when a type only needs one direction—for example, when fields only implement `Deserialize` or `Serialize`, or when you want to provide a manual or plain `#[derive(Serialize)]` / `#[derive(Deserialize)]` implementation for the disabled direction (disabling both is a compile-time error):

```rust
use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// Deserialize-only field (does not implement Serialize):
#[derive(Debug, PartialEq, Deserialize)]
pub struct IncomingToken(String);

#[derive(Deserialize)]
pub struct RequestV1 {
    pub token: IncomingToken,
}

#[backwards_compat(serialize = false, versions(1: RequestV1, 2: Self))]
#[derive(Debug, PartialEq)]
pub struct Request {
    pub token: IncomingToken,
    pub retries: u8,
}

impl From<RequestV1> for Request {
    fn from(v1: RequestV1) -> Self {
        Self { token: v1.token, retries: 3 }
    }
}

// Serialize-only field (does not implement Deserialize):
#[derive(Debug, PartialEq, Serialize)]
pub struct OutgoingMetric(u64);

#[backwards_compat(deserialize = false, versions(1: Self))]
#[derive(Debug, PartialEq)]
pub struct TelemetryEvent {
    pub latency_ms: OutgoingMetric,
}

# fn main() {
let req: Request = serde_json::from_str(r#"{"version": 1, "token": "abc"}"#).unwrap();
assert_eq!(req, Request { token: IncomingToken("abc".into()), retries: 3 });

let event = TelemetryEvent { latency_ms: OutgoingMetric(42) };
let json = serde_json::to_string(&event).unwrap();
assert_eq!(json, r#"{"version":1,"latency_ms":42}"#);
# }
```

### `no_std` + `alloc` Support

`backwards-compat` supports `#![no_std]` environments that provide `alloc`. Disable the default `std` feature in `Cargo.toml`:

```toml
[dependencies]
backwards-compat = { version = "3.0", default-features = false }
serde = { version = "1.0", default-features = false, features = ["derive", "alloc"] }
```

All macro-generated `Serialize`, `Deserialize`, `BackwardsCompat`, `BackwardsCompatDeserialize`, `BackwardsCompatSerialize`, and `Migrated<T>` implementations work without `std`.

### JSON Schema Generation (`schemars`)

Enable the `schemars` feature to generate versioned JSON Schemas reflecting every active version and tag format (`oneOf` across versions, internally or adjacently tagged):

```toml
[dependencies]
backwards-compat = { version = "3.0", features = ["schemars"] }
schemars = "1.0"
```

Place `#[derive(schemars::JsonSchema)]` below `#[backwards_compat(...)]` on your model (and on historical version types):

```rust
# #[cfg(feature = "schemars")]
# fn main() {
use backwards_compat::backwards_compat;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct PayloadV1 {
    pub name: String,
}

#[backwards_compat(tag = "version", versions(1: PayloadV1, 2: Self))]
#[derive(JsonSchema)]
pub struct Payload {
    pub name: String,
    pub count: u32,
}

impl From<PayloadV1> for Payload {
    fn from(v1: PayloadV1) -> Self {
        Self { name: v1.name, count: 0 }
    }
}

let schema = schema_for!(Payload);
let json = serde_json::to_string(&schema).unwrap();
assert!(json.contains("oneOf"));
# }
# #[cfg(not(feature = "schemars"))]
# fn main() {}
```

`Migrated<T>` also implements `schemars::JsonSchema` whenever `T: BackwardsCompat + JsonSchema`.

### Inline Tag Aliases (`|`) & String Version Keys

Use string keys (`"1.0"`, `"2.0"`) or combine multiple homogeneous wire tags with `|` (for example, accepting both `"1"` and `"v1"` string tags, or `1` and `10` integer tags):

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecordV1 {
    pub id: u64,
}

impl From<RecordV1> for Record {
    fn from(v1: RecordV1) -> Self {
        Self { id: v1.id, data: String::new() }
    }
}

#[backwards_compat(
    tag = "version",
    versions(
        "1" | "v1": RecordV1,
        "2" | "v2": Self,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub id: u64,
    pub data: String,
}
```

### Generics and Lifetimes

Domain models with lifetimes and generic parameters are supported out of the box:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
pub struct RecordV1<'a, T> {
    pub key: &'a str,
    pub val: T,
}

#[backwards_compat(
    tag = "version",
    versions(1: RecordV1<'a, T>, 2: Self)
)]
#[derive(Debug, PartialEq)]
pub struct Record<'a, T: Clone>
where
    T: std::fmt::Debug,
{
    pub key: &'a str,
    pub val: T,
    pub active: bool,
}

impl<'a, T: Clone> From<RecordV1<'a, T>> for Record<'a, T>
where
    T: std::fmt::Debug,
{
    fn from(v1: RecordV1<'a, T>) -> Self {
        Self { key: v1.key, val: v1.val, active: true }
    }
}
```

### Pre-Backwards-Compat (Unversioned) Schemas (`_: Type` / `_: Self`)

When introducing `#[backwards_compat]` to an existing model that was previously serialized without any version tag field, use `_` inside `versions(...)`. Multiple `_` entries are supported and tried in declaration order (like `#[serde(untagged)]`):

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigV0 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
    pub host: String,
}

impl From<ConfigV0> for ConfigV1 {
    fn from(v0: ConfigV0) -> Self {
        Self { name: v0.name, host: "localhost".into() }
    }
}

impl From<ConfigV1> for Config {
    fn from(v1: ConfigV1) -> Self {
        Self { name: v1.name, host: v1.host, port: 8080 }
    }
}

#[backwards_compat(
    tag = "version",
    versions(
        _: ConfigV1,
        _: ConfigV0,
        1: ConfigV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct Config {
    pub name: String,
    pub host: String,
    pub port: u16,
}
```

If your struct's fields haven't changed yet and you want to start writing `"version": 1` while still accepting legacy unversioned payloads, use `_: Self`:

```rust
use backwards_compat::backwards_compat;

#[backwards_compat(tag = "version", versions(_: Self, 1: Self))]
#[derive(Debug, PartialEq)]
pub struct Config {
    pub name: String,
    pub port: u16,
}
```

### Feature-Gated Versions (`#[cfg(...)]`)

Historical and unversioned (`_`) version entries can be gated behind `#[cfg(...)]` attributes so legacy wire types and migrations are only compiled when enabled:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[cfg(feature = "legacy-v1")]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
}

#[cfg(feature = "legacy-v1")]
impl From<ConfigV1> for Config {
    fn from(v1: ConfigV1) -> Self {
        Self { name: v1.name, port: 8080 }
    }
}

#[backwards_compat(
    tag = "version",
    versions(
        #[cfg(feature = "legacy-v1")] 1: ConfigV1,
        2: Self,
    )
)]
#[derive(Debug, PartialEq)]
pub struct Config {
    pub name: String,
    pub port: u16,
}
```

- Multiple `#[cfg(...)]` predicates on a single entry or chained along a migration path are automatically combined with `all(...)`.
- Neither the active `#[current]` version nor the `#[serialize]` version may be `#[cfg(...)]`-gated, and unconditional versions cannot migrate through a gated version (add `#[cfg(...)]` or use an explicit `=>` jump).

---

## Migrating from 1.x to 2.0

1. **Declare the current version inside `versions(...)`**: Top-level `version = N` was removed. Add `N: Self` (or mark an entry `#[current]`) inside `versions(...)`.
2. **Native integer tags**: Integer keys like `1` now serialize as native integers (`"version": 1`) instead of strings (`"version": "1"`). Use homogeneous integer or string aliases (e.g., `"1" | "v1": ModelV1` or `1 | 10: ModelV1`) when supporting multiple legacy tag values.
3. **Canonical entry directives**: Use `#[fallible]` (aliases `#[try_from]`, `#[try]`, etc. were removed) and `_` for unversioned entries (`none`/`None`/`untagged` aliases were removed).

---

## Macro Attributes Reference

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `tag` | string | `"version"` | Tag field name in serialized output (e.g. `tag = "version"` or `tag("version")`). |
| `content` | string | *none* (type-level) / `"data"` (field-level) | Content field name for adjacent tagging (e.g. `content = "data"`). Field-level `#[backwards_compat(...)]` defaults to `"data"`. |
| `via` | type path | *none* | Unified proxy redirection mode (`via = ProxyType` or `via(ProxyType)`), mutually exclusive with `tag`, `content`, and `versions`. Supports `try_from`, `try_into`, `fallible`, `from = fn_path`, `to = fn_path`, `serialize`, `deserialize`, and `dump`. |
| `serialize` | bool | `true` | Controls whether `Serialize` and `BackwardsCompatSerialize` (and the borrowed shadow wire type) are generated (`serialize = false` or `serialize(false)`). Struct/enum-level only; cannot be combined with `deserialize = false`. |
| `deserialize` | bool | `true` | Controls whether `Deserialize` and `BackwardsCompatDeserialize` (and migration chains) are generated (`deserialize = false` or `deserialize(false)`). Struct/enum-level only; cannot be combined with `serialize = false`. |
| `dump` | flag | *disabled* | Prints the generated Rust token stream to `stderr` during compilation. |
| `versions` | list / map | *empty* | Version entries supporting `#[current]`, `#[serialize]`, `#[serialize(with = fn)]`, `#[serializable]`, `#[serializable(with = fn)]`, `#[serializable(try_with = fn)]`, `#[fallible]`, `#[with(fn)]`, `#[cfg(...)]`, `\|` aliases, `_`, and `=> target` / `=> TargetType` / `=> Self`. |

---

## How It Works

Under the hood, `#[backwards_compat]` generates:
1. **Streaming-First Deserialization**:
   - **Zero-buffer fast path**: When the version tag field appears first in a map or adjacent sequence, the deserializer dispatches directly into the matched version's `Deserialize` implementation without buffering the payload—preserving zero-copy `&'de str` borrowing.
   - **Prefix-only slow path**: When other fields appear before the version tag, only the preceding prefix entries are buffered and replayed ahead of the remaining live stream.
   - **Direct version dispatch & fail-fast errors**: Native integer (`u64`) and string (`&str`) tags dispatch directly to each version's deserialization arm (no intermediate helper enum), failing immediately on unknown, future, or `#[cfg]`-disabled versions without reading the rest of the payload.
   - **Single-pass untagged fallback**: When `_` entries are declared and the tag field is absent, the already-collected payload is tried once per `_` entry in declaration order via `ContentRefDeserializer`.
2. **Always-Inlined Migrations**: Multi-step `From`, `#[fallible]` `TryFrom`, and `#[with(...)]` chains are inlined directly inside each version's deserialization arm, so fallible steps only require `core::fmt::Display` and can mix distinct error types freely.
3. **Zero-Copy Native-Tag Serialization**: Emits a private `__SerializeHelper` struct holding the native `u64` or `&'static str` tag field alongside a borrowed reference to your wire struct/enum, guaranteeing zero clones.
4. **Field-Level Helpers & Proxy Redirection**: Field-level `#[backwards_compat(...)]` emits private same-scope per-field Serde helpers (`serialize`, `serialize_ref`, `deserialize`) so multiple fields of the same type can have independent chains with zero trait collisions and zero-clone serialization. `#[backwards_compat(via = ProxyType, ...)]` delegates `Serialize`, `Deserialize`, `BackwardsCompat`, and `BackwardsCompatDeserialize` directly through `ProxyType`.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
