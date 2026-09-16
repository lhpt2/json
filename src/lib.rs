//! `cson_edit` — a CSON parser and editor that preserves comments and
//! formatting, analogous to [`toml_edit`](https://docs.rs/toml_edit).
//!
//! This crate is the `Document -> Node` tree originally designed as part
//! of a CSON-flavored `serde_json` fork: a parse tree that keeps comments
//! and blank lines around so a configuration file can be edited and
//! written back without losing them. It was extracted to its own crate
//! (see the extraction notes in the repository's `docs/` directory) so
//! it can be depended on independently, the way `toml_edit` is
//! independent of `serde_json`.
//!
//! It is intentionally independent from `serde`: `Deserializer`/
//! `Serializer` (Schicht 2/3, in `de.rs`/`ser.rs`) are built on top of
//! this tree, not the other way around, because comments have nowhere to
//! live inside a `serde::Deserializer`'s pull-based interface. `de.rs`'s
//! doc comment goes into this in more detail.
//!
//! [`Document::merge_from`] closes the loop on top of those two:
//! it diffs a freshly `Serialize`d tree against this document and
//! rewrites only the values that actually changed, so a typed edit
//! (`deserialize` → mutate → `merge_from`) keeps every comment. See
//! `merge.rs`, and `ser.rs`'s doc comment for why Schicht 2 + 3 don't
//! add up to that on their own.
//!
//! # Example
//!
//! For plain typed read/write with no need to keep comments around, use
//! the conventional top-level functions (matching `serde_json`'s /
//! `serde_yaml`'s own naming):
//!
//! ```
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Config {
//!     name: String,
//!     port: u16,
//! }
//!
//! # fn main() -> Result<(), cson_edit::ParseError> {
//! let cfg = Config { name: "svc".to_string(), port: 8080 };
//! let text = cson_edit::to_string(&cfg)?;
//! let cfg2: Config = cson_edit::from_str(&text)?;
//! assert_eq!(cfg, cfg2);
//! # Ok(())
//! # }
//! ```
//!
//! To keep comments alive across an edit, work with [`Document`]
//! directly instead:
//!
//! ```
//! use cson_edit::{parse, Document};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize)]
//! struct Config {
//!     name: String,
//!     port: u16,
//! }
//!
//! # fn main() -> Result<(), cson_edit::ParseError> {
//! // Read-only structural access, comments intact:
//! let doc = parse("name = \"svc\"  # prod\nport = 8080\n")?;
//! println!("{}", doc);
//!
//! // Typed read (Schicht 2) -- the Document you read from still has its
//! // comments; the struct itself, like any plain Rust struct, does not:
//! let cfg: Config = doc.deserialize()?;
//!
//! // Typed write (Schicht 3) -- builds a *fresh* tree, no comments,
//! // default Style, since there's no source layout to take one from:
//! let fresh = Document::from_serialize(&cfg)?;
//! println!("{}", fresh.to_cson_string());
//! # Ok(())
//! # }
//! ```

#![no_std]
#![deny(missing_docs)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

// We only use our own error type; no need for From conversions provided by
// the standard library's try! macro. This reduces lines of LLVM IR.
macro_rules! tri {
    ($e:expr $(,)?) => {
        match $e {
            core::result::Result::Ok(val) => val,
            core::result::Result::Err(err) => return core::result::Result::Err(err),
        }
    };
}

mod de;
mod lexer;
mod merge;
mod parser;
mod ser;
mod style;
mod trivia;
mod writer;

pub use ser::{to_node, Serializer};
pub use style::{Indent, IndentChar, KeyStyle, Quote, Separator, Style};

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;

/// A parsed CSON document: a value tree plus the trivia needed to write it
/// back out with comments intact.
#[derive(Debug, Clone, PartialEq)]
pub struct Document<'a> {
    pub(crate) root: Node<'a>,
    /// Trivia after the last token, up to end of file.
    pub(crate) suffix: Cow<'a, str>,
    /// Whether the source omitted the outer `{` `}` (bare object root).
    pub(crate) bare_root: bool,
    pub(crate) style: Style,
}

/// A single value together with the trivia (whitespace + `#` comments) that
/// preceded it in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Node<'a> {
    /// Raw whitespace/comment text before this node. Never contains `,`,
    /// `:` or `=` -- those are emitted by the writer.
    pub(crate) prefix: Cow<'a, str>,
    pub(crate) value: Value<'a>,
}

/// A CSON value: what [`Node::value`] holds, once its trivia (comments,
/// whitespace) has been set aside.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// Any numeric literal, kept as raw text -- see [`Number`].
    Number(Number<'a>),
    /// Any string, regardless of how it was quoted in the source (or
    /// will be quoted on write) -- see [`CsonStr`].
    Str(CsonStr<'a>),
    /// `[ ... ]`.
    Array {
        /// The array's elements, in source (or insertion) order.
        items: Vec<Node<'a>>,
        /// Trivia before the closing `]`.
        trailing: Cow<'a, str>,
    },
    /// `{ ... }`, or a bare (brace-less) top-level document.
    Object {
        /// The object's key/value pairs, in source (or insertion) order.
        /// Duplicate keys are preserved as separate entries, not merged.
        entries: Vec<Entry<'a>>,
        /// Trivia before the closing `}`.
        trailing: Cow<'a, str>,
    },
}

/// One `key: value` (or `key = value`) pair inside an [`Value::Object`].
///
/// The key is a full [`Node`], not a bare string, so a comment
/// immediately before the key (rather than before the whole entry) has
/// somewhere to live.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub(crate) key: Node<'a>,
    pub(crate) value: Node<'a>,
}

/// A decoded CSON string. Quoting style is not stored here: the writer
/// always re-renders according to `Style`, never per-node layout.
#[derive(Debug, Clone, PartialEq)]
pub struct CsonStr<'a> {
    pub(crate) value: Cow<'a, str>,
}

impl<'a> CsonStr<'a> {
    /// Builds a string value from `s`. Always valid -- any content is
    /// fine, since the writer quotes and escapes it as needed -- so
    /// this is the recommended way to build a [`Value::Str`] by hand
    /// (`Value::Str(CsonStr::new("hello"))`).
    pub fn new<S: Into<Cow<'a, str>>>(s: S) -> Self {
        CsonStr { value: s.into() }
    }

    /// The decoded string content (escapes already resolved).
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// A raw number literal. Kept as text (not `f64`) so integers beyond 2^53
/// survive a round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct Number<'a> {
    pub(crate) raw: Cow<'a, str>,
}

impl<'a> Number<'a> {
    /// The number's literal text, exactly as it appeared in the source
    /// (or was formatted on write) -- e.g. `"1.50"`, not `"1.5"`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Numeric equality, ignoring the raw literal's formatting (`1.50` ==
    /// `1.5`).
    pub fn numeric_eq(&self, other: &Number<'_>) -> bool {
        if self.raw == other.raw {
            return true;
        }
        match (self.raw.parse::<f64>(), other.raw.parse::<f64>()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

/// Error produced while parsing or (de)serializing a CSON document.
///
/// Used both for genuine syntax errors (in which case `line`/`column`
/// point at the offending byte, 1-indexed) and for `serde`
/// (de)serialization errors raised via `serde::de::Error::custom`/
/// `serde::ser::Error::custom` (in which case `line`/`column` are `0`,
/// since those don't correspond to a specific position in a document).
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// Human-readable description of what went wrong.
    pub message: String,
    /// 1-indexed source line, or `0` if not applicable (see above).
    pub line: usize,
    /// 1-indexed source column, or `0` if not applicable (see above).
    pub column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {} column {}", self.message, self.line, self.column)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseError {}

/// Shorthand for `Result<T, ParseError>`, used throughout this crate's
/// public API.
pub type ParseResult<T> = Result<T, ParseError>;

impl<'a> Node<'a> {
    /// Builds a new node with an empty prefix (no comment) around
    /// `value`.
    pub fn new(value: Value<'a>) -> Self {
        Node { prefix: Cow::Borrowed(""), value }
    }

    /// This node's value.
    pub fn value(&self) -> &Value<'a> {
        &self.value
    }

    /// Mutable access to this node's value -- e.g. `match
    /// node.value_mut() { Value::Object { entries, .. } => ..., ... }`
    /// to reach into a nested structure. The node's `prefix` (and thus
    /// any comment attached to it) is untouched by mutating through
    /// this reference, which is exactly what makes it possible to
    /// change a value without losing the comment above it -- see the
    /// crate documentation's editing example.
    pub fn value_mut(&mut self) -> &mut Value<'a> {
        &mut self.value
    }

    /// Replaces this node's value, keeping its existing `prefix` (and
    /// thus any comment attached to it) untouched.
    pub fn set_value(&mut self, value: Value<'a>) {
        self.value = value;
    }

    /// Discards this node's prefix and returns its value.
    pub fn into_value(self) -> Value<'a> {
        self.value
    }

    /// The raw whitespace/comment text preceding this node in the
    /// source (or set via [`Node::set_prefix`]).
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Set the raw prefix (whitespace + `#` comments) for this node.
    ///
    /// Returns an error if `text` contains anything other than whitespace
    /// and `#`-comments, since that would silently produce invalid CSON.
    pub fn set_prefix<S: Into<Cow<'a, str>>>(&mut self, text: S) -> Result<(), &'static str> {
        let text = text.into();
        if !trivia::is_valid_prefix(&text) {
            return Err("prefix may only contain whitespace and '#' comments");
        }
        self.prefix = text;
        Ok(())
    }
}

impl<'a> Entry<'a> {
    /// This entry's key, as a full node (so a comment before the key
    /// has a prefix slot to live in).
    pub fn key(&self) -> &Node<'a> {
        &self.key
    }

    /// Mutable access to this entry's key node -- e.g. to rename a key
    /// in place (`entry.key_mut().set_value(...)`) while keeping any
    /// comment attached to it.
    pub fn key_mut(&mut self) -> &mut Node<'a> {
        &mut self.key
    }

    /// This entry's value.
    pub fn value(&self) -> &Node<'a> {
        &self.value
    }

    /// Mutable access to this entry's value node -- the way to change
    /// one field's value while leaving its comment, and every other
    /// entry, untouched. See the crate documentation's editing example.
    pub fn value_mut(&mut self) -> &mut Node<'a> {
        &mut self.value
    }

    /// The key's text, if it's a string (which, for a well-formed
    /// [`Document`], it always is -- object keys are never anything
    /// else).
    pub fn key_str(&self) -> Option<&str> {
        match &self.key.value {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Concatenates two trivia strings in source order, borrowing whichever
/// side is non-empty rather than allocating when only one side has
/// content -- the common case for [`Value::remove`]/[`Value::remove_index`],
/// where a removed node usually has no comment at all.
fn concat_prefix<'a>(a: Cow<'a, str>, b: Cow<'a, str>) -> Cow<'a, str> {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let mut s = a.into_owned();
    s.push_str(&b);
    Cow::Owned(s)
}

impl<'a> Value<'a> {
    /// Looks up `key` among this object's entries, returning its value
    /// node. `None` if `self` isn't [`Value::Object`], or has no entry
    /// with that key. Entries are searched in source order, so a
    /// duplicate key (CSON allows them; see [`Value::Object`]'s doc
    /// comment) resolves to the first match.
    pub fn get(&self, key: &str) -> Option<&Node<'a>> {
        match self {
            Value::Object { entries, .. } => {
                entries.iter().find(|e| e.key_str() == Some(key)).map(Entry::value)
            }
            _ => None,
        }
    }

    /// Mutable version of [`Value::get`].
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node<'a>> {
        match self {
            Value::Object { entries, .. } => {
                entries.iter_mut().find(|e| e.key_str() == Some(key)).map(Entry::value_mut)
            }
            _ => None,
        }
    }

    /// Appends a new `key: value` entry to this object, after every
    /// existing one -- including any existing entry with the same key;
    /// this never deduplicates (see [`Value::Object`]'s doc comment on
    /// why duplicate keys are preserved). The new entry's key node has
    /// an empty prefix (no comment); use [`Node::set_prefix`] on
    /// [`Entry::key_mut`] afterward to add one.
    ///
    /// Returns `Err` if `self` isn't [`Value::Object`] -- this never
    /// silently changes `self`'s variant.
    pub fn insert<K>(&mut self, key: K, value: Node<'a>) -> Result<(), &'static str>
    where
        K: Into<Cow<'a, str>>,
    {
        match self {
            Value::Object { entries, .. } => {
                entries.push(Entry { key: Node::new(Value::Str(CsonStr::new(key))), value });
                Ok(())
            }
            _ => Err("Value::insert called on a value that is not an object"),
        }
    }

    /// Removes and returns the entry for `key`, if `self` is
    /// [`Value::Object`] and has one (the first match, if the key is
    /// duplicated).
    ///
    /// Implements the deletion rule from the crate's design notes: the
    /// removed entry's comments (its key's prefix, then its value's, in
    /// that source order) are not discarded -- they move onto whatever
    /// now takes its place, the following entry's key prefix, or the
    /// object's `trailing` slot if the removed entry was last. The
    /// returned node's own prefix is cleared, since its content has
    /// already been relocated: reinserting the returned node elsewhere
    /// therefore can't duplicate the comment.
    pub fn remove(&mut self, key: &str) -> Option<Node<'a>> {
        match self {
            Value::Object { entries, trailing } => {
                let idx = entries.iter().position(|e| e.key_str() == Some(key))?;
                let mut removed = entries.remove(idx);
                let key_prefix = core::mem::replace(&mut removed.key.prefix, Cow::Borrowed(""));
                let value_prefix = core::mem::replace(&mut removed.value.prefix, Cow::Borrowed(""));
                let moved = concat_prefix(key_prefix, value_prefix);
                if idx < entries.len() {
                    let next_prefix = core::mem::replace(&mut entries[idx].key.prefix, Cow::Borrowed(""));
                    entries[idx].key.prefix = concat_prefix(moved, next_prefix);
                } else {
                    let old_trailing = core::mem::replace(trailing, Cow::Borrowed(""));
                    *trailing = concat_prefix(moved, old_trailing);
                }
                Some(removed.value)
            }
            _ => None,
        }
    }

    /// Element at `index`, if `self` is [`Value::Array`] and `index` is
    /// in bounds. A thin wrapper (`items.get(index)` works just as well
    /// once you've matched into [`Value::Array`]) kept for symmetry with
    /// [`Value::get`].
    pub fn get_index(&self, index: usize) -> Option<&Node<'a>> {
        match self {
            Value::Array { items, .. } => items.get(index),
            _ => None,
        }
    }

    /// Mutable version of [`Value::get_index`].
    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut Node<'a>> {
        match self {
            Value::Array { items, .. } => items.get_mut(index),
            _ => None,
        }
    }

    /// Appends `value` to this array, after every existing element.
    ///
    /// Returns `Err` if `self` isn't [`Value::Array`] -- this never
    /// silently changes `self`'s variant.
    pub fn push(&mut self, value: Node<'a>) -> Result<(), &'static str> {
        match self {
            Value::Array { items, .. } => {
                items.push(value);
                Ok(())
            }
            _ => Err("Value::push called on a value that is not an array"),
        }
    }

    /// Removes and returns the element at `index`, if `self` is
    /// [`Value::Array`] and `index` is in bounds.
    ///
    /// Same comment-migration rule as [`Value::remove`]: the removed
    /// element's prefix moves onto the next element's prefix, or the
    /// array's `trailing` slot if the removed element was last; the
    /// returned node's own prefix is cleared.
    pub fn remove_index(&mut self, index: usize) -> Option<Node<'a>> {
        match self {
            Value::Array { items, trailing } => {
                if index >= items.len() {
                    return None;
                }
                let mut removed = items.remove(index);
                let moved = core::mem::replace(&mut removed.prefix, Cow::Borrowed(""));
                if index < items.len() {
                    let next_prefix = core::mem::replace(&mut items[index].prefix, Cow::Borrowed(""));
                    items[index].prefix = concat_prefix(moved, next_prefix);
                } else {
                    let old_trailing = core::mem::replace(trailing, Cow::Borrowed(""));
                    *trailing = concat_prefix(moved, old_trailing);
                }
                Some(removed)
            }
            _ => None,
        }
    }
}

impl Value<'static> {
    /// Builds a value from any `Serialize` type, via the same Schicht 3
    /// machinery [`crate::to_node`]/[`Document::from_serialize`] use.
    ///
    /// This is the recommended way to build a replacement
    /// [`Value::Number`] by hand (there's no public raw-literal
    /// constructor for `Number`, since a hand-written literal could be
    /// syntactically invalid CSON and there would be nothing to catch
    /// that until write time): `Value::from_serialize(&42i64)?` always
    /// produces a valid one. It works equally well for whole nested
    /// structures, e.g. `Value::from_serialize(&my_struct)?`.
    pub fn from_serialize<T>(value: &T) -> ParseResult<Self>
    where
        T: ?Sized + Serialize,
    {
        Ok(ser::to_node(value)?.into_value())
    }
}

impl<'a> Document<'a> {
    /// The document's root node (an object, or an array for a
    /// braced/bracketed top level -- see [`Document::bare_root`]).
    pub fn root(&self) -> &Node<'a> {
        &self.root
    }

    /// Mutable access to the root node, for hand-editing a value while
    /// keeping every other node's `prefix` (and thus every comment)
    /// intact. See the crate documentation's editing example.
    pub fn root_mut(&mut self) -> &mut Node<'a> {
        &mut self.root
    }

    /// Trivia (whitespace/comments) after the last token, up to end of
    /// file -- e.g. a trailing `# note` with nothing after it.
    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// Whether the source omitted the outer `{` `}` (a bare object
    /// root, `ws object-items` in the grammar). Never `true` for an
    /// array: CSON only allows a bare root for objects.
    pub fn bare_root(&self) -> bool {
        self.bare_root
    }

    /// The layout choices [`Document::to_cson_string`] renders with.
    /// Detected first-match from the source by [`parse`], or
    /// [`Style::default`] for a document built via
    /// [`Document::from_serialize`].
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// Override this document's [`Style`] -- e.g. to force a particular
    /// separator or quote character regardless of what the source used.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Schicht 2: deserialize the root node into a typed Rust value,
    /// without going through `T`'s usual text round trip. The document
    /// is left untouched (and keeps its comments), so the typed value
    /// can be edited and written back with [`Document::merge_from`].
    /// See `de.rs`.
    pub fn deserialize<'de, T>(&'de self) -> ParseResult<T>
    where
        T: serde::de::Deserialize<'de>,
    {
        T::deserialize(self.root())
    }

    /// Writes a typed value back into this document, changing only what
    /// actually differs and leaving every comment (and every unchanged
    /// value's exact source text) in place.
    ///
    /// This is the other half of [`Document::deserialize`], and the
    /// reason the two together are more than
    /// [`Document::from_serialize`]: `from_serialize` builds a **brand
    /// new** tree, with no comments and a default [`Style`], so using it
    /// to write an edit back would discard everything this crate exists
    /// to preserve. `merge_from` instead serializes `value` to a fresh
    /// tree and *diffs* it against this document:
    ///
    /// ```
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Serialize, Deserialize)]
    /// struct Config { name: String, port: u16 }
    ///
    /// # fn main() -> Result<(), cson_edit::ParseError> {
    /// let mut doc = cson_edit::parse("name: \"svc\"\nport: 8080  # ops override\n")?;
    /// let mut cfg: Config = doc.deserialize()?;
    ///
    /// cfg.port = 9090;
    /// doc.merge_from(&cfg)?;
    ///
    /// let text = doc.to_cson_string();
    /// assert!(text.contains("9090"));
    /// assert!(text.contains("# ops override")); // comment survived
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The rules, in full:
    ///
    /// * **Unchanged scalars are not touched at all.** Numbers are
    ///   compared numerically rather than textually, so a document's
    ///   `1.50` isn't rewritten to `1.5` just because that's how the
    ///   typed value formats; the raw literal is kept whenever the value
    ///   is unchanged.
    /// * **Changed scalars** have only their value replaced. The
    ///   surrounding [`Node`] -- and therefore its `prefix`, where its
    ///   comment lives -- is never replaced.
    /// * **Objects**: keys in both are merged recursively, in the
    ///   document's existing order; keys only in `value` are appended
    ///   after the existing ones; keys only in the document are removed
    ///   via [`Value::remove`], so their comments migrate rather than
    ///   disappear.
    /// * **Arrays**: elements are merged position by position; extra
    ///   elements in `value` are appended, extra elements in the
    ///   document are removed via [`Value::remove_index`] (same comment
    ///   migration).
    /// * **A shape change** (a field that was a number is now an
    ///   object, say) has no meaningful field-by-field diff, so that
    ///   node's value is replaced wholesale.
    ///
    /// Two caveats worth knowing before you rely on it:
    ///
    /// * **`#[serde(skip)]` and `skip_serializing_if` interact badly
    ///   with the "only in the document → remove" rule.** A field that
    ///   `T` deliberately doesn't serialize is indistinguishable, in the
    ///   fresh tree, from a key the caller wants gone -- so it will be
    ///   removed from the document. If `T` skips fields that the file is
    ///   meant to keep, edit those keys by hand
    ///   ([`Value::get_mut`]/[`Node::set_value`]) instead of merging.
    /// * **Duplicate keys**: CSON keeps them (see [`Value::Object`]),
    ///   but a serialized `T` never has any. The first entry with a
    ///   given key is the one merged into; later duplicates of that same
    ///   key are left untouched, matching [`Value::get`]/
    ///   [`Value::remove`]'s first-match convention.
    pub fn merge_from<T>(&mut self, value: &T) -> ParseResult<()>
    where
        T: ?Sized + Serialize,
    {
        let fresh = tri!(ser::to_node(value));
        merge::merge_value(self.root.value_mut(), fresh.into_value());
        Ok(())
    }
}

impl Document<'static> {
    /// Schicht 3: serialize a typed Rust value into a fresh `Document`
    /// (default `Style`, no comments -- there is no source layout to
    /// take one from). See `ser.rs`.
    pub fn from_serialize<T>(value: &T) -> ParseResult<Self>
    where
        T: ?Sized + serde::ser::Serialize,
    {
        let root = tri!(ser::to_node(value));
        Ok(Document { root, suffix: Cow::Borrowed(""), bare_root: false, style: Style::default() })
    }
}

/// Parse a CSON document from text, preserving comments for a later write.
pub fn parse(input: &str) -> ParseResult<Document<'_>> {
    parser::parse_document(input)
}

impl<'a> fmt::Display for Document<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&writer::write_document(self))
    }
}

impl<'a> Document<'a> {
    /// Serialize the document back to CSON text.
    pub fn to_cson_string(&self) -> String {
        writer::write_document(self)
    }
}

// ---------------------------------------------------------------------
// The conventional top-level API a serde data-format crate is expected
// to have (`from_str`/`to_string`/`from_reader`/`to_writer`, matching
// serde_json/serde_yaml/toml naming), layered on top of Schicht 1 + 2/3
// above. `T` here must be fully owned (`DeserializeOwned`, i.e. `for<'de>
// Deserialize<'de>`): the `Document` these functions parse into is a
// local temporary that doesn't outlive the call, so a `T` borrowing
// straight from the input (`&str` fields and the like) can't be
// expressed through this convenience layer. Call [`parse`] yourself and
// keep the `Document` alive if you need that.

/// Deserialize an instance of `T` from a string of CSON text.
///
/// A thin wrapper around [`parse`] + [`Document::deserialize`] for when
/// you don't need to keep the parsed [`Document`] (and its comments)
/// around afterward.
pub fn from_str<T>(s: &str) -> ParseResult<T>
where
    T: DeserializeOwned,
{
    parse(s)?.deserialize()
}

/// Deserialize an instance of `T` from CSON text read from a UTF-8 byte
/// slice.
pub fn from_slice<T>(v: &[u8]) -> ParseResult<T>
where
    T: DeserializeOwned,
{
    let s = match core::str::from_utf8(v) {
        Ok(s) => s,
        Err(e) => return Err(ParseError { message: e.to_string(), line: 0, column: 0 }),
    };
    from_str(s)
}

/// Serialize `value` as a `String` of CSON text.
///
/// A thin wrapper around [`Document::from_serialize`] +
/// [`Document::to_cson_string`]: default `Style`, no comments, since
/// there's no source document to take either from. See
/// [`Document::from_serialize`] for that trade-off in more detail.
pub fn to_string<T>(value: &T) -> ParseResult<String>
where
    T: ?Sized + Serialize,
{
    Ok(Document::from_serialize(value)?.to_cson_string())
}

/// Serialize `value` as a CSON byte vector (UTF-8).
pub fn to_vec<T>(value: &T) -> ParseResult<Vec<u8>>
where
    T: ?Sized + Serialize,
{
    Ok(to_string(value)?.into_bytes())
}

#[cfg(feature = "std")]
fn io_error(e: std::io::Error) -> ParseError {
    ParseError { message: e.to_string(), line: 0, column: 0 }
}

/// Deserialize an instance of `T` from CSON text read from an
/// `io::Read` stream.
///
/// Reads the whole stream into memory first (this crate parses a
/// complete document in one pass, not incrementally), so this offers no
/// advantage over [`from_str`] beyond convenience with an existing
/// reader; there is no streaming/multi-document support.
#[cfg(feature = "std")]
pub fn from_reader<R, T>(mut reader: R) -> ParseResult<T>
where
    R: std::io::Read,
    T: DeserializeOwned,
{
    let mut buf = String::new();
    match std::io::Read::read_to_string(&mut reader, &mut buf) {
        Ok(_) => from_str(&buf),
        Err(e) => Err(io_error(e)),
    }
}

/// Serialize `value` as CSON text to an `io::Write` stream.
#[cfg(feature = "std")]
pub fn to_writer<W, T>(mut writer: W, value: &T) -> ParseResult<()>
where
    W: std::io::Write,
    T: ?Sized + Serialize,
{
    let s = to_string(value)?;
    match std::io::Write::write_all(&mut writer, s.as_bytes()) {
        Ok(()) => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

#[cfg(test)]
mod serde_tests;
#[cfg(test)]
mod tests;
