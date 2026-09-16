//! Schicht 3: `impl serde::Serializer with Ok = Node`.
//!
//! Mirrors `serde_json::value::ser`'s `Serializer { type Ok = Value; .. }`
//! closely on purpose, producing a fresh [`Node`]`<'static>` tree instead
//! of a `serde_json::Value`. A tree built this way has no comments and no
//! source layout of its own (every `prefix` is empty) -- that's expected:
//! this is the "typed value -> tree" half of the round trip.
//!
//! Combining that fresh tree with an *existing* one's trivia is
//! `Document::merge_from`'s job, in `merge.rs` -- a separate step, not
//! something having both a `Deserializer` and a `Serializer` adds up to
//! on its own. It needs a diff (walk both trees in parallel, match
//! objects by key and arrays by position, and overwrite only a changed
//! node's `value` -- never the whole `Node`, or its `prefix` goes with
//! it) plus a trivia-blind equality check (so an untouched field, even
//! if its formatting differs syntactically, isn't misdetected as
//! changed) and numeric-not-textual number comparison (so `1.50` from
//! disk and `1.5` from the struct count as equal). This module only
//! knows how to build a tree from scratch; `merge.rs` is what reconciles
//! one against an existing one.

use super::{CsonStr, Entry, Node, Number, ParseError, Value};
use alloc::borrow::{Cow, ToOwned};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Display;
use serde::ser::{self, Serialize};

fn node(value: Value<'static>) -> Node<'static> {
    Node { prefix: Cow::Borrowed(""), value }
}

fn str_node(s: String) -> Node<'static> {
    node(Value::Str(CsonStr { value: Cow::Owned(s) }))
}

fn number_node(raw: String) -> Node<'static> {
    node(Value::Number(Number { raw: Cow::Owned(raw) }))
}

/// Serialize `value` into a fresh, comment-free [`Node`]`<'static>`.
///
/// Lower-level than [`crate::to_string`]/[`crate::Document::from_serialize`]
/// (which both call this): use it directly when you only need a bare
/// value, e.g. to build one field's replacement by hand rather than a
/// whole document.
pub fn to_node<T>(value: &T) -> Result<Node<'static>, ParseError>
where
    T: ?Sized + Serialize,
{
    value.serialize(Serializer)
}

/// The `serde::Serializer` that backs [`to_node`].
pub struct Serializer;

impl ser::Serializer for Serializer {
    type Ok = Node<'static>;
    type Error = ParseError;

    type SerializeSeq = SerializeVec;
    type SerializeTuple = SerializeVec;
    type SerializeTupleStruct = SerializeVec;
    type SerializeTupleVariant = SerializeTupleVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeMap;
    type SerializeStructVariant = SerializeStructVariant;

    #[inline]
    fn serialize_bool(self, value: bool) -> Result<Node<'static>, ParseError> {
        Ok(node(Value::Bool(value)))
    }

    #[inline]
    fn serialize_i8(self, value: i8) -> Result<Node<'static>, ParseError> {
        self.serialize_i64(value as i64)
    }

    #[inline]
    fn serialize_i16(self, value: i16) -> Result<Node<'static>, ParseError> {
        self.serialize_i64(value as i64)
    }

    #[inline]
    fn serialize_i32(self, value: i32) -> Result<Node<'static>, ParseError> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i64(self, value: i64) -> Result<Node<'static>, ParseError> {
        Ok(number_node((value).to_string()))
    }

    fn serialize_i128(self, value: i128) -> Result<Node<'static>, ParseError> {
        Ok(number_node(value.to_string()))
    }

    #[inline]
    fn serialize_u8(self, value: u8) -> Result<Node<'static>, ParseError> {
        self.serialize_u64(value as u64)
    }

    #[inline]
    fn serialize_u16(self, value: u16) -> Result<Node<'static>, ParseError> {
        self.serialize_u64(value as u64)
    }

    #[inline]
    fn serialize_u32(self, value: u32) -> Result<Node<'static>, ParseError> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u64(self, value: u64) -> Result<Node<'static>, ParseError> {
        Ok(number_node((value).to_string()))
    }

    fn serialize_u128(self, value: u128) -> Result<Node<'static>, ParseError> {
        Ok(number_node(value.to_string()))
    }

    #[inline]
    fn serialize_f32(self, value: f32) -> Result<Node<'static>, ParseError> {
        if value.is_finite() {
            Ok(number_node((value).to_string()))
        } else {
            Ok(node(Value::Null))
        }
    }

    #[inline]
    fn serialize_f64(self, value: f64) -> Result<Node<'static>, ParseError> {
        if value.is_finite() {
            Ok(number_node((value).to_string()))
        } else {
            Ok(node(Value::Null))
        }
    }

    #[inline]
    fn serialize_char(self, value: char) -> Result<Node<'static>, ParseError> {
        let mut s = String::new();
        s.push(value);
        Ok(str_node(s))
    }

    #[inline]
    fn serialize_str(self, value: &str) -> Result<Node<'static>, ParseError> {
        Ok(str_node(value.to_owned()))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Node<'static>, ParseError> {
        let items = value
            .iter()
            .map(|&b| number_node((b).to_string()))
            .collect();
        Ok(node(Value::Array { items, trailing: Cow::Borrowed("") }))
    }

    #[inline]
    fn serialize_unit(self) -> Result<Node<'static>, ParseError> {
        Ok(node(Value::Null))
    }

    #[inline]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node<'static>, ParseError> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Node<'static>, ParseError> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Node<'static>, ParseError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Node<'static>, ParseError>
    where
        T: ?Sized + Serialize,
    {
        let entries = vec![Entry { key: str_node(variant.to_owned()), value: tri!(to_node(value)) }];
        Ok(node(Value::Object { entries, trailing: Cow::Borrowed("") }))
    }

    #[inline]
    fn serialize_none(self) -> Result<Node<'static>, ParseError> {
        self.serialize_unit()
    }

    #[inline]
    fn serialize_some<T>(self, value: &T) -> Result<Node<'static>, ParseError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, ParseError> {
        Ok(SerializeVec { items: Vec::with_capacity(len.unwrap_or(0)) })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, ParseError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, ParseError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, ParseError> {
        Ok(SerializeTupleVariant { name: variant.to_owned(), items: Vec::with_capacity(len) })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, ParseError> {
        Ok(SerializeMap { entries: Vec::with_capacity(len.unwrap_or(0)), next_key: None })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct, ParseError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, ParseError> {
        Ok(SerializeStructVariant { name: variant.to_owned(), entries: Vec::new() })
    }

    fn collect_str<T>(self, value: &T) -> Result<Node<'static>, ParseError>
    where
        T: ?Sized + Display,
    {
        Ok(str_node(value.to_string()))
    }
}

pub struct SerializeVec {
    items: Vec<Node<'static>>,
}

pub struct SerializeTupleVariant {
    name: String,
    items: Vec<Node<'static>>,
}

pub struct SerializeMap {
    entries: Vec<Entry<'static>>,
    next_key: Option<Node<'static>>,
}

pub struct SerializeStructVariant {
    name: String,
    entries: Vec<Entry<'static>>,
}

impl ser::SerializeSeq for SerializeVec {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(tri!(to_node(value)));
        Ok(())
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        Ok(node(Value::Array { items: self.items, trailing: Cow::Borrowed("") }))
    }
}

impl ser::SerializeTuple for SerializeVec {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SerializeVec {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for SerializeTupleVariant {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(tri!(to_node(value)));
        Ok(())
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        let entries = vec![Entry {
            key: str_node(self.name),
            value: node(Value::Array { items: self.items, trailing: Cow::Borrowed("") }),
        }];
        Ok(node(Value::Object { entries, trailing: Cow::Borrowed("") }))
    }
}

impl ser::SerializeMap for SerializeMap {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        self.next_key = Some(str_node(tri!(key.serialize(MapKeySerializer))));
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        let key = self
            .next_key
            .take()
            .expect("serialize_value called before serialize_key");
        self.entries.push(Entry { key, value: tri!(to_node(value)) });
        Ok(())
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        Ok(node(Value::Object { entries: self.entries, trailing: Cow::Borrowed("") }))
    }
}

impl ser::SerializeStruct for SerializeMap {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        ser::SerializeMap::end(self)
    }
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = Node<'static>;
    type Error = ParseError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ParseError>
    where
        T: ?Sized + Serialize,
    {
        self.entries.push(Entry { key: str_node(key.to_owned()), value: tri!(to_node(value)) });
        Ok(())
    }

    fn end(self) -> Result<Node<'static>, ParseError> {
        let entries = vec![Entry {
            key: str_node(self.name),
            value: node(Value::Object { entries: self.entries, trailing: Cow::Borrowed("") }),
        }];
        Ok(node(Value::Object { entries, trailing: Cow::Borrowed("") }))
    }
}

fn key_must_be_a_string() -> ParseError {
    ParseError { message: "key must be a string".into(), line: 0, column: 0 }
}

fn float_key_must_be_finite() -> ParseError {
    ParseError { message: "float key must be finite".into(), line: 0, column: 0 }
}

/// Serializes any `Serialize` value into a `String`, for use as an object
/// key (mirrors `value::ser::MapKeySerializer`).
struct MapKeySerializer;

impl ser::Serializer for MapKeySerializer {
    type Ok = String;
    type Error = ParseError;

    type SerializeSeq = ser::Impossible<String, ParseError>;
    type SerializeTuple = ser::Impossible<String, ParseError>;
    type SerializeTupleStruct = ser::Impossible<String, ParseError>;
    type SerializeTupleVariant = ser::Impossible<String, ParseError>;
    type SerializeMap = ser::Impossible<String, ParseError>;
    type SerializeStruct = ser::Impossible<String, ParseError>;
    type SerializeStructVariant = ser::Impossible<String, ParseError>;

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<String, ParseError> {
        Ok(variant.to_owned())
    }

    #[inline]
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<String, ParseError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_bool(self, value: bool) -> Result<String, ParseError> {
        Ok(if value { "true" } else { "false" }.to_owned())
    }

    fn serialize_i8(self, value: i8) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_i16(self, value: i16) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_i32(self, value: i32) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_i64(self, value: i64) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_i128(self, value: i128) -> Result<String, ParseError> {
        Ok(value.to_string())
    }

    fn serialize_u8(self, value: u8) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_u16(self, value: u16) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_u32(self, value: u32) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_u64(self, value: u64) -> Result<String, ParseError> {
        Ok((value).to_string())
    }

    fn serialize_u128(self, value: u128) -> Result<String, ParseError> {
        Ok(value.to_string())
    }

    fn serialize_f32(self, value: f32) -> Result<String, ParseError> {
        if value.is_finite() {
            Ok((value).to_string())
        } else {
            Err(float_key_must_be_finite())
        }
    }

    fn serialize_f64(self, value: f64) -> Result<String, ParseError> {
        if value.is_finite() {
            Ok((value).to_string())
        } else {
            Err(float_key_must_be_finite())
        }
    }

    #[inline]
    fn serialize_char(self, value: char) -> Result<String, ParseError> {
        let mut s = String::new();
        s.push(value);
        Ok(s)
    }

    #[inline]
    fn serialize_str(self, value: &str) -> Result<String, ParseError> {
        Ok(value.to_owned())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<String, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit(self) -> Result<String, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<String, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String, ParseError>
    where
        T: ?Sized + Serialize,
    {
        Err(key_must_be_a_string())
    }

    fn serialize_none(self) -> Result<String, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_some<T>(self, _value: &T) -> Result<String, ParseError>
    where
        T: ?Sized + Serialize,
    {
        Err(key_must_be_a_string())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct, ParseError> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, ParseError> {
        Err(key_must_be_a_string())
    }

    fn collect_str<T>(self, value: &T) -> Result<String, ParseError>
    where
        T: ?Sized + Display,
    {
        Ok(value.to_string())
    }
}

impl ser::Error for ParseError {
    #[cold]
    fn custom<T: Display>(msg: T) -> Self {
        ParseError { message: msg.to_string(), line: 0, column: 0 }
    }
}
