//! # backwards-compat
//!
//! Declarative, compile-time verified schema versioning and backwards compatibility for Serde.
//!
//! ## Overview
//!
//! In applications with evolving data formats, schemas change across versions.
//! `backwards-compat` streamlines the creation of versioning enums and migration pipelines.
//!
//! You declare the sequence of versions using `v1 = <Type>, v2 = <Type>, ...` in a `{}` block.
//! The macro verifies at compile time that each version implements `Into` (or `TryInto`) for
//! the next version in sequence, and generates:
//! - The version enum with Serde serialization & deserialization support
//! - Tagged version matching (`#[serde(tag = "version")]`)
//! - Optional fallback to untagged deserialization for legacy unversioned data
//! - Sequential upgrade chains: older versions automatically convert forward to the latest or target type
//! - `From` and `TryFrom` conversions for seamless use with `#[serde(from = "...")]` or `#[serde(try_from = "...")]`
//!
//! ## Example: Basic Tagged Versioning
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
//! backwards_compat! {
//!     pub enum ConfigVersion {
//!         v1 = ConfigV1,
//!         v2 = ConfigV2,
//!     }
//! }
//!
//! // Deserializing {"version": "1", "name": "app"} yields ConfigV2 via ConfigVersion:
//! let json_v1 = r#"{"version": "1", "name": "my-app"}"#;
//! let versioned: ConfigVersion = serde_json::from_str(json_v1).unwrap();
//! let config: ConfigV2 = versioned.upgrade();
//! assert_eq!(config.port, 8080);
//! ```
//!
//! ## Example: Untagged Fallback (Hybrid)
//!
//! For schemas where legacy data did not include a `"version"` tag:
//!
//! ```rust
//! use backwards_compat::backwards_compat;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct V1 {
//!     pub host: String,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! pub struct V2 {
//!     pub host: String,
//!     pub timeout_sec: u32,
//! }
//!
//! impl From<V1> for V2 {
//!     fn from(v1: V1) -> Self {
//!         Self { host: v1.host, timeout_sec: 30 }
//!     }
//! }
//!
//! backwards_compat! {
//!     #[tag = "version"]
//!     pub enum AppConfig {
//!         #[untagged]
//!         v1 = V1,
//!         v2 = V2,
//!     }
//! }
//!
//! // Untagged legacy json parses as V1 and upgrades to V2:
//! let legacy_json = r#"{"host": "localhost"}"#;
//! let config: AppConfig = serde_json::from_str(legacy_json).unwrap();
//! let current: V2 = config.upgrade();
//! assert_eq!(current.timeout_sec, 30);
//! ```

pub use backwards_compat_derive::backwards_compat;
