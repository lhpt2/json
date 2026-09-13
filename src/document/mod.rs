//! CSON's comment-preserving document model (Schicht 1).
//!
//! This module implements the "Document -> Node" tree described in
//! `CLAUDE.md`: a parse tree that keeps comments and blank lines around so a
//! configuration file can be edited and written back without losing them.
//! It is intentionally independent from `serde`: see `CLAUDE.md` for why the
//! `Deserializer`/`Serializer` layers must be built on top of this, not the
//! other way around.
//!
//! This is Schicht 1 (+ the style/writer step) of the architecture
//! described in `CLAUDE.md`; layers 2 and 3 (`serde` integration and
//! `merge_from`) build on top of it later, so most getters here are
//! deliberately minimal rather than a full editing API.

#![allow(missing_docs)]

mod lexer;
mod parser;
mod style;
mod trivia;
mod writer;

pub use style::{Indent, IndentChar, KeyStyle, Quote, Separator, Style};

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

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

#[cfg(test)]
mod tests;
