//! # backwards-compat
//!
//! Declarative, compile-time verified schema versioning and backwards compatibility for Serde.
//!
//! ## Overview
//!
//! In applications with evolving data formats, schemas change across versions.
//! `backwards-compat` streamlines schema evolution by automatically implementing
//! `serde::Serialize` and `serde::Deserialize` directly for your target model.
//!
//! You declare previous schema versions and their transitions using the `compat TargetModel` syntax:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct ConfigV1 {
//!     pub name: String,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct ConfigV2 {
//!     pub name: String,
//!     pub port: u16,
//! }
//!
//! impl From<ConfigV1> for ConfigV2 {
//!     fn from(v1: ConfigV1) -> Self {
//!         Self {
//!             name: v1.name,
//!             port: 8080,
//!         }
//!     }
//! }
//!
//! // The target domain model Config does NOT need #[derive(Serialize, Deserialize)].
//! // backwards_compat! generates Serialize and Deserialize implementations automatically.
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct Config {
//!     pub name: String,
//!     pub port: u16,
//! }
//!
//! impl From<ConfigV2> for Config {
//!     fn from(v2: ConfigV2) -> Self {
//!         Self {
//!             name: v2.name,
//!             port: v2.port,
//!         }
//!     }
//! }
//!
//! impl From<Config> for ConfigV2 {
//!     fn from(c: Config) -> Self {
//!         Self {
//!             name: c.name,
//!             port: c.port,
//!         }
//!     }
//! }
//!
//! backwards_compat! {
//!     #[tag = "version", version = 2]
//!     compat Config {
//!         1: ConfigV1,
//!         2: ConfigV2,
//!     }
//! }
//!
//! // Deserializing {"version": "1", "name": "my-app"} yields Config:
//! let json_v1 = r#"{"version": "1", "name": "my-app"}"#;
//! let config: Config = serde_json::from_str(json_v1).unwrap();
//! assert_eq!(config.port, 8080);
//!
//! // Serializing Config automatically tags it with the current version:
//! let serialized = serde_json::to_string(&config).unwrap();
//! assert!(serialized.contains(r#""version":"2""#));
//! ```
//!
//! ## DAG Transitions
//!
//! By default, versions upgrade sequentially (`1 -> 2 -> ... -> N`).
//! You can also define custom upgrade graphs with `=> <next_version>`:
//!
//! ```rust
//! # use backwards_compat::backwards_compat;
//! # use serde::{Deserialize, Serialize};
//! # #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! # pub struct V1 { pub val: i32 }
//! # #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! # pub struct V2 { pub val: i32 }
//! # #[derive(Debug, Clone, PartialEq)]
//! # pub struct V3 { pub val: i32 }
//! # impl From<V1> for V3 { fn from(v: V1) -> Self { Self { val: v.val } } }
//! # impl From<V2> for V3 { fn from(v: V2) -> Self { Self { val: v.val } } }
//! backwards_compat! {
//!     #[tag = "version", version = 3]
//!     compat V3 {
//!         1: V1 => 3,
//!         2: V2 => 3,
//!     }
//! }
//! ```

pub use backwards_compat_derive::backwards_compat;
