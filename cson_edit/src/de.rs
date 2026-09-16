//! Schicht 2: `impl serde::Deserializer for &Node`.
//!
//! This lets a typed Rust value be read out of a parsed [`super::Document`]
//! while the `Document` itself stays intact -- unlike a plain
//! `text -> T` deserializer, which would have to discard the source (and
//! its comments) to produce `T` in the first place. `serde::Deserializer`
//! is a pull-based interface with no callback for "there was a comment
//! here," so comments can only survive by staying in the `Document`
//! that's being read *from*, not by riding along into `T`.
//!
//! The structure mirrors `serde_json::value::de`'s `impl<'de>
//! Deserializer<'de> for &'de Value` closely on purpose (same method
//! bodies, same helper-struct shapes) -- this crate started as part of a
//! CSON-flavored `serde_json` fork -- adapted to this crate's
//! `Value`/`Node`/`Entry` types instead of `serde_json::Value`/`Map`.

use super::{Entry, Node, Number, ParseError, Value};
use alloc::borrow::Cow;
use alloc::string::ToString;
use core::fmt;
use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, Expected, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};

impl de::Error for ParseError {
    #[cold]
    fn custom<T: fmt::Display>(msg: T) -> Self {
        ParseError { message: msg.to_string(), line: 0, column: 0 }
    }
}

impl<'a> Node<'a> {
    #[cold]
    fn unexpected(&self) -> Unexpected<'_> {
        match &self.value {
            Value::Null => Unexpected::Unit,
            Value::Bool(b) => Unexpected::Bool(*b),
            Value::Number(n) => n.unexpected(),
            Value::Str(s) => Unexpected::Str(s.as_str()),
            Value::Array { .. } => Unexpected::Seq,
            Value::Object { .. } => Unexpected::Map,
        }
    }

    #[cold]
    fn invalid_type<E: de::Error>(&self, exp: &dyn Expected) -> E {
        de::Error::invalid_type(self.unexpected(), exp)
    }
}

impl<'a> Number<'a> {
    #[cold]
    fn unexpected(&self) -> Unexpected<'_> {
        let raw = self.as_str();
        if !raw.contains(['.', 'e', 'E']) {
            if let Ok(u) = raw.parse::<u64>() {
                return Unexpected::Unsigned(u);
            }
            if let Ok(i) = raw.parse::<i64>() {
                return Unexpected::Signed(i);
            }
        }
        match raw.parse::<f64>() {
            Ok(f) => Unexpected::Float(f),
            Err(_) => Unexpected::Other("number"),
        }
    }
}

fn visit_number_any<'de, V>(n: &Number<'_>, visitor: V) -> Result<V::Value, ParseError>
where
    V: Visitor<'de>,
{
    let raw = n.as_str();
    if !raw.contains(['.', 'e', 'E']) {
        if let Ok(u) = raw.parse::<u64>() {
            return visitor.visit_u64(u);
        }
        if let Ok(i) = raw.parse::<i64>() {
            return visitor.visit_i64(i);
        }
    }
    match raw.parse::<f64>() {
        Ok(f) => visitor.visit_f64(f),
        Err(_) => Err(de::Error::custom(alloc::format!("invalid number literal {:?}", raw))),
    }
}

macro_rules! deserialize_node_number {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, ParseError>
        where
            V: Visitor<'de>,
        {
            match &self.value {
                Value::Number(n) => match n.as_str().parse::<$ty>() {
                    Ok(v) => visitor.$visit(v),
                    Err(_) => Err(de::Error::custom(alloc::format!(
                        "number {} does not fit in {}",
                        n.as_str(),
                        stringify!($ty),
                    ))),
                },
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

impl<'a, 'de> serde::Deserializer<'de> for &'de Node<'a> {
    type Error = ParseError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Number(n) => visit_number_any(n, visitor),
            Value::Str(s) => visitor.visit_borrowed_str(s.as_str()),
            Value::Array { items, .. } => visit_array(items, visitor),
            Value::Object { entries, .. } => visit_object(entries, visitor),
        }
    }

    deserialize_node_number!(deserialize_i8, visit_i8, i8);
    deserialize_node_number!(deserialize_i16, visit_i16, i16);
    deserialize_node_number!(deserialize_i32, visit_i32, i32);
    deserialize_node_number!(deserialize_i64, visit_i64, i64);
    deserialize_node_number!(deserialize_i128, visit_i128, i128);
    deserialize_node_number!(deserialize_u8, visit_u8, u8);
    deserialize_node_number!(deserialize_u16, visit_u16, u16);
    deserialize_node_number!(deserialize_u32, visit_u32, u32);
    deserialize_node_number!(deserialize_u64, visit_u64, u64);
    deserialize_node_number!(deserialize_u128, visit_u128, u128);
    deserialize_node_number!(deserialize_f32, visit_f32, f32);
    deserialize_node_number!(deserialize_f64, visit_f64, f64);

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Object { entries, .. } => {
                if entries.len() != 1 {
                    return Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                }
                let entry = &entries[0];
                let variant = match &entry.key.value {
                    Value::Str(s) => s.as_str(),
                    _ => return Err(self.invalid_type(&"string or map")),
                };
                let _ = (name, variants);
                visitor.visit_enum(EnumRefDeserializer { variant, value: Some(&entry.value) })
            }
            Value::Str(s) => {
                visitor.visit_enum(EnumRefDeserializer { variant: s.as_str(), value: None })
            }
            other => Err(de::Error::invalid_type(other.unexpected(), &"string or map")),
        }
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Bool(v) => visitor.visit_bool(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Str(s) => visitor.visit_borrowed_str(s.as_str()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Str(s) => visitor.visit_borrowed_str(s.as_str()),
            Value::Array { items, .. } => visit_array(items, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Array { items, .. } => visit_array(items, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, ParseError>
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
    ) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Object { entries, .. } => visit_object(entries, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &self.value {
            Value::Object { entries, .. } => visit_object(entries, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

fn visit_array<'de, V>(items: &'de [Node<'_>], visitor: V) -> Result<V::Value, ParseError>
where
    V: Visitor<'de>,
{
    let len = items.len();
    let mut deserializer = SeqRefDeserializer { iter: items.iter() };
    let seq = tri!(visitor.visit_seq(&mut deserializer));
    let remaining = deserializer.iter.len();
    if remaining == 0 {
        Ok(seq)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in array"))
    }
}

fn visit_object<'de, V>(entries: &'de [Entry<'_>], visitor: V) -> Result<V::Value, ParseError>
where
    V: Visitor<'de>,
{
    let len = entries.len();
    let mut deserializer = MapRefDeserializer { iter: entries.iter(), value: None };
    let map = tri!(visitor.visit_map(&mut deserializer));
    let remaining = deserializer.iter.len();
    if remaining == 0 {
        Ok(map)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in map"))
    }
}

struct SeqRefDeserializer<'de> {
    iter: core::slice::Iter<'de, Node<'de>>,
}

impl<'de> SeqAccess<'de> for SeqRefDeserializer<'de> {
    type Error = ParseError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, ParseError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(node) => seed.deserialize(node).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

struct MapRefDeserializer<'de> {
    iter: core::slice::Iter<'de, Entry<'de>>,
    value: Option<&'de Node<'de>>,
}

impl<'de> MapAccess<'de> for MapRefDeserializer<'de> {
    type Error = ParseError;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, ParseError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(entry) => {
                self.value = Some(&entry.value);
                let key = match &entry.key.value {
                    Value::Str(s) => Cow::Borrowed(s.as_str()),
                    _ => unreachable!("object keys are always strings"),
                };
                seed.deserialize(MapKeyDeserializer { key }).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, ParseError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(node) => seed.deserialize(node),
            None => Err(de::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

struct EnumRefDeserializer<'de> {
    variant: &'de str,
    value: Option<&'de Node<'de>>,
}

impl<'de> EnumAccess<'de> for EnumRefDeserializer<'de> {
    type Error = ParseError;
    type Variant = VariantRefDeserializer<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), ParseError>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = self.variant.into_deserializer();
        let visitor = VariantRefDeserializer { value: self.value };
        seed.deserialize(variant).map(|v| (v, visitor))
    }
}

struct VariantRefDeserializer<'de> {
    value: Option<&'de Node<'de>>,
}

impl<'de> VariantAccess<'de> for VariantRefDeserializer<'de> {
    type Error = ParseError;

    fn unit_variant(self) -> Result<(), ParseError> {
        match self.value {
            Some(node) => Deserialize::deserialize(node),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, ParseError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.value {
            Some(node) => seed.deserialize(node),
            None => Err(de::Error::invalid_type(Unexpected::UnitVariant, &"newtype variant")),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(node) => match &node.value {
                Value::Array { items, .. } => {
                    if items.is_empty() {
                        visitor.visit_unit()
                    } else {
                        visit_array(items, visitor)
                    }
                }
                other => Err(de::Error::invalid_type(other.unexpected(), &"tuple variant")),
            },
            None => Err(de::Error::invalid_type(Unexpected::UnitVariant, &"tuple variant")),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(node) => match &node.value {
                Value::Object { entries, .. } => visit_object(entries, visitor),
                other => Err(de::Error::invalid_type(other.unexpected(), &"struct variant")),
            },
            None => Err(de::Error::invalid_type(Unexpected::UnitVariant, &"struct variant")),
        }
    }
}

impl<'a> Value<'a> {
    #[cold]
    fn unexpected(&self) -> Unexpected<'_> {
        match self {
            Value::Null => Unexpected::Unit,
            Value::Bool(b) => Unexpected::Bool(*b),
            Value::Number(n) => n.unexpected(),
            Value::Str(s) => Unexpected::Str(s.as_str()),
            Value::Array { .. } => Unexpected::Seq,
            Value::Object { .. } => Unexpected::Map,
        }
    }
}

struct MapKeyDeserializer<'de> {
    key: Cow<'de, str>,
}

macro_rules! deserialize_numeric_key_impl {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, ParseError>
        where
            V: Visitor<'de>,
        {
            match self.key.parse::<$ty>() {
                Ok(v) => visitor.$visit(v),
                Err(_) => Err(de::Error::invalid_value(Unexpected::Str(&self.key), &visitor)),
            }
        }
    };
}

impl<'de> serde::Deserializer<'de> for MapKeyDeserializer<'de> {
    type Error = ParseError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match self.key {
            Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
            Cow::Owned(s) => visitor.visit_string(s),
        }
    }

    deserialize_numeric_key_impl!(deserialize_i8, visit_i8, i8);
    deserialize_numeric_key_impl!(deserialize_i16, visit_i16, i16);
    deserialize_numeric_key_impl!(deserialize_i32, visit_i32, i32);
    deserialize_numeric_key_impl!(deserialize_i64, visit_i64, i64);
    deserialize_numeric_key_impl!(deserialize_i128, visit_i128, i128);
    deserialize_numeric_key_impl!(deserialize_u8, visit_u8, u8);
    deserialize_numeric_key_impl!(deserialize_u16, visit_u16, u16);
    deserialize_numeric_key_impl!(deserialize_u32, visit_u32, u32);
    deserialize_numeric_key_impl!(deserialize_u64, visit_u64, u64);
    deserialize_numeric_key_impl!(deserialize_u128, visit_u128, u128);
    deserialize_numeric_key_impl!(deserialize_f32, visit_f32, f32);
    deserialize_numeric_key_impl!(deserialize_f64, visit_f64, f64);

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        match &*self.key {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(de::Error::invalid_type(Unexpected::Str(&self.key), &visitor)),
        }
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError>
    where
        V: Visitor<'de>,
    {
        self.key.into_deserializer().deserialize_enum(name, variants, visitor)
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct identifier ignored_any
    }
}

