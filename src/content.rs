use core::fmt;
use core::marker::PhantomData;
use serde::de::{
    self, Deserialize, DeserializeSeed, Deserializer, EnumAccess, Expected, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};

#[derive(Debug)]
pub enum Content<'de> {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    Str(&'de str),
    ByteBuf(Vec<u8>),
    Bytes(&'de [u8]),
    None,
    Some(Box<Self>),
    Unit,
    Newtype(Box<Self>),
    Seq(Vec<Self>),
    Map(Vec<(Self, Self)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagValue<'de> {
    Int(u64),
    NegativeInt(i64),
    Str(&'de str),
    String(String),
    Other,
}

impl TagValue<'_> {
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match *self {
            Self::Int(n) => Some(n),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for TagValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::NegativeInt(n) => write!(f, "{n}"),
            Self::Str(s) => write!(f, "\"{s}\""),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Other => f.write_str("<non-version tag>"),
        }
    }
}

#[derive(Debug)]
pub enum ExtractedPayload<'de> {
    Tagged {
        tag: TagValue<'de>,
        payload: Content<'de>,
    },
    Untagged(Content<'de>),
    NotAMap(Content<'de>),
    MissingContent {
        tag: TagValue<'de>,
    },
}

impl<'de> Content<'de> {
    fn matches_key(&self, key: &str) -> bool {
        let key_bytes = key.as_bytes();
        match self {
            Self::Str(s) => *s == key,
            Self::String(s) => s == key,
            Self::Bytes(b) => *b == key_bytes,
            Self::ByteBuf(b) => b.as_slice() == key_bytes,
            _ => false,
        }
    }

    #[must_use]
    pub fn has_map_key(&self, key: &str) -> bool {
        match self {
            Self::Map(entries) => entries.iter().any(|(k, _)| k.matches_key(key)),
            _ => false,
        }
    }

    #[must_use]
    pub fn into_tag_value(self) -> TagValue<'de> {
        match self {
            Self::U8(n) => TagValue::Int(u64::from(n)),
            Self::U16(n) => TagValue::Int(u64::from(n)),
            Self::U32(n) => TagValue::Int(u64::from(n)),
            Self::U64(n) => TagValue::Int(n),
            Self::U128(n) => u64::try_from(n).map_or(TagValue::Other, TagValue::Int),
            Self::I8(n) => {
                u64::try_from(n).map_or_else(|_| TagValue::NegativeInt(i64::from(n)), TagValue::Int)
            }
            Self::I16(n) => {
                u64::try_from(n).map_or_else(|_| TagValue::NegativeInt(i64::from(n)), TagValue::Int)
            }
            Self::I32(n) => {
                u64::try_from(n).map_or_else(|_| TagValue::NegativeInt(i64::from(n)), TagValue::Int)
            }
            Self::I64(n) => u64::try_from(n).map_or(TagValue::NegativeInt(n), TagValue::Int),
            Self::I128(n) => u64::try_from(n).map_or_else(
                |_| i64::try_from(n).map_or(TagValue::Other, TagValue::NegativeInt),
                TagValue::Int,
            ),
            Self::Str(s) => TagValue::Str(s),
            Self::String(s) => TagValue::String(s),
            _ => TagValue::Other,
        }
    }

    #[must_use]
    pub fn extract_internal_tag(self, tag_field: &str) -> ExtractedPayload<'de> {
        let mut cur = self;
        while let Self::Newtype(inner) = cur {
            cur = *inner;
        }
        match cur {
            Self::Map(mut entries) => {
                if let Some(pos) = entries.iter().position(|(k, _)| k.matches_key(tag_field)) {
                    let (_, tag_content) = entries.remove(pos);
                    ExtractedPayload::Tagged {
                        tag: tag_content.into_tag_value(),
                        payload: Self::Map(entries),
                    }
                } else {
                    ExtractedPayload::Untagged(Self::Map(entries))
                }
            }
            other => ExtractedPayload::NotAMap(other),
        }
    }

    #[must_use]
    pub fn extract_adjacent_tag(self, tag_field: &str, content_field: &str) -> ExtractedPayload<'de> {
        let mut cur = self;
        while let Self::Newtype(inner) = cur {
            cur = *inner;
        }
        match cur {
            Self::Map(mut entries) => {
                let Some(tag_pos) = entries.iter().position(|(k, _)| k.matches_key(tag_field)) else {
                    return ExtractedPayload::Untagged(Self::Map(entries));
                };
                let (_, tag_content) = entries.remove(tag_pos);
                let tag = tag_content.into_tag_value();

                if let Some(content_pos) = entries.iter().position(|(k, _)| k.matches_key(content_field)) {
                    let (_, payload) = entries.remove(content_pos);
                    ExtractedPayload::Tagged { tag, payload }
                } else {
                    ExtractedPayload::MissingContent { tag }
                }
            }
            other => ExtractedPayload::NotAMap(other),
        }
    }

    #[cold]
    fn unexpected(&self) -> Unexpected<'_> {
        match *self {
            Self::Bool(b) => Unexpected::Bool(b),
            Self::U8(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U16(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U32(n) => Unexpected::Unsigned(u64::from(n)),
            Self::U64(n) => Unexpected::Unsigned(n),
            Self::U128(_) => Unexpected::Other("128-bit unsigned integer"),
            Self::I8(n) => Unexpected::Signed(i64::from(n)),
            Self::I16(n) => Unexpected::Signed(i64::from(n)),
            Self::I32(n) => Unexpected::Signed(i64::from(n)),
            Self::I64(n) => Unexpected::Signed(n),
            Self::I128(_) => Unexpected::Other("128-bit signed integer"),
            Self::F32(f) => Unexpected::Float(f64::from(f)),
            Self::F64(f) => Unexpected::Float(f),
            Self::Char(c) => Unexpected::Char(c),
            Self::String(ref s) => Unexpected::Str(s),
            Self::Str(s) => Unexpected::Str(s),
            Self::ByteBuf(ref b) => Unexpected::Bytes(b),
            Self::Bytes(b) => Unexpected::Bytes(b),
            Self::None | Self::Some(_) => Unexpected::Option,
            Self::Unit => Unexpected::Unit,
            Self::Newtype(_) => Unexpected::NewtypeStruct,
            Self::Seq(_) => Unexpected::Seq,
            Self::Map(_) => Unexpected::Map,
        }
    }
}

impl<'de> Deserialize<'de> for Content<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ContentVisitor { value: PhantomData })
    }
}

struct ContentVisitor<'de> {
    value: PhantomData<Content<'de>>,
}

impl<'de> Visitor<'de> for ContentVisitor<'de> {
    type Value = Content<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_bool<F>(self, v: bool) -> Result<Self::Value, F> {
        Ok(Content::Bool(v))
    }

    fn visit_i8<F>(self, v: i8) -> Result<Self::Value, F> {
        Ok(Content::I8(v))
    }

    fn visit_i16<F>(self, v: i16) -> Result<Self::Value, F> {
        Ok(Content::I16(v))
    }

    fn visit_i32<F>(self, v: i32) -> Result<Self::Value, F> {
        Ok(Content::I32(v))
    }

    fn visit_i64<F>(self, v: i64) -> Result<Self::Value, F> {
        Ok(Content::I64(v))
    }

    fn visit_i128<F>(self, v: i128) -> Result<Self::Value, F> {
        Ok(Content::I128(v))
    }

    fn visit_u8<F>(self, v: u8) -> Result<Self::Value, F> {
        Ok(Content::U8(v))
    }

    fn visit_u16<F>(self, v: u16) -> Result<Self::Value, F> {
        Ok(Content::U16(v))
    }

    fn visit_u32<F>(self, v: u32) -> Result<Self::Value, F> {
        Ok(Content::U32(v))
    }

    fn visit_u64<F>(self, v: u64) -> Result<Self::Value, F> {
        Ok(Content::U64(v))
    }

    fn visit_u128<F>(self, v: u128) -> Result<Self::Value, F> {
        Ok(Content::U128(v))
    }

    fn visit_f32<F>(self, v: f32) -> Result<Self::Value, F> {
        Ok(Content::F32(v))
    }

    fn visit_f64<F>(self, v: f64) -> Result<Self::Value, F> {
        Ok(Content::F64(v))
    }

    fn visit_char<F>(self, v: char) -> Result<Self::Value, F> {
        Ok(Content::Char(v))
    }

    fn visit_str<F>(self, v: &str) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Content::String(v.to_owned()))
    }

    fn visit_borrowed_str<F>(self, v: &'de str) -> Result<Self::Value, F> {
        Ok(Content::Str(v))
    }

    fn visit_string<F>(self, v: String) -> Result<Self::Value, F> {
        Ok(Content::String(v))
    }

    fn visit_bytes<F>(self, v: &[u8]) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Content::ByteBuf(v.to_vec()))
    }

    fn visit_borrowed_bytes<F>(self, v: &'de [u8]) -> Result<Self::Value, F> {
        Ok(Content::Bytes(v))
    }

    fn visit_byte_buf<F>(self, v: Vec<u8>) -> Result<Self::Value, F> {
        Ok(Content::ByteBuf(v))
    }

    fn visit_unit<F>(self) -> Result<Self::Value, F> {
        Ok(Content::Unit)
    }

    fn visit_none<F>(self) -> Result<Self::Value, F> {
        Ok(Content::None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Content::Some(Box::new(v)))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Content::Newtype(Box::new(v)))
    }

    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(e) = seq.next_element()? {
            vec.push(e);
        }
        Ok(Content::Seq(vec))
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut vec = Vec::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(kv) = map.next_entry()? {
            vec.push(kv);
        }
        Ok(Content::Map(vec))
    }
}

pub struct ContentDeserializer<'de, E> {
    content: Content<'de>,
    err: PhantomData<E>,
}

impl<'de, E> ContentDeserializer<'de, E>
where
    E: de::Error,
{
    #[must_use]
    pub const fn new(content: Content<'de>) -> Self {
        Self {
            content,
            err: PhantomData,
        }
    }

    #[cold]
    fn invalid_type(self, exp: &dyn Expected) -> E {
        de::Error::invalid_type(self.content.unexpected(), exp)
    }

    fn deserialize_integer<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::U8(v) => visitor.visit_u8(v),
            Content::U16(v) => visitor.visit_u16(v),
            Content::U32(v) => visitor.visit_u32(v),
            Content::U64(v) => visitor.visit_u64(v),
            Content::U128(v) => visitor.visit_u128(v),
            Content::I8(v) => visitor.visit_i8(v),
            Content::I16(v) => visitor.visit_i16(v),
            Content::I32(v) => visitor.visit_i32(v),
            Content::I64(v) => visitor.visit_i64(v),
            Content::I128(v) => visitor.visit_i128(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_float<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::F32(v) => visitor.visit_f32(v),
            Content::F64(v) => visitor.visit_f64(v),
            Content::U8(v) => visitor.visit_u8(v),
            Content::U16(v) => visitor.visit_u16(v),
            Content::U32(v) => visitor.visit_u32(v),
            Content::U64(v) => visitor.visit_u64(v),
            Content::I8(v) => visitor.visit_i8(v),
            Content::I16(v) => visitor.visit_i16(v),
            Content::I32(v) => visitor.visit_i32(v),
            Content::I64(v) => visitor.visit_i64(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }
}

impl<'de, E> Deserializer<'de> for ContentDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Bool(v) => visitor.visit_bool(v),
            Content::U8(v) => visitor.visit_u8(v),
            Content::U16(v) => visitor.visit_u16(v),
            Content::U32(v) => visitor.visit_u32(v),
            Content::U64(v) => visitor.visit_u64(v),
            Content::U128(v) => visitor.visit_u128(v),
            Content::I8(v) => visitor.visit_i8(v),
            Content::I16(v) => visitor.visit_i16(v),
            Content::I32(v) => visitor.visit_i32(v),
            Content::I64(v) => visitor.visit_i64(v),
            Content::I128(v) => visitor.visit_i128(v),
            Content::F32(v) => visitor.visit_f32(v),
            Content::F64(v) => visitor.visit_f64(v),
            Content::Char(v) => visitor.visit_char(v),
            Content::String(v) => visitor.visit_string(v),
            Content::Str(v) => visitor.visit_borrowed_str(v),
            Content::ByteBuf(v) => visitor.visit_byte_buf(v),
            Content::Bytes(v) => visitor.visit_borrowed_bytes(v),
            Content::Unit => visitor.visit_unit(),
            Content::None => visitor.visit_none(),
            Content::Some(v) => visitor.visit_some(Self::new(*v)),
            Content::Newtype(v) => visitor.visit_newtype_struct(Self::new(*v)),
            Content::Seq(v) => visit_content_seq(v, visitor),
            Content::Map(v) => visit_content_map(v, visitor),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Bool(v) => visitor.visit_bool(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Char(v) => visitor.visit_char(v),
            Content::String(v) => visitor.visit_string(v),
            Content::Str(v) => visitor.visit_borrowed_str(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::String(v) => visitor.visit_string(v),
            Content::Str(v) => visitor.visit_borrowed_str(v),
            Content::ByteBuf(v) => visitor.visit_byte_buf(v),
            Content::Bytes(v) => visitor.visit_borrowed_bytes(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::String(v) => visitor.visit_string(v),
            Content::Str(v) => visitor.visit_borrowed_str(v),
            Content::ByteBuf(v) => visitor.visit_byte_buf(v),
            Content::Bytes(v) => visitor.visit_borrowed_bytes(v),
            Content::Seq(v) => visit_content_seq(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::None => visitor.visit_none(),
            Content::Some(v) => visitor.visit_some(Self::new(*v)),
            Content::Unit => visitor.visit_unit(),
            other => visitor.visit_some(Self::new(other)),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Unit => visitor.visit_unit(),
            Content::Map(ref v) if v.is_empty() => visitor.visit_unit(),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Map(ref v) if v.is_empty() => visitor.visit_unit(),
            Content::Seq(ref v) if v.is_empty() => visitor.visit_unit(),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Newtype(v) => visitor.visit_newtype_struct(Self::new(*v)),
            other => visitor.visit_newtype_struct(Self::new(other)),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Seq(v) => visit_content_seq(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Map(v) => visit_content_map(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::Seq(v) => visit_content_seq(v, visitor),
            Content::Map(v) => visit_content_map(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let (variant, value) = match self.content {
            Content::Map(value) => {
                let mut iter = value.into_iter();
                let Some((variant, value)) = iter.next() else {
                    return Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                };
                if iter.next().is_some() {
                    return Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                }
                (variant, Some(value))
            }
            s @ (Content::String(_) | Content::Str(_)) => (s, None),
            other => {
                return Err(de::Error::invalid_type(other.unexpected(), &"string or map"));
            }
        };

        visitor.visit_enum(EnumDeserializer {
            variant,
            value,
            err: PhantomData,
        })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.content {
            Content::String(v) => visitor.visit_string(v),
            Content::Str(v) => visitor.visit_borrowed_str(v),
            Content::ByteBuf(v) => visitor.visit_byte_buf(v),
            Content::Bytes(v) => visitor.visit_borrowed_bytes(v),
            Content::U8(v) => visitor.visit_u8(v),
            Content::U64(v) => visitor.visit_u64(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        drop(self);
        visitor.visit_unit()
    }
}

impl<'de, E> IntoDeserializer<'de, E> for ContentDeserializer<'de, E>
where
    E: de::Error,
{
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

struct EnumDeserializer<'de, E> {
    variant: Content<'de>,
    value: Option<Content<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> EnumAccess<'de> for EnumDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;
    type Variant = VariantDeserializer<'de, Self::Error>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), E>
    where
        V: DeserializeSeed<'de>,
    {
        let visitor = VariantDeserializer {
            value: self.value,
            err: PhantomData,
        };
        seed.deserialize(ContentDeserializer::new(self.variant))
            .map(|v| (v, visitor))
    }
}

pub struct VariantDeserializer<'de, E> {
    value: Option<Content<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> VariantAccess<'de> for VariantDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn unit_variant(self) -> Result<(), E> {
        self.value.map_or_else(
            || Ok(()),
            |value| Deserialize::deserialize(ContentDeserializer::new(value)),
        )
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, E>
    where
        T: DeserializeSeed<'de>,
    {
        self.value.map_or_else(
            || {
                Err(de::Error::invalid_type(
                    Unexpected::UnitVariant,
                    &"newtype variant",
                ))
            },
            |value| seed.deserialize(ContentDeserializer::new(value)),
        )
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(Content::Seq(v)) => visit_content_seq(v, visitor),
            Some(other) => Err(de::Error::invalid_type(other.unexpected(), &"tuple variant")),
            None => Err(de::Error::invalid_type(Unexpected::UnitVariant, &"tuple variant")),
        }
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(Content::Map(v)) => visit_content_map(v, visitor),
            Some(Content::Seq(v)) => visit_content_seq(v, visitor),
            Some(other) => Err(de::Error::invalid_type(other.unexpected(), &"struct variant")),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

struct SeqDeserializer<'de, E> {
    iter: <Vec<Content<'de>> as IntoIterator>::IntoIter,
    err: PhantomData<E>,
}

impl<'de, E> SeqAccess<'de> for SeqDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.iter.next().map_or_else(
            || Ok(None),
            |value| seed.deserialize(ContentDeserializer::new(value)).map(Some),
        )
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

fn visit_content_seq<'de, V, E>(content: Vec<Content<'de>>, visitor: V) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let seq_visitor = SeqDeserializer {
        iter: content.into_iter(),
        err: PhantomData,
    };
    visitor.visit_seq(seq_visitor)
}

struct MapDeserializer<'de, E> {
    iter: <Vec<(Content<'de>, Content<'de>)> as IntoIterator>::IntoIter,
    value: Option<Content<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> MapAccess<'de> for MapDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(ContentDeserializer::new(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.value.take().map_or_else(
            || Err(de::Error::custom("value is missing")),
            |value| seed.deserialize(ContentDeserializer::new(value)),
        )
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

fn visit_content_map<'de, V, E>(content: Vec<(Content<'de>, Content<'de>)>, visitor: V) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let map_visitor = MapDeserializer {
        iter: content.into_iter(),
        value: None,
        err: PhantomData,
    };
    visitor.visit_map(map_visitor)
}

pub struct VariantContentDeserializer<'de, E> {
    variant: &'static str,
    payload: Content<'de>,
    err: PhantomData<E>,
}

impl<'de, E> VariantContentDeserializer<'de, E>
where
    E: de::Error,
{
    #[must_use]
    pub const fn new(variant: &'static str, payload: Content<'de>) -> Self {
        Self {
            variant,
            payload,
            err: PhantomData,
        }
    }
}

impl<'de, E> EnumAccess<'de> for VariantContentDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;
    type Variant = VariantDeserializer<'de, E>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = VariantDeserializer {
            value: Some(self.payload),
            err: PhantomData,
        };
        seed.deserialize(self.variant.into_deserializer())
            .map(|v| (v, variant))
    }
}

impl<'de, E> Deserializer<'de> for VariantContentDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(de::Error::invalid_type(Unexpected::Enum, &visitor))
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_enum(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

#[cold]
#[must_use]
pub fn unknown_version_error<E: de::Error>(
    found: &TagValue<'_>,
    current_int: Option<u64>,
    supported: &[&str],
    has_disabled_versions: bool,
) -> E {
    if matches!(found, TagValue::Other) {
        return de::Error::invalid_type(
            Unexpected::Other("non-version tag"),
            &"an integer or string version tag",
        );
    }

    let list = supported.join(", ");
    if let TagValue::Int(n) = *found
        && let Some(max) = current_int
        && n > max
    {
        return de::Error::custom(format!(
            "version {n} is newer than supported version {max} (supported versions: {list})"
        ));
    }

    let gated_hint = if has_disabled_versions {
        "; some versions may be disabled by feature flags"
    } else {
        ""
    };
    de::Error::custom(format!(
        "unknown version {found} (supported versions: {list}{gated_hint})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::Error as ValueError;

    #[test]
    fn test_into_tag_value_and_accessors() {
        // Unsigned integers
        assert_eq!(Content::U8(1).into_tag_value(), TagValue::Int(1));
        assert_eq!(Content::U16(2).into_tag_value(), TagValue::Int(2));
        assert_eq!(Content::U32(3).into_tag_value(), TagValue::Int(3));
        assert_eq!(Content::U64(u64::MAX).into_tag_value(), TagValue::Int(u64::MAX));
        assert_eq!(
            Content::U128(u128::from(u64::MAX)).into_tag_value(),
            TagValue::Int(u64::MAX)
        );
        assert_eq!(
            Content::U128(u128::from(u64::MAX) + 1).into_tag_value(),
            TagValue::Other
        );

        // Signed integers (positive, zero, negative, overflowing)
        assert_eq!(Content::I8(7).into_tag_value(), TagValue::Int(7));
        assert_eq!(Content::I8(-5).into_tag_value(), TagValue::NegativeInt(-5));
        assert_eq!(Content::I16(0).into_tag_value(), TagValue::Int(0));
        assert_eq!(Content::I16(-10).into_tag_value(), TagValue::NegativeInt(-10));
        assert_eq!(Content::I32(42).into_tag_value(), TagValue::Int(42));
        assert_eq!(Content::I32(-42).into_tag_value(), TagValue::NegativeInt(-42));
        assert_eq!(Content::I64(99).into_tag_value(), TagValue::Int(99));
        assert_eq!(
            Content::I64(i64::MIN).into_tag_value(),
            TagValue::NegativeInt(i64::MIN)
        );
        assert_eq!(
            Content::I128(i128::from(u64::MAX)).into_tag_value(),
            TagValue::Int(u64::MAX)
        );
        assert_eq!(
            Content::I128(i128::from(u64::MAX) + 1).into_tag_value(),
            TagValue::Other
        );
        assert_eq!(
            Content::I128(i128::from(i64::MIN)).into_tag_value(),
            TagValue::NegativeInt(i64::MIN)
        );
        assert_eq!(
            Content::I128(i128::from(i64::MIN) - 1).into_tag_value(),
            TagValue::Other
        );

        // Strings
        assert_eq!(Content::Str("v1").into_tag_value(), TagValue::Str("v1"));
        assert_eq!(
            Content::String("v2".to_owned()).into_tag_value(),
            TagValue::String("v2".to_owned())
        );

        // Non-tag values
        assert_eq!(Content::Bool(true).into_tag_value(), TagValue::Other);
        assert_eq!(Content::None.into_tag_value(), TagValue::Other);
        assert_eq!(Content::Unit.into_tag_value(), TagValue::Other);

        // Accessors & Display
        let int_tag = TagValue::Int(5);
        assert_eq!(int_tag.as_u64(), Some(5));
        assert_eq!(int_tag.as_str(), None);
        assert_eq!(int_tag.to_string(), "5");

        let neg_tag = TagValue::NegativeInt(-3);
        assert_eq!(neg_tag.as_u64(), None);
        assert_eq!(neg_tag.as_str(), None);
        assert_eq!(neg_tag.to_string(), "-3");

        let str_tag = TagValue::Str("v1");
        assert_eq!(str_tag.as_u64(), None);
        assert_eq!(str_tag.as_str(), Some("v1"));
        assert_eq!(str_tag.to_string(), "\"v1\"");

        let string_tag = TagValue::String("v2".to_owned());
        assert_eq!(string_tag.as_u64(), None);
        assert_eq!(string_tag.as_str(), Some("v2"));
        assert_eq!(string_tag.to_string(), "\"v2\"");

        let other_tag = TagValue::Other;
        assert_eq!(other_tag.as_u64(), None);
        assert_eq!(other_tag.as_str(), None);
        assert_eq!(other_tag.to_string(), "<non-version tag>");
    }

    #[test]
    fn test_extract_internal_tag() {
        // Tagged map removes tag key from remaining payload
        let map = Content::Map(vec![
            (Content::Str("version"), Content::U64(2)),
            (Content::Str("name"), Content::Str("alice")),
        ]);
        match map.extract_internal_tag("version") {
            ExtractedPayload::Tagged { tag, payload } => {
                assert_eq!(tag, TagValue::Int(2));
                assert!(!payload.has_map_key("version"));
                assert!(payload.has_map_key("name"));
            }
            other => panic!("expected Tagged, got {other:?}"),
        }

        // Untagged map
        let untagged = Content::Map(vec![(Content::Str("name"), Content::Str("bob"))]);
        match untagged.extract_internal_tag("version") {
            ExtractedPayload::Untagged(payload) => {
                assert!(payload.has_map_key("name"));
            }
            other => panic!("expected Untagged, got {other:?}"),
        }

        // Newtype-wrapped map
        let wrapped = Content::Newtype(Box::new(Content::Map(vec![
            (Content::Str("version"), Content::Str("v1")),
            (Content::Str("x"), Content::U8(1)),
        ])));
        match wrapped.extract_internal_tag("version") {
            ExtractedPayload::Tagged { tag, payload } => {
                assert_eq!(tag, TagValue::Str("v1"));
                assert!(!payload.has_map_key("version"));
                assert!(payload.has_map_key("x"));
            }
            other => panic!("expected Tagged, got {other:?}"),
        }

        // Non-map
        match Content::Seq(vec![Content::U8(1)]).extract_internal_tag("version") {
            ExtractedPayload::NotAMap(Content::Seq(items)) => assert_eq!(items.len(), 1),
            other => panic!("expected NotAMap, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_adjacent_tag() {
        // Tagged + content map
        let map = Content::Map(vec![
            (Content::Str("version"), Content::U64(3)),
            (Content::Str("data"), Content::Str("payload")),
        ]);
        match map.extract_adjacent_tag("version", "data") {
            ExtractedPayload::Tagged {
                tag,
                payload: Content::Str(s),
            } => {
                assert_eq!(tag, TagValue::Int(3));
                assert_eq!(s, "payload");
            }
            other => panic!("expected Tagged with Str payload, got {other:?}"),
        }

        // Tagged with missing content
        let missing = Content::Map(vec![(Content::Str("version"), Content::U64(3))]);
        match missing.extract_adjacent_tag("version", "data") {
            ExtractedPayload::MissingContent { tag } => {
                assert_eq!(tag, TagValue::Int(3));
            }
            other => panic!("expected MissingContent, got {other:?}"),
        }

        // Untagged map
        let untagged = Content::Map(vec![(Content::Str("data"), Content::U64(99))]);
        match untagged.extract_adjacent_tag("version", "data") {
            ExtractedPayload::Untagged(payload) => {
                assert!(payload.has_map_key("data"));
            }
            other => panic!("expected Untagged, got {other:?}"),
        }

        // Non-map
        match Content::U64(42).extract_adjacent_tag("version", "data") {
            ExtractedPayload::NotAMap(Content::U64(42)) => {}
            other => panic!("expected NotAMap, got {other:?}"),
        }
    }

    #[test]
    fn test_variant_content_deserializer() {
        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct OwnedPayload {
            id: u32,
        }

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct BorrowedPayload<'a> {
            name: &'a str,
        }

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        enum TestHelper<'a> {
            Owned(OwnedPayload),
            #[serde(borrow)]
            Borrowed(BorrowedPayload<'a>),
        }

        let owned_content = Content::Map(vec![(Content::Str("id"), Content::U32(42))]);
        let owned = TestHelper::deserialize(VariantContentDeserializer::<ValueError>::new(
            "Owned",
            owned_content,
        ))
        .unwrap();
        assert_eq!(owned, TestHelper::Owned(OwnedPayload { id: 42 }));

        let borrowed_content = Content::Map(vec![(Content::Str("name"), Content::Str("alice"))]);
        let borrowed = TestHelper::deserialize(VariantContentDeserializer::<ValueError>::new(
            "Borrowed",
            borrowed_content,
        ))
        .unwrap();
        assert_eq!(borrowed, TestHelper::Borrowed(BorrowedPayload { name: "alice" }));

        let err = TestHelper::deserialize(VariantContentDeserializer::<ValueError>::new(
            "UnknownVariant",
            Content::Unit,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unknown variant `UnknownVariant`"));
    }

    #[test]
    fn test_unknown_version_error() {
        let err_other: ValueError = unknown_version_error(&TagValue::Other, Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_other.to_string(),
            "invalid type: non-version tag, expected an integer or string version tag"
        );

        let err_future: ValueError =
            unknown_version_error(&TagValue::Int(9), Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_future.to_string(),
            "version 9 is newer than supported version 7 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int: ValueError =
            unknown_version_error(&TagValue::Int(3), Some(7), &["5", "6", "7"], false);
        assert_eq!(
            err_unknown_int.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7)"
        );

        let err_unknown_int_disabled: ValueError =
            unknown_version_error(&TagValue::Int(3), Some(7), &["5", "6", "7"], true);
        assert_eq!(
            err_unknown_int_disabled.to_string(),
            "unknown version 3 (supported versions: 5, 6, 7; some versions may be disabled by feature flags)"
        );

        let err_unknown_str: ValueError =
            unknown_version_error(&TagValue::Str("v0"), None, &["\"v1\"", "\"v2\""], false);
        assert_eq!(
            err_unknown_str.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\")"
        );

        let err_unknown_str_disabled: ValueError =
            unknown_version_error(&TagValue::Str("v0"), None, &["\"v1\"", "\"v2\""], true);
        assert_eq!(
            err_unknown_str_disabled.to_string(),
            "unknown version \"v0\" (supported versions: \"v1\", \"v2\"; some versions may be disabled by feature flags)"
        );
    }

    #[test]
    fn test_content_unexpected_all_variants() {
        let non_bool_cases: Vec<(Content<'_>, &str)> = vec![
            (Content::U8(1), "integer `1`"),
            (Content::U16(2), "integer `2`"),
            (Content::U32(3), "integer `3`"),
            (Content::U64(4), "integer `4`"),
            (Content::U128(5), "128-bit unsigned integer"),
            (Content::I8(-1), "integer `-1`"),
            (Content::I16(-2), "integer `-2`"),
            (Content::I32(-3), "integer `-3`"),
            (Content::I64(-4), "integer `-4`"),
            (Content::I128(-5), "128-bit signed integer"),
            (Content::F32(1.5), "floating point"),
            (Content::F64(2.5), "floating point"),
            (Content::Char('x'), "character `x`"),
            (Content::String("owned".to_owned()), "string \"owned\""),
            (Content::Str("borrowed"), "string \"borrowed\""),
            (Content::ByteBuf(vec![1, 2]), "byte array"),
            (Content::Bytes(b"ab"), "byte array"),
            (Content::None, "Option value"),
            (Content::Some(Box::new(Content::U8(1))), "Option value"),
            (Content::Unit, "unit value"),
            (Content::Newtype(Box::new(Content::U8(1))), "newtype struct"),
            (Content::Seq(vec![]), "sequence"),
            (Content::Map(vec![]), "map"),
        ];

        for (content, expected_substr) in non_bool_cases {
            let err = bool::deserialize(ContentDeserializer::<ValueError>::new(content))
                .expect_err("expected bool deserialization error");
            assert!(
                err.to_string().contains(expected_substr),
                "expected `{expected_substr}` in `{err}`"
            );
        }

        let bool_err = u8::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true)))
            .expect_err("expected u8 deserialization error for Bool");
        assert!(bool_err.to_string().contains("boolean `true`"));
    }

    #[test]
    fn test_content_visitor_all_methods() {
        let visitor = || ContentVisitor { value: PhantomData };

        assert!(matches!(
            visitor().visit_bool::<ValueError>(true),
            Ok(Content::Bool(true))
        ));
        assert!(matches!(
            visitor().visit_i8::<ValueError>(-1),
            Ok(Content::I8(-1))
        ));
        assert!(matches!(
            visitor().visit_i16::<ValueError>(-2),
            Ok(Content::I16(-2))
        ));
        assert!(matches!(
            visitor().visit_i32::<ValueError>(-3),
            Ok(Content::I32(-3))
        ));
        assert!(matches!(
            visitor().visit_i64::<ValueError>(-4),
            Ok(Content::I64(-4))
        ));
        assert!(matches!(
            visitor().visit_i128::<ValueError>(-5),
            Ok(Content::I128(-5))
        ));
        assert!(matches!(visitor().visit_u8::<ValueError>(1), Ok(Content::U8(1))));
        assert!(matches!(
            visitor().visit_u16::<ValueError>(2),
            Ok(Content::U16(2))
        ));
        assert!(matches!(
            visitor().visit_u32::<ValueError>(3),
            Ok(Content::U32(3))
        ));
        assert!(matches!(
            visitor().visit_u64::<ValueError>(4),
            Ok(Content::U64(4))
        ));
        assert!(matches!(
            visitor().visit_u128::<ValueError>(5),
            Ok(Content::U128(5))
        ));
        assert!(
            matches!(visitor().visit_f32::<ValueError>(1.25), Ok(Content::F32(v)) if (v - 1.25).abs() < f32::EPSILON)
        );
        assert!(
            matches!(visitor().visit_f64::<ValueError>(2.5), Ok(Content::F64(v)) if (v - 2.5).abs() < f64::EPSILON)
        );
        assert!(matches!(
            visitor().visit_char::<ValueError>('z'),
            Ok(Content::Char('z'))
        ));
        assert!(matches!(visitor().visit_str::<ValueError>("hi"), Ok(Content::String(s)) if s == "hi"));
        assert!(matches!(
            visitor().visit_borrowed_str::<ValueError>("hi"),
            Ok(Content::Str("hi"))
        ));
        assert!(
            matches!(visitor().visit_string::<ValueError>("hi".to_owned()), Ok(Content::String(s)) if s == "hi")
        );
        assert!(matches!(visitor().visit_bytes::<ValueError>(b"ok"), Ok(Content::ByteBuf(b)) if b == b"ok"));
        assert!(matches!(
            visitor().visit_borrowed_bytes::<ValueError>(b"ok"),
            Ok(Content::Bytes(b"ok"))
        ));
        assert!(
            matches!(visitor().visit_byte_buf::<ValueError>(vec![9]), Ok(Content::ByteBuf(b)) if b == [9])
        );
        assert!(matches!(visitor().visit_unit::<ValueError>(), Ok(Content::Unit)));
        assert!(matches!(visitor().visit_none::<ValueError>(), Ok(Content::None)));
        assert!(matches!(
            visitor().visit_some(ContentDeserializer::<ValueError>::new(Content::U8(7))),
            Ok(Content::Some(inner)) if matches!(*inner, Content::U8(7))
        ));
        assert!(matches!(
            visitor().visit_newtype_struct(ContentDeserializer::<ValueError>::new(Content::U8(8))),
            Ok(Content::Newtype(inner)) if matches!(*inner, Content::U8(8))
        ));

        // Expecting formatter
        let err = Content::deserialize(VariantContentDeserializer::<ValueError>::new("V", Content::Unit))
            .expect_err("VariantContentDeserializer::deserialize_any should fail");
        assert!(err.to_string().contains("any value"));
    }

    #[test]
    fn test_content_deserializer_primitives_and_containers() {
        struct BytesVisitor;
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("bytes")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(v.as_bytes().to_vec())
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(v.to_vec())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(out)
            }
        }

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct UnitS;

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct Nt(u16);

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct Pair(u8, u8);

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        struct Pt {
            x: u8,
            y: u8,
        }

        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Alpha,
        }

        // Bool
        assert!(bool::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true))).unwrap());

        // Integers across all signed/unsigned types
        assert_eq!(
            i8::deserialize(ContentDeserializer::<ValueError>::new(Content::I8(-8))).unwrap(),
            -8
        );
        assert_eq!(
            i16::deserialize(ContentDeserializer::<ValueError>::new(Content::I16(-16))).unwrap(),
            -16
        );
        assert_eq!(
            i32::deserialize(ContentDeserializer::<ValueError>::new(Content::I32(-32))).unwrap(),
            -32
        );
        assert_eq!(
            i64::deserialize(ContentDeserializer::<ValueError>::new(Content::I64(-64))).unwrap(),
            -64
        );
        assert_eq!(
            i128::deserialize(ContentDeserializer::<ValueError>::new(Content::I128(-128))).unwrap(),
            -128
        );
        assert_eq!(
            u8::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(8))).unwrap(),
            8
        );
        assert_eq!(
            u16::deserialize(ContentDeserializer::<ValueError>::new(Content::U16(16))).unwrap(),
            16
        );
        assert_eq!(
            u32::deserialize(ContentDeserializer::<ValueError>::new(Content::U32(32))).unwrap(),
            32
        );
        assert_eq!(
            u64::deserialize(ContentDeserializer::<ValueError>::new(Content::U64(64))).unwrap(),
            64
        );
        assert_eq!(
            u128::deserialize(ContentDeserializer::<ValueError>::new(Content::U128(128))).unwrap(),
            128
        );
        let _ = i32::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("nope"))).unwrap_err();

        // Floats including int-to-float coercion
        assert!(
            (f32::deserialize(ContentDeserializer::<ValueError>::new(Content::F32(1.5))).unwrap() - 1.5)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (f64::deserialize(ContentDeserializer::<ValueError>::new(Content::F64(2.5))).unwrap() - 2.5)
                .abs()
                < f64::EPSILON
        );
        for (c, expected) in [
            (Content::U8(1), 1.0_f64),
            (Content::U16(2), 2.0),
            (Content::U32(3), 3.0),
            (Content::U64(4), 4.0),
            (Content::I8(-1), -1.0),
            (Content::I16(-2), -2.0),
            (Content::I32(-3), -3.0),
            (Content::I64(-4), -4.0),
        ] {
            let val = f64::deserialize(ContentDeserializer::<ValueError>::new(c)).unwrap();
            assert!((val - expected).abs() < f64::EPSILON);
        }
        let _ = f64::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(false))).unwrap_err();

        // Char
        assert_eq!(
            char::deserialize(ContentDeserializer::<ValueError>::new(Content::Char('a'))).unwrap(),
            'a'
        );
        assert_eq!(
            char::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
                "b".to_owned()
            )))
            .unwrap(),
            'b'
        );
        assert_eq!(
            char::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("c"))).unwrap(),
            'c'
        );
        let _ = char::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        // Str & String
        assert_eq!(
            <&str>::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("s"))).unwrap(),
            "s"
        );
        assert_eq!(
            String::deserialize(ContentDeserializer::<ValueError>::new(Content::String(
                "o".to_owned()
            )))
            .unwrap(),
            "o"
        );
        assert_eq!(
            String::deserialize(ContentDeserializer::<ValueError>::new(Content::ByteBuf(
                b"bb".to_vec()
            )))
            .unwrap(),
            "bb"
        );
        assert_eq!(
            <&str>::deserialize(ContentDeserializer::<ValueError>::new(Content::Bytes(b"bs"))).unwrap(),
            "bs"
        );
        let _ = String::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        // Bytes & ByteBuf
        assert_eq!(
            ContentDeserializer::<ValueError>::new(Content::String("ab".to_owned()))
                .deserialize_bytes(BytesVisitor)
                .unwrap(),
            b"ab"
        );
        assert_eq!(
            ContentDeserializer::<ValueError>::new(Content::Str("cd"))
                .deserialize_byte_buf(BytesVisitor)
                .unwrap(),
            b"cd"
        );
        assert_eq!(
            ContentDeserializer::<ValueError>::new(Content::ByteBuf(vec![1, 2]))
                .deserialize_bytes(BytesVisitor)
                .unwrap(),
            vec![1, 2]
        );
        assert_eq!(
            ContentDeserializer::<ValueError>::new(Content::Bytes(&[3, 4]))
                .deserialize_byte_buf(BytesVisitor)
                .unwrap(),
            vec![3, 4]
        );
        assert_eq!(
            ContentDeserializer::<ValueError>::new(Content::Seq(vec![Content::U8(5), Content::U8(6)]))
                .deserialize_byte_buf(BytesVisitor)
                .unwrap(),
            vec![5, 6]
        );
        let _ = ContentDeserializer::<ValueError>::new(Content::Bool(true))
            .deserialize_byte_buf(BytesVisitor)
            .unwrap_err();

        // Option
        assert_eq!(
            Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::None)).unwrap(),
            None
        );
        assert_eq!(
            Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap(),
            None
        );
        assert_eq!(
            Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Some(Box::new(
                Content::U8(9)
            ))))
            .unwrap(),
            Some(9)
        );
        assert_eq!(
            Option::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(10))).unwrap(),
            Some(10)
        );

        // Unit & unit struct
        <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap();
        <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![]))).unwrap();
        let _ = <()>::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        assert_eq!(
            UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![]))).unwrap(),
            UnitS
        );
        assert_eq!(
            UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(vec![]))).unwrap(),
            UnitS
        );
        assert_eq!(
            UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap(),
            UnitS
        );
        let _ = UnitS::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        // Newtype struct
        assert_eq!(
            Nt::deserialize(ContentDeserializer::<ValueError>::new(Content::Newtype(
                Box::new(Content::U16(3))
            )))
            .unwrap(),
            Nt(3)
        );
        assert_eq!(
            Nt::deserialize(ContentDeserializer::<ValueError>::new(Content::U16(4))).unwrap(),
            Nt(4)
        );

        // Seq, tuple, tuple struct, map, struct
        assert_eq!(
            Vec::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(vec![
                Content::U8(1)
            ])))
            .unwrap(),
            vec![1]
        );
        let _ = Vec::<u8>::deserialize(ContentDeserializer::<ValueError>::new(Content::Unit)).unwrap_err();
        assert_eq!(
            <(u8, bool)>::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(vec![
                Content::U8(2),
                Content::Bool(true),
            ])))
            .unwrap(),
            (2, true)
        );

        assert_eq!(
            Pair::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(vec![
                Content::U8(3),
                Content::U8(4),
            ])))
            .unwrap(),
            Pair(3, 4)
        );

        assert_eq!(
            std::collections::BTreeMap::<String, u8>::deserialize(ContentDeserializer::<ValueError>::new(
                Content::Map(vec![(Content::Str("k"), Content::U8(5))])
            ))
            .unwrap()
            .get("k"),
            Some(&5)
        );
        let _ = std::collections::BTreeMap::<String, u8>::deserialize(
            ContentDeserializer::<ValueError>::new(Content::Unit),
        )
        .unwrap_err();

        assert_eq!(
            Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::Seq(vec![
                Content::U8(1),
                Content::U8(2),
            ])))
            .unwrap(),
            Pt { x: 1, y: 2 }
        );
        assert_eq!(
            Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![
                (Content::Str("x"), Content::U8(3)),
                (Content::Str("y"), Content::U8(4)),
            ])))
            .unwrap(),
            Pt { x: 3, y: 4 }
        );
        let _ = Pt::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        // Identifier (String, Str, ByteBuf, Bytes, U8, U64, invalid)
        for id_content in [
            Content::String("alpha".to_owned()),
            Content::Str("alpha"),
            Content::ByteBuf(b"alpha".to_vec()),
            Content::Bytes(b"alpha"),
            Content::U8(0),
            Content::U64(0),
        ] {
            assert_eq!(
                Field::deserialize(ContentDeserializer::<ValueError>::new(id_content)).unwrap(),
                Field::Alpha
            );
        }
        let _ = Field::deserialize(ContentDeserializer::<ValueError>::new(Content::Bool(true))).unwrap_err();

        // IgnoredAny & IntoDeserializer
        serde::de::IgnoredAny::deserialize(
            ContentDeserializer::<ValueError>::new(Content::U64(123)).into_deserializer(),
        )
        .unwrap();

        // deserialize_any roundtrip across variants
        for sample in [
            Content::Bool(false),
            Content::U8(1),
            Content::U16(2),
            Content::U32(3),
            Content::U64(4),
            Content::U128(5),
            Content::I8(-1),
            Content::I16(-2),
            Content::I32(-3),
            Content::I64(-4),
            Content::I128(-5),
            Content::F32(1.0),
            Content::F64(2.0),
            Content::Char('a'),
            Content::String("s".to_owned()),
            Content::Str("b"),
            Content::ByteBuf(vec![1]),
            Content::Bytes(&[2]),
            Content::Unit,
            Content::None,
            Content::Some(Box::new(Content::U8(1))),
            Content::Newtype(Box::new(Content::U8(2))),
            Content::Seq(vec![Content::U8(3)]),
            Content::Map(vec![(Content::Str("k"), Content::U8(4))]),
        ] {
            let _ = Content::deserialize(ContentDeserializer::<ValueError>::new(sample)).unwrap();
        }
    }

    #[test]
    fn test_enum_variant_errors_and_adjacent_newtype() {
        #[derive(Debug, PartialEq, Eq, serde::Deserialize)]
        enum Sample {
            Unit,
            Newtype(u8),
            Tuple(u8, u8),
            Struct { a: u8 },
        }

        // Valid enum forms
        assert_eq!(
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Unit"))).unwrap(),
            Sample::Unit
        );
        assert_eq!(
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![(
                Content::Str("Struct"),
                Content::Seq(vec![Content::U8(7)]),
            )])))
            .unwrap(),
            Sample::Struct { a: 7 }
        );

        // deserialize_enum errors: empty map, multi-key map, non-string/non-map
        let _ =
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![]))).unwrap_err();
        let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![
            (Content::Str("Unit"), Content::Unit),
            (Content::Str("Newtype"), Content::U8(1)),
        ])))
        .unwrap_err();
        let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::U8(1))).unwrap_err();

        // VariantDeserializer error arms
        let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![(
            Content::Str("Unit"),
            Content::U8(1),
        )])))
        .unwrap_err();
        let _ =
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Newtype"))).unwrap_err();
        let _ =
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Tuple"))).unwrap_err();
        let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![(
            Content::Str("Tuple"),
            Content::U8(1),
        )])))
        .unwrap_err();
        let _ =
            Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Str("Struct"))).unwrap_err();
        let _ = Sample::deserialize(ContentDeserializer::<ValueError>::new(Content::Map(vec![(
            Content::Str("Struct"),
            Content::U8(1),
        )])))
        .unwrap_err();

        // extract_adjacent_tag with nested Content::Newtype + ByteBuf/Bytes keys + has_map_key on non-map
        let nested = Content::Newtype(Box::new(Content::Newtype(Box::new(Content::Map(vec![
            (Content::Bytes(b"version"), Content::U8(2)),
            (Content::ByteBuf(b"data".to_vec()), Content::U8(99)),
        ])))));
        assert!(!nested.has_map_key("version"));
        match nested.extract_adjacent_tag("version", "data") {
            ExtractedPayload::Tagged {
                tag: TagValue::Int(2),
                payload: Content::U8(99),
            } => {}
            other => panic!("expected Tagged(2, U8(99)), got {other:?}"),
        }
    }
}
