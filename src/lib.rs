//! # backwards-compat
//!
//! Declarative, compile-time verified schema versioning and backwards compatibility for Serde and domain models.
//!
//! ## Overview
//!
//! In applications with evolving data formats, schemas change across versions.
//! `backwards-compat` streamlines schema evolution by:
//! 1. Automatically implementing `serde::Serialize` and `serde::Deserialize` directly for your target model.
//! 2. Automatically implementing `From<V>` (or `TryFrom<V>`) for your target model from every declared historical version `V`, enabling effortless in-code migrations.
//! 3. Implicitly generating wire structs, avoiding manual serialization boilerplate for the latest version.
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
//! // backwards_compat generates Serialize and Deserialize implementations automatically,
//! // as well as an implicit ConfigShadowWire struct if needed.
//! // It also automatically creates From<ConfigV1> to Config conversions based on the chain.
//! #[backwards_compat(tag = "version", version = 2, versions(1: ConfigV1))]
//! #[derive(Debug, Clone, PartialEq)]
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
//! // 1. Deserializing legacy serialized data upgrades automatically:
//! let json_v1 = r#"{"version": "1", "name": "my-app"}"#;
//! let config: Config = serde_json::from_str(json_v1).unwrap();
//! assert_eq!(config.port, 8080);
//!
//! // 2. Serializing Config automatically tags it with the current version:
//! let serialized = serde_json::to_string(&config).unwrap();
//! assert!(serialized.contains(r#""version":"2""#));
//!
//! // 3. In-code manual conversion using From / Into:
//! let v1 = ConfigV1 { name: "my-app".into() };
//! let from_v1 = Config::from(v1.clone());
//! let into_config: Config = v1.into();
//! assert_eq!(from_v1, config);
//! assert_eq!(into_config, config);
//! ```
//!
//! ## Fallible In-Code Conversion (`TryFrom`)
//!
//! When schema transitions are marked `#[fallible]`, `backwards_compat` generates `TryFrom<V>`
//! implementations for the target model:
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
//! #[backwards_compat(tag = "version", version = 2, error = CustomError, versions(#[fallible] 1: ServerV1))]
//! #[derive(Debug, Clone, PartialEq)]
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
//! ## DAG Transitions
//!
//! By default, versions upgrade sequentially (`1 -> 2 -> ... -> N`).
//! You can also define custom upgrade graphs with `=> <next_version>`:
//!
//! ```rust
//! # use backwards_compat::backwards_compat;
//! # use serde::Deserialize;
//! # #[derive(Debug, Clone, PartialEq, Deserialize)]
//! # pub struct V1 { pub val: i32 }
//! # #[derive(Debug, Clone, PartialEq, Deserialize)]
//! # pub struct V2 { pub val: i32 }
//! # impl From<V1> for V3 { fn from(v: V1) -> Self { Self { val: v.val } } }
//! # impl From<V2> for V3 { fn from(v: V2) -> Self { Self { val: v.val } } }
//! #[backwards_compat(tag = "version", version = 3, versions(1: V1 => 3, 2: V2 => 3))]
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct V3 { pub val: i32 }
//! ```
//!
//! ## String Version Keys
//!
//! You can use string keys (such as `SemVer` strings) instead of integer versions:
//!
//! ```rust
//! # use backwards_compat::backwards_compat;
//! # use serde::Deserialize;
//! # #[derive(Debug, Clone, PartialEq, Deserialize)]
//! # pub struct V1 { pub val: i32 }
//! # impl From<V1> for V2 { fn from(v: V1) -> Self { Self { val: v.val } } }
//! #[backwards_compat(tag = "version", version = "2.0", versions("1.0": V1))]
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct V2 { pub val: i32 }
//! ```
pub use backwards_compat_derive::backwards_compat;
