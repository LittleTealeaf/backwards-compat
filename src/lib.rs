#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]

extern crate alloc;

pub use backwards_compat_derive::backwards_compat;

mod de;
#[cfg(feature = "schemars")]
mod schema;
mod ser;

/// Describes the wire-level tagging representation used by a [`BackwardsCompat`] type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tagging {
    /// Internally tagged: version tag is merged alongside payload fields.
    Internal {
        /// The serialized field name used for the version tag.
        tag: &'static str,
    },
    /// Adjacently tagged: version tag and payload are sibling fields.
    Adjacent {
        /// The serialized field name used for the version tag.
        tag: &'static str,
        /// The serialized field name used for the content payload.
        content: &'static str,
    },
    /// Externally tagged: version tag wraps the payload as a newtype enum variant.
    External,
}

/// Metadata trait implemented for domain models annotated with `#[backwards_compat(...)]`.
pub trait BackwardsCompat: Sized {
    /// The type representing version identifiers (`u64` or `&'static str`).
    type Version: Copy + Eq + core::fmt::Debug + core::fmt::Display + 'static;

    /// The current active schema version.
    const CURRENT_VERSION: Self::Version;

    /// The schema version emitted when serializing this type.
    const SERIALIZE_VERSION: Self::Version;

    /// All supported schema versions compiled into this binary.
    const SUPPORTED_VERSIONS: &'static [Self::Version];

    /// Whether unversioned (`_`) payloads without a version tag field are accepted.
    const ACCEPTS_UNTAGGED: bool = false;

    /// The wire-level tagging strategy used by this type.
    const TAGGING: Tagging = Tagging::Internal { tag: "version" };
}

/// Extension of [`BackwardsCompat`] supporting deserialization with source-version tracking.
pub trait BackwardsCompatDeserialize<'de>: BackwardsCompat {
    /// Deserializes `Self` while recording the source schema version the payload was decoded from.
    fn deserialize_migrated<D>(deserializer: D) -> Result<Migrated<Self>, D::Error>
    where
        D: serde::Deserializer<'de>;
}

/// Extension of [`BackwardsCompat`] supporting runtime selection of the output schema version.
pub trait BackwardsCompatSerialize: BackwardsCompat {
    /// All schema versions that this type can serialize to at runtime.
    const SERIALIZABLE_VERSIONS: &'static [Self::Version];

    /// Serializes `self` using the schema and wire format of `version`.
    fn serialize_as_version<S>(&self, version: Self::Version, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer;

    /// Returns a `serde::Serialize` adapter that serializes `self` as `version`.
    #[inline]
    fn as_version(&self, version: Self::Version) -> SerializeAsVersion<'_, Self> {
        SerializeAsVersion { value: self, version }
    }
}

/// Wrapper that serializes a [`BackwardsCompatSerialize`] value using a chosen runtime schema version.
#[derive(Debug, Clone, Copy)]
pub struct SerializeAsVersion<'a, T: BackwardsCompat> {
    /// Reference to the domain value being serialized.
    pub value: &'a T,
    /// Target schema version to emit.
    pub version: T::Version,
}

impl<T: BackwardsCompatSerialize> serde::Serialize for SerializeAsVersion<'_, T> {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize_as_version(self.version, serializer)
    }
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

    /// Returns `true` if the payload was decoded from an unversioned (`_`) schema without a tag field.
    #[must_use]
    pub const fn is_untagged(&self) -> bool {
        self.source_version.is_none()
    }

    /// Returns the source schema version found in the payload, or `None` if untagged.
    #[must_use]
    pub const fn source_version(&self) -> Option<T::Version> {
        self.source_version
    }

    /// Transforms the inner value while preserving `source_version`.
    #[must_use]
    pub fn map<U: BackwardsCompat<Version = T::Version>, F: FnOnce(T) -> U>(self, f: F) -> Migrated<U> {
        Migrated {
            value: f(self.value),
            source_version: self.source_version,
        }
    }

    /// Consumes this wrapper and returns the inner domain value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: BackwardsCompat> core::ops::Deref for Migrated<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: BackwardsCompat> core::ops::DerefMut for Migrated<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T: BackwardsCompat> AsRef<T> for Migrated<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.value
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
    pub use alloc::{borrow::ToOwned, boxed::Box, format, string::String, vec::Vec};

    pub use crate::de::{
        AdjacentKey, AdjacentKeyResult, AdjacentMapDispatch, ChainedMapAccess, Content, ContentDeserializer,
        ContentRefDeserializer, IntTagMode, IntTagOrContent, InternalKey, InternalKeyResult,
        InternalMapDispatch, OptionForwardingDeserializer, RemainingMapDeserializer, StrTag, StrTagMode,
        StrTagOrContent, TagMode, UntaggedMultiDispatch, UntaggedMultiVisitor, UntaggedSingleDispatch,
        UntaggedSingleVisitor, cautious_size_hint, drain_map, read_adjacent_content,
        unknown_int_version_error, unknown_str_version_error, visit_adjacent_map,
        visit_adjacent_map_untagged, visit_internal_map, visit_internal_map_untagged,
    };
    #[cfg(feature = "schemars")]
    pub use crate::schema::{
        TagSchemaValue, schema_adjacently_tagged, schema_externally_tagged, schema_internally_tagged,
        schema_one_of,
    };
    pub use crate::ser::{serialize_adjacently_tagged, serialize_internally_tagged, serialize_unit_tagged};
    #[cfg(feature = "schemars")]
    pub use schemars;

    pub struct WireConvert<'b, T: ?Sized, W>(pub &'b T, pub ::core::marker::PhantomData<fn() -> W>);

    impl<'b, T: ?Sized, W> WireConvert<'b, T, W> {
        #[inline]
        #[must_use]
        pub const fn new(value: &'b T) -> Self {
            Self(value, ::core::marker::PhantomData)
        }
    }

    pub trait BorrowedWireConvert<W> {
        fn convert_wire(self) -> W;
    }

    impl<'b, T: ?Sized, W> BorrowedWireConvert<W> for WireConvert<'b, T, W>
    where
        &'b T: ::core::convert::Into<W>,
    {
        #[inline]
        fn convert_wire(self) -> W {
            ::core::convert::Into::into(self.0)
        }
    }

    pub trait FallbackWireConvert<W> {
        fn convert_wire(&self) -> W;
    }

    impl<T, W> FallbackWireConvert<W> for WireConvert<'_, T, W>
    where
        T: ::core::clone::Clone + ::core::convert::Into<W>,
    {
        #[inline]
        fn convert_wire(&self) -> W {
            ::core::convert::Into::into(::core::clone::Clone::clone(self.0))
        }
    }

    pub struct TryWireConvert<'b, T: ?Sized, W>(pub &'b T, pub ::core::marker::PhantomData<fn() -> W>);

    impl<'b, T: ?Sized, W> TryWireConvert<'b, T, W> {
        #[inline]
        #[must_use]
        pub const fn new(value: &'b T) -> Self {
            Self(value, ::core::marker::PhantomData)
        }
    }

    pub trait BorrowedTryWireConvert<W> {
        type Error: ::core::fmt::Display;

        fn try_convert_wire(self) -> ::core::result::Result<W, Self::Error>;
    }

    impl<'b, T: ?Sized, W> BorrowedTryWireConvert<W> for TryWireConvert<'b, T, W>
    where
        &'b T: ::core::convert::TryInto<W>,
        <&'b T as ::core::convert::TryInto<W>>::Error: ::core::fmt::Display,
    {
        type Error = <&'b T as ::core::convert::TryInto<W>>::Error;

        #[inline]
        fn try_convert_wire(self) -> ::core::result::Result<W, Self::Error> {
            ::core::convert::TryInto::try_into(self.0)
        }
    }

    pub trait FallbackTryWireConvert<W> {
        type Error: ::core::fmt::Display;

        fn try_convert_wire(&self) -> ::core::result::Result<W, Self::Error>;
    }

    impl<T, W> FallbackTryWireConvert<W> for TryWireConvert<'_, T, W>
    where
        T: ::core::clone::Clone + ::core::convert::TryInto<W>,
        <T as ::core::convert::TryInto<W>>::Error: ::core::fmt::Display,
    {
        type Error = <T as ::core::convert::TryInto<W>>::Error;

        #[inline]
        fn try_convert_wire(&self) -> ::core::result::Result<W, Self::Error> {
            ::core::convert::TryInto::try_into(::core::clone::Clone::clone(self.0))
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

    #[test]
    fn test_try_wire_convert_borrowed_zero_clone() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedTryWireConvert as _, FallbackTryWireConvert as _, TryWireConvert};

        // Intentionally does NOT implement Clone.
        struct NonCloneSource(u32);

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u16);

        impl TryFrom<&NonCloneSource> for Wire {
            type Error = &'static str;

            fn try_from(src: &NonCloneSource) -> Result<Self, Self::Error> {
                u16::try_from(src.0).map(Wire).map_err(|_| "overflow")
            }
        }

        let ok_src = NonCloneSource(42);
        let ok_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&ok_src).try_convert_wire();
        assert_eq!(ok_res, Ok(Wire(42)));

        let err_src = NonCloneSource(70_000);
        let err_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&err_src).try_convert_wire();
        assert_eq!(err_res, Err("overflow"));
    }

    #[test]
    fn test_try_wire_convert_clone_fallback() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedTryWireConvert as _, FallbackTryWireConvert as _, TryWireConvert};

        #[derive(Clone)]
        struct CloneOnlySource(u32);

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u16);

        impl TryFrom<CloneOnlySource> for Wire {
            type Error = &'static str;

            fn try_from(src: CloneOnlySource) -> Result<Self, Self::Error> {
                u16::try_from(src.0).map(Wire).map_err(|_| "overflow")
            }
        }

        let ok_src = CloneOnlySource(123);
        let ok_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&ok_src).try_convert_wire();
        assert_eq!(ok_res, Ok(Wire(123)));

        let err_src = CloneOnlySource(100_000);
        let err_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&err_src).try_convert_wire();
        assert_eq!(err_res, Err("overflow"));
    }

    #[test]
    fn test_try_wire_convert_prefers_borrowed_over_clone() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedTryWireConvert as _, FallbackTryWireConvert as _, TryWireConvert};

        struct PanicOnCloneSource(u32);

        impl Clone for PanicOnCloneSource {
            fn clone(&self) -> Self {
                panic!("Clone must not be called when borrowed TryInto is available");
            }
        }

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u16);

        impl TryFrom<&PanicOnCloneSource> for Wire {
            type Error = &'static str;

            fn try_from(src: &PanicOnCloneSource) -> Result<Self, Self::Error> {
                u16::try_from(src.0).map(Wire).map_err(|_| "borrowed overflow")
            }
        }

        impl TryFrom<PanicOnCloneSource> for Wire {
            type Error = &'static str;

            fn try_from(src: PanicOnCloneSource) -> Result<Self, Self::Error> {
                u16::try_from(src.0).map(Wire).map_err(|_| "owned overflow")
            }
        }

        let ok_src = PanicOnCloneSource(7);
        let ok_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&ok_src).try_convert_wire();
        assert_eq!(ok_res, Ok(Wire(7)));

        let err_src = PanicOnCloneSource(70_000);
        let err_res: Result<Wire, _> = TryWireConvert::<_, Wire>::new(&err_src).try_convert_wire();
        assert_eq!(err_res, Err("borrowed overflow"));
    }

    #[test]
    fn test_wire_convert_borrowed_zero_clone() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert};

        struct NonCloneSource(u32);

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u32);

        impl From<&NonCloneSource> for Wire {
            fn from(src: &NonCloneSource) -> Self {
                Self(src.0)
            }
        }

        let src = NonCloneSource(42);
        let wire: Wire = WireConvert::<_, Wire>::new(&src).convert_wire();
        assert_eq!(wire, Wire(42));
    }

    #[test]
    fn test_wire_convert_clone_fallback() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert};

        #[derive(Clone)]
        struct CloneOnlySource(u32);

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u32);

        impl From<CloneOnlySource> for Wire {
            fn from(src: CloneOnlySource) -> Self {
                Self(src.0)
            }
        }

        let src = CloneOnlySource(99);
        let wire: Wire = WireConvert::<_, Wire>::new(&src).convert_wire();
        assert_eq!(wire, Wire(99));
    }

    #[test]
    fn test_wire_convert_prefers_borrowed_over_clone() {
        #[allow(
            unused_imports,
            reason = "both specialization traits must be in scope to verify dispatch"
        )]
        use __private::{BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert};

        struct PanicOnCloneSource(u32);

        impl Clone for PanicOnCloneSource {
            fn clone(&self) -> Self {
                panic!("Clone must not be called when borrowed Into is available");
            }
        }

        #[derive(Debug, PartialEq, Eq)]
        struct Wire(u32);

        impl From<&PanicOnCloneSource> for Wire {
            fn from(src: &PanicOnCloneSource) -> Self {
                Self(src.0 + 1)
            }
        }

        impl From<PanicOnCloneSource> for Wire {
            fn from(src: PanicOnCloneSource) -> Self {
                Self(src.0)
            }
        }

        let src = PanicOnCloneSource(7);
        let wire: Wire = WireConvert::<_, Wire>::new(&src).convert_wire();
        assert_eq!(wire, Wire(8));
    }

    #[test]
    fn test_migrated_ergonomics_deref_as_ref_and_map() {
        let current = Migrated {
            value: MockModel(20),
            source_version: Some(2),
        };
        assert!(!current.is_untagged());
        assert_eq!(current.source_version(), Some(2));
        assert_eq!(current.0, 20);
        assert_eq!(current.as_ref(), &MockModel(20));

        let mut untagged = Migrated {
            value: MockModel(0),
            source_version: None,
        };
        assert!(untagged.is_untagged());
        assert_eq!(untagged.source_version(), None);
        untagged.0 = 42;
        assert_eq!(*untagged, MockModel(42));

        let mapped = untagged.map(|m| MockModel(m.0 + 1));
        assert!(mapped.is_untagged());
        assert_eq!(mapped.into_inner(), MockModel(43));
    }
}
