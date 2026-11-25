use core::fmt::Display;

use serde::ser::{self, Impossible, Serialize, SerializeMap, SerializeStruct, Serializer};

/// Serializes an internally-tagged payload as a struct when `content` is a struct or unit.
///
/// Falls back to `serialize_map` only when `content` itself serializes as a map.
/// Using `serialize_struct` ensures formats like RON emit `(version: ..., ...)`
/// instead of `{"version": ..., ...}`.
pub fn serialize_internally_tagged<S, T, C>(
    serializer: S,
    struct_name: &'static str,
    tag_field: &'static str,
    tag: &T,
    content: &C,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + ?Sized,
    C: Serialize + ?Sized,
{
    content.serialize(FlatStructSerializer {
        serializer,
        struct_name,
        tag_field,
        tag,
    })
}

struct FlatStructSerializer<'a, S, T: ?Sized> {
    serializer: S,
    struct_name: &'static str,
    tag_field: &'static str,
    tag: &'a T,
}

fn cannot_flatten<E: ser::Error>() -> E {
    ser::Error::custom("can only flatten structs and maps")
}

struct FlatSerializeStruct<SS>(SS);

impl<SS: SerializeStruct> SerializeStruct for FlatSerializeStruct<SS> {
    type Ok = SS::Ok;
    type Error = SS::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.0.serialize_field(key, value)
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), Self::Error> {
        self.0.skip_field(key)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.0.end()
    }
}

struct FlatSerializeMap<SM>(SM);

impl<SM: SerializeMap> SerializeMap for FlatSerializeMap<SM> {
    type Ok = SM::Ok;
    type Error = SM::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.0.serialize_key(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.0.serialize_value(value)
    }

    fn serialize_entry<K: Serialize + ?Sized, V: Serialize + ?Sized>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), Self::Error> {
        self.0.serialize_entry(key, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.0.end()
    }
}

impl<S, T> Serializer for FlatStructSerializer<'_, S, T>
where
    S: Serializer,
    T: Serialize + ?Sized,
{
    type Ok = S::Ok;
    type Error = S::Error;

    type SerializeSeq = Impossible<S::Ok, S::Error>;
    type SerializeTuple = Impossible<S::Ok, S::Error>;
    type SerializeTupleStruct = Impossible<S::Ok, S::Error>;
    type SerializeTupleVariant = Impossible<S::Ok, S::Error>;
    type SerializeMap = FlatSerializeMap<S::SerializeMap>;
    type SerializeStruct = FlatSerializeStruct<S::SerializeStruct>;
    type SerializeStructVariant = Impossible<S::Ok, S::Error>;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_some<V: Serialize + ?Sized>(self, value: &V) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        let mut state = self.serializer.serialize_struct(self.struct_name, 1)?;
        state.serialize_field(self.tag_field, self.tag)?;
        state.end()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_newtype_struct<V: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &V,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<V: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &V,
    ) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(cannot_flatten())
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let mut map = self.serializer.serialize_map(len.map(|n| n + 1))?;
        map.serialize_entry(self.tag_field, self.tag)?;
        Ok(FlatSerializeMap(map))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        let mut state = self.serializer.serialize_struct(self.struct_name, len + 1)?;
        state.serialize_field(self.tag_field, self.tag)?;
        Ok(FlatSerializeStruct(state))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(cannot_flatten())
    }

    fn collect_str<V: Display + ?Sized>(self, _value: &V) -> Result<Self::Ok, Self::Error> {
        Err(cannot_flatten())
    }

    fn is_human_readable(&self) -> bool {
        self.serializer.is_human_readable()
    }
}
