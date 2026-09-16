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
//! `merge_from` — diffing a freshly `Serialize`d tree against an
//! existing, comment-carrying one so a typed edit only touches the
//! fields that changed — is not implemented yet; see `ser.rs`'s doc
//! comment for why Schicht 2 + 3 don't already add up to that.
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
#![allow(missing_docs)]

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

#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Null,
    Bool(bool),
    Number(Number<'a>),
    Str(CsonStr<'a>),
    Array {
        items: Vec<Node<'a>>,
        /// Trivia before the closing `]`.
        trailing: Cow<'a, str>,
    },
    Object {
        entries: Vec<Entry<'a>>,
        /// Trivia before the closing `}`.
        trailing: Cow<'a, str>,
    },
}

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

/// Error produced while parsing a CSON document.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {} column {}", self.message, self.line, self.column)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

impl<'a> Node<'a> {
    pub fn value(&self) -> &Value<'a> {
        &self.value
    }

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
    pub fn key(&self) -> &Node<'a> {
        &self.key
    }

    pub fn value(&self) -> &Node<'a> {
        &self.value
    }

    pub fn key_str(&self) -> Option<&str> {
        match &self.key.value {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl<'a> Document<'a> {
    pub fn root(&self) -> &Node<'a> {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut Node<'a> {
        &mut self.root
    }

    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    pub fn bare_root(&self) -> bool {
        self.bare_root
    }

    pub fn style(&self) -> &Style {
        &self.style
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Schicht 2: deserialize the root node into a typed Rust value,
    /// without going through `T`'s usual text round trip (so a later
    /// `Document::from_serialize` + diff -- `merge_from`, not yet
    /// implemented -- could in principle still see this document's
    /// comments). See `de.rs`.
    pub fn deserialize<'de, T>(&'de self) -> ParseResult<T>
    where
        T: serde::de::Deserialize<'de>,
    {
        T::deserialize(self.root())
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
