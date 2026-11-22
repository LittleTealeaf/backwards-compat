# backwards-compat 

[![Crates.io](https://img.shields.io/crates/v/backwards-compat.svg)](https://crates.io/crates/backwards-compat)
[![Documentation](https://img.shields.io/badge/docs-github_pages-blue.svg)](https://littletealeaf.github.io/backwards-compat/backwards_compat/)
[![CI](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml/badge.svg)](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Declarative, compile-time verified schema versioning and backwards compatibility for [Serde](https://serde.rs/).

---

## Overview

In applications with evolving data formats (such as configuration files, databases, event logs, or network APIs), schemas change over time. Manually maintaining backwards compatibility usually requires writing verbose intermediary enums, custom deserializers, and error-prone migration glue code.

`backwards-compat` provides the `#[backwards_compat]` attribute macro to declaratively define schema transitions on `struct` and `enum` domain models. It automatically generates `serde::Serialize`, `serde::Deserialize`, `BackwardsCompat`, `BackwardsCompatDeserialize`, and transitive `From<V>` / `TryFrom<V>` implementations:
- **Deserialization**: Strictly matches native integer (`1`) or string (`"1.0"`) version tags, deserializes into the matching versioned wire type, and runs the migration chain to produce your target model (with rich errors for unknown, future, or feature-disabled versions).
- **Serialization**: Encodes your target model with zero-copy shadow wire borrowing and emits the native version tag automatically (with optional `#[serialize]` overrides for rolling deployments).
- **In-Code Conversion & Introspection**: Converts multi-step legacy schema structs via `From` / `TryFrom`, exposes schema metadata via `BackwardsCompat`, and supports opt-in source-version tracking via `Migrated<T>`.

---

## Features

- **Structs & Enums**: Apply `#[backwards_compat]` to named structs, tuple structs, unit structs, or `enum` domain models.
- **Native Integer & String Tags**: Emits native integer (`"version": 2`) or string (`"version": "2.0"`) tags with strict type matching, plus inline `|` aliases (`1 | "1": ModelV1`) for mixed or legacy tag compatibility.
- **Zero-Boilerplate & Zero-Copy**: Your target type does not need `#[derive(Serialize, Deserialize)]` or `Clone`—serialization borrows fields directly without cloning.
- **Metadata & Opt-In Migration Tracking**: Implements `BackwardsCompat` (`CURRENT_VERSION`, `SERIALIZE_VERSION`, `SUPPORTED_VERSIONS`, `ACCEPTS_UNTAGGED`) and `Migrated<T>` to detect when payloads were upgraded.
- **Rolling Deployments (`#[serialize]`)**: Override the serialized wire version (`#[serialize]` or `#[serialize(with = fn_path)]`) while already accepting newer incoming schemas.
- **Custom Converters (`#[with(fn_path)]`)**: Supply a custom conversion function per step instead of implementing `From` / `TryFrom`.
- **Generics & Lifetimes**: Full support for structs and enums with type parameters, lifetimes, and `where` clauses.
- **Adjacent Tagging**: Use `content = "..."` for tuple structs, scalar types, newtypes, and enums.
- **Linear & DAG Migrations**: Support sequential version upgrades (`1 -> 2 -> ... -> N`) or DAG jumps (`1 => 3`, `1 => Self`).
- **Infallible & Fallible Migrations**: Seamless support for `From` and `#[fallible]` `TryFrom` migrations with custom error types.
- **Multiple Unversioned (`_`) Schemas**: Accept one or more pre-backwards-compat unversioned schemas tried in declaration order (`#[serde(untagged)]`).
- **Feature-Gated Versions (`#[cfg(...)]`)**: Conditionally compile historical or unversioned (`_`) schema entries behind Cargo features or `#[cfg(...)]` predicates.
- **Format Agnostic**: Works out of the box with JSON, TOML, RON, YAML, and any other format supported by Serde.

---

## Installation

Add `backwards-compat` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
backwards-compat = "2.0"
serde = { version = "1.0", features = ["derive"] }
```

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

### Fallible Migrations (`#[fallible]` + Custom Error)

When migrations can fail validation, mark steps as `#[fallible]`, specify `error = ...`, and implement `TryFrom`:

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
    error = ConfigError,
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

### Custom Converters (`#[with(fn_path)]`) & DAG Transitions (`=> tag` / `=> Self`)

Use `=> <target_version>` or `=> Self` to jump directly across versions, and `#[with(fn_path)]` to supply a custom conversion function instead of `From` / `TryFrom`:

```rust
fn upgrade_v1(v1: AppConfigV1) -> AppConfig {
    AppConfig { /* ... */ }
}

#[backwards_compat(
    tag = "version",
    versions(
        #[with(upgrade_v1)] 1: AppConfigV1 => Self,
        2: AppConfigV2 => Self,
        3: Self,
    )
)]
pub struct AppConfig {
    // ...
}
```

### Metadata (`BackwardsCompat`), Migration Tracking (`Migrated<T>`), & Rolling Deployments (`#[serialize]`)

Every `#[backwards_compat]` type implements `BackwardsCompat` and `BackwardsCompatDeserialize`. Deserialize into `Migrated<T>` to inspect the source version and detect whether a migration occurred, and mark an older version with `#[serialize]` (or `#[serialize(with = fn_path)]`) during rolling deployments:

```rust
use backwards_compat::{BackwardsCompat, Migrated, backwards_compat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocV1 {
    pub title: String,
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

// Track whether an incoming payload was upgraded:
let migrated: Migrated<Doc> = serde_json::from_str(r#"{"version": 1, "title": "hi"}"#).unwrap();
assert!(migrated.was_migrated());
assert_eq!(migrated.source_version, Some(1));
```

### Inline Tag Aliases (`|`) & String Version Keys

Use string keys (`"1.0"`, `"2.0"`) or combine multiple wire tags with `|` (for example, accepting both native integer tags and legacy 1.x stringified integer tags):

```rust
#[backwards_compat(
    tag = "version",
    versions(
        1 | "1": RecordV1,
        2 | "2": Self,
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
2. **Native integer tags**: Integer keys like `1` now serialize as native integers (`"version": 1`) instead of strings (`"version": "1"`). To keep accepting 1.x stringified integer payloads during migration, use inline aliases: `1 | "1": ModelV1, 2 | "2": Self`.
3. **Canonical entry directives**: Use `#[fallible]` (aliases `#[try_from]`, `#[try]`, etc. were removed) and `_` for unversioned entries (`none`/`None`/`untagged` aliases were removed).

---

## Macro Attributes Reference

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `tag` | string | `"version"` | Tag field name in serialized output (e.g. `tag = "version"` or `tag("version")`). |
| `content` | string | *none* | Content field name for adjacent tagging (e.g. `content = "data"`). |
| `error` | path | `Box<dyn Error + Send + Sync>` | Error type for `TryFrom` migrations (e.g. `error = MyError`). |
| `dump` | flag | *disabled* | Prints the generated Rust token stream to `stderr` during compilation. |
| `versions` | list / map | *empty* | Version entries supporting `#[current]`, `#[serialize]`, `#[serialize(with = fn)]`, `#[fallible]`, `#[with(fn)]`, `#[cfg(...)]`, `\|` aliases, `_`, and `=> target` / `=> Self`. |

---

## How It Works

Under the hood, `#[backwards_compat]` generates:
1. **Strict Native-Tag Deserialization**: Buffers the incoming payload once, extracts the version tag field, strictly dispatches integer or string tags to an externally-tagged `__VersionHelper` via `VariantContentDeserializer` (or falls back to `#[serde(untagged)] enum __UntaggedHelper` when `_` entries are present), and runs the resolved migration path.
2. **Zero-Copy Native-Tag Serialization**: Emits a private `__SerializeHelper` struct holding the native `u64` or `&'static str` tag field alongside a borrowed reference to your wire struct/enum, guaranteeing zero clones.
3. **Metadata & Conversions**: Implements `BackwardsCompat`, `BackwardsCompatDeserialize`, `serde::Deserialize`, `serde::Serialize`, and transitive `From<V>` / `TryFrom<V>` impls for multi-step historical versions.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
