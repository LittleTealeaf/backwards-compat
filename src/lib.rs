#![doc = include_str!("../README.md")]

pub use backwards_compat_derive::backwards_compat;

mod de;

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
    pub use crate::de::{
        AdjacentKey, AdjacentKeyResult, ChainedMapAccess, Content, ContentDeserializer,
        ContentRefDeserializer, InternalKey, InternalKeyResult, OptionForwardingDeserializer,
        RemainingMapDeserializer, TagValue, drain_map, read_adjacent_content, unknown_version_error,
    };

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
}
