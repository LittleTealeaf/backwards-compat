# backwards-compat

[![Crates.io](https://img.shields.io/crates/v/backwards-compat.svg)](https://crates.io/crates/backwards-compat)
[![Documentation](https://img.shields.io/badge/docs-github_pages-blue.svg)](https://littletealeaf.github.io/backwards-compat/backwards_compat/)
[![CI](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml/badge.svg)](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Declarative, compile-time verified schema versioning and backwards compatibility for [Serde](https://serde.rs/).

---

## Overview

In applications with evolving data formats (such as configuration files, databases, event logs, or network APIs), schemas change over time. Manually maintaining backwards compatibility usually requires writing verbose intermediary enums, custom deserializers, and error-prone migration glue code.

`backwards-compat` provides the `#[backwards_compat]` attribute macro to declaratively define schema transitions. It automatically generates `serde::Serialize` and `serde::Deserialize` implementations directly for your target domain model, as well as `From<V>` and `TryFrom<V>` implementations for manual in-code upgrades:
- **Deserialization**: Detects the incoming schema version tag, deserializes into the matching versioned struct, and automatically runs the migration chain to produce your target model.
- **Serialization**: Encodes your target model using the current active schema version and injects the version tag automatically.
- **In-Code Conversion**: Converts legacy schema struct instances directly into the target model via `From::from` / `.into()` (or `TryFrom::try_from` / `.try_into()`).

---

## Features

- **Zero-Boilerplate Domain Models**: Your target struct does not need `#[derive(Serialize, Deserialize)]`—the attribute macro generates them directly.
- **Zero-Copy Serialization**: Domain structs do not need `Clone`. Serialization borrows fields directly without cloning.
- **Generics & Lifetimes**: Full support for structs with type parameters, lifetimes, and `where` clauses (e.g. `struct Model<'a, T>`).
- **Adjacent Tagging**: Use `content = "..."` for tuple structs, scalar types, and newtypes that Serde cannot internally tag.
- **Implicit Wire Structs**: The `#[backwards_compat(...)]` attribute implicitly creates a shadow wire struct if your target struct is the latest version, letting you skip declaring a final schema struct entirely.
- **Direct In-Code Conversions**: Automatically implements `From<V>` (or `TryFrom<V>`) on your target domain model for every declared historical version `V`, allowing `TargetModel::from(v1)` or `v1.into()`.
- **Linear & DAG Migrations**: Support straightforward sequential version upgrades (`1 -> 2 -> ... -> N`) or complex Directed Acyclic Graph (DAG) upgrades (e.g. `1 => 3`).
- **Infallible & Fallible Migrations**: Seamless support for infallible migrations using `From` as well as fallible migrations using `TryFrom` with custom error types (`#[fallible]`, `#[try]`, `#[try_from]`, `#[try_into]`).
- **Flexible Versioning**: Supports integer version tags (`1`, `2`, `3`) or string keys (`"1.0"`, `"2.0"`).
- **Custom Tag Field**: Configure any version field name (e.g., `version`, `schema_version`, `_v`).
- **Format Agnostic**: Works out of the box with JSON, TOML, RON, YAML, and any other format supported by Serde.

---

## Installation

Add `backwards-compat` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
backwards-compat = "1.0"
serde = { version = "1.0", features = ["derive"] }
```

---

## Quick Start: Sequential Infallible Migrations

Define previous versions of your struct with standard Serde derives, implement `From` transitions between versions, and use `#[backwards_compat(...)]` on your target model:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

// 1. Define historical schemas (only Deserialize is required)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
}

// 2. Define the current domain model (no #[derive(Serialize, Deserialize)] or Clone needed)
// The macro automatically implements Serialize and Deserialize, and generates an
// implicit zero-copy wire struct for serialization logic.
#[backwards_compat(tag = "version", version = 2, versions(1: ConfigV1))]
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
    let legacy_json = r#"{"version": "1", "name": "my-service"}"#;
    let config: Config = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(config.port, 8080);

    // Serializing Config automatically tags it with current version 2:
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains(r#""version":"2""#));

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

### Fallible Migrations (`TryFrom` + Custom Error)

When migrations can fail validation, mark steps as `#[fallible]` (or `#[try]`, `#[try_from]`, `#[try_into]`), specify the custom error type in the attribute (`error = ...`), and implement `TryFrom`:

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
    version = 2,
    error = ConfigError,
    versions(#[try_from] 1: ServerConfigV1)
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

fn main() {
    // In-code manual conversion using TryFrom / TryInto:
    let v1 = ServerConfigV1 { port: 8080 };
    let config: ServerConfig = ServerConfig::try_from(v1.clone()).unwrap();
    let config_into: ServerConfig = v1.try_into().unwrap();
    assert_eq!(config, config_into);
}
```

### Adjacent Tagging (`content = "..."`)

For tuple structs, primitive representations, or scalar types where Serde cannot use internal tagging, specify `content = "..."`:

```rust
use backwards_compat::backwards_compat;
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
pub struct ScalarV1(pub u32);

#[backwards_compat(
    tag = "type",
    content = "payload",
    version = 2,
    versions(1: ScalarV1)
)]
#[derive(Debug, PartialEq)]
pub struct ScalarTarget(pub String);

impl From<ScalarV1> for ScalarTarget {
    fn from(v1: ScalarV1) -> Self {
        Self(v1.0.to_string())
    }
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
    version = 2,
    versions(1: RecordV1<'a, T>)
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
        Self {
            key: v1.key,
            val: v1.val,
            active: true,
        }
    }
}
```

### Directed Acyclic Graph (DAG) Migrations

If a legacy schema can fast-track directly to a future schema rather than walking through every intermediate version, define transitions with `=> <target_version>`:

```rust
#[backwards_compat(tag = "version", version = 3, versions(1: AppConfigV1 => 3, 2: AppConfigV2 => 3))]
pub struct AppConfig {
    // ...
}
```

### String Version Keys

You can use string keys (such as SemVer strings) instead of integer versions:

```rust
#[backwards_compat(
    tag = "schema_version",
    version = "2.0",
    versions(
        "0.1": RecordV0_1,
        "1.0": RecordV1_0,
    )
)]
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseRecord {
    pub id: u64,
    pub data: String,
}
```

---

## Macro Attributes Reference

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `tag` | string | `"version"` | Tag field name in serialized output (e.g. `tag = "version"` or `tag("version")`). |
| `content` | string | *none* | Content field name for adjacent tagging (e.g. `content = "data"`). |
| `version` | int / string | *highest version* | Target schema version (e.g. `version = 2` or `version = "2.0"`). |
| `error` | path | `Box<dyn Error + Send + Sync>` | Error type for `TryFrom` migrations (e.g. `error = MyError`). |
| `dump` | flag | *disabled* | Prints the generated Rust token stream to `stderr` during compilation. |
| `versions` | list / map | *empty* | List of version mappings (`versions(1: V1 => 2, #[try_from] 2: V2)`). |

---

## How It Works

Under the hood, `#[backwards_compat]` constructs an internal, private enum representing all declared schema versions with `#[serde(tag = ...)]` (or `#[serde(tag = ..., content = ...)]`).

1. **On Deserialization**: Serde inspects the tag field and deserializes the payload into the appropriate version variant. The macro then traverses the shortest migration path in the dependency graph using your `From` or `TryFrom` implementations until it produces the target domain model.
2. **On Serialization**: The macro serializes your target domain model using zero-copy shadow wire references, guaranteeing zero clones while ensuring that the version tag is present.
3. **In-Code Conversions**: The macro generates `From<V> for TargetModel` (or `TryFrom<V> for TargetModel`) for all declared versions `V` by composing the transition steps along the shortest path.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
