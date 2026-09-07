# backwards-compat

[![Crates.io](https://img.shields.io/crates/v/backwards-compat.svg)](https://crates.io/crates/backwards-compat)
[![Documentation](https://img.shields.io/badge/docs-github_pages-blue.svg)](https://littletealeaf.github.io/backwards-compat/backwards_compat/)
[![CI](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml/badge.svg)](https://github.com/LittleTealeaf/backwards-compat/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Declarative, compile-time verified schema versioning and backwards compatibility for [Serde](https://serde.rs/).

---

## Overview

In applications with evolving data formats (such as configuration files, databases, event logs, or network APIs), schemas change over time. Manually maintaining backwards compatibility usually requires writing verbose intermediary enums, custom deserializers, and error-prone migration glue code.

`backwards-compat` provides the `backwards_compat!` macro to declaratively define schema transitions. It automatically generates `serde::Serialize` and `serde::Deserialize` implementations directly for your target domain model:
- **Deserialization**: Detects the incoming schema version tag, deserializes into the matching versioned struct, and automatically runs the migration chain to produce your target model.
- **Serialization**: Encodes your target model using the current active schema version and injects the version tag automatically.

---

## Features

- **Zero-Boilerplate Domain Models**: Your target struct does not need `#[derive(Serialize, Deserialize)]`—the macro generates them directly.
- **Linear & DAG Migrations**: Support straightforward sequential version upgrades (`1 -> 2 -> ... -> N`) or complex Directed Acyclic Graph (DAG) upgrades (e.g. `1 => 3`).
- **Infallible & Fallible Migrations**: Seamless support for infallible migrations using `From` as well as fallible migrations using `TryFrom` with custom error types (`#[fallible]`).
- **Flexible Versioning**: Supports integer version tags (`1`, `2`, `3`) or string keys (`"1.0"`, `"2.0"`).
- **Custom Tag Field**: Configure any version field name (e.g., `version`, `schema_version`, `_v`).
- **Format Agnostic**: Works out of the box with JSON, TOML, RON, YAML, and any other format supported by Serde.

---

## Installation

Add `backwards-compat` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
backwards-compat = "0.2"
serde = { version = "1.0", features = ["derive"] }
```

---

## Quick Start: Sequential Infallible Migrations

Define previous versions of your struct with standard Serde derives, implement `From` transitions between versions, and use `backwards_compat!` on your target model:

```rust
use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};

// 1. Define legacy schemas
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigV1 {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigV2 {
    pub name: String,
    pub port: u16,
}

impl From<ConfigV1> for ConfigV2 {
    fn from(v1: ConfigV1) -> Self {
        Self {
            name: v1.name,
            port: 8080, // Default port for upgraded v1 configs
        }
    }
}

// 2. Define the current domain model (no #[derive(Serialize, Deserialize)] needed)
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub name: String,
    pub port: u16,
}

impl From<ConfigV2> for Config {
    fn from(v2: ConfigV2) -> Self {
        Self {
            name: v2.name,
            port: v2.port,
        }
    }
}

impl From<Config> for ConfigV2 {
    fn from(c: Config) -> Self {
        Self {
            name: c.name,
            port: c.port,
        }
    }
}

// 3. Declare the compatibility chain
backwards_compat! {
    #[tag = "version", version = 2]
    compat Config {
        1: ConfigV1,
        2: ConfigV2,
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
}
```

---

## Advanced Usage

### Fallible Migrations (`TryFrom` + Custom Error)

When migrations can fail validation, mark steps as `#[fallible]`, specify the custom error type in the attribute (`error = ...`), and implement `TryFrom`:

```rust
use backwards_compat::backwards_compat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error("Port {0} is out of range")]
    InvalidPort(u32),
    #[error("Validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigV1 {
    pub port: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigV2 {
    pub port: u16,
}

impl TryFrom<ServerConfigV1> for ServerConfigV2 {
    type Error = ConfigError;

    fn try_from(v1: ServerConfigV1) -> Result<Self, Self::Error> {
        let port: u16 = v1.port.try_into().map_err(|_| ConfigError::InvalidPort(v1.port))?;
        Ok(Self { port })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub port: u16,
}

impl TryFrom<ServerConfigV2> for ServerConfig {
    type Error = ConfigError;

    fn try_from(v2: ServerConfigV2) -> Result<Self, Self::Error> {
        if v2.port == 0 {
            return Err(ConfigError::Validation("Port cannot be 0".into()));
        }
        Ok(Self { port: v2.port })
    }
}

impl From<ServerConfig> for ServerConfigV2 {
    fn from(s: ServerConfig) -> Self {
        Self { port: s.port }
    }
}

backwards_compat! {
    #[tag = "version", version = 2, error = ConfigError]
    compat ServerConfig {
        #[fallible] 1: ServerConfigV1,
        #[fallible] 2: ServerConfigV2,
    }
}
```

### Directed Acyclic Graph (DAG) Migrations

If a legacy schema can fast-track directly to a future schema rather than walking through every intermediate version, define transitions with `=> <target_version>`:

```rust
backwards_compat! {
    #[tag = "version", version = 3]
    compat AppConfig {
        1: AppConfigV1 => 3, // Bypasses version 2 directly to 3
        2: AppConfigV2 => 3,
        3: AppConfigV3,
    }
}
```

### String Version Keys

You can use string keys (such as SemVer strings) instead of integer versions:

```rust
backwards_compat! {
    #[tag = "schema_version", version = "2.0"]
    compat DatabaseRecord {
        "0.1": RecordV0_1,
        "1.0": RecordV1_0,
        "2.0": RecordV2_0,
    }
}
```

---

## How It Works

Under the hood, `backwards_compat!` constructs an internal, private enum representing all declared schema versions with `#[serde(tag = ...)]`.

1. **On Deserialization**: Serde inspects the tag field and deserializes the payload into the appropriate version variant. The macro then traverses the shortest migration path in the dependency graph using your `From` or `TryFrom` implementations until it produces the target domain model.
2. **On Serialization**: The macro converts your target domain model into the designated target version and serializes it, ensuring that the version tag is present.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
