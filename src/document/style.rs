//! First-match layout detection, per `CLAUDE.md`.
//!
//! Each field is set at most once, at the first time the corresponding
//! source construct is seen while parsing. The writer then renders the
//! *entire* output using these choices, uniformly -- never per-node.

use super::lexer::QuoteKind;
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Colon,
    Equals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Double,
    Single,
}

impl From<QuoteKind> for Quote {
    fn from(q: QuoteKind) -> Self {
        match q {
            QuoteKind::Double => Quote::Double,
            QuoteKind::Single => Quote::Single,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStyle {
    Bare,
    Double,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentChar {
    Space,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub ch: IndentChar,
    pub width: usize,
}

impl Indent {
    pub fn render(&self, depth: usize) -> String {
        let ch = match self.ch {
            IndentChar::Space => ' ',
            IndentChar::Tab => '\t',
        };
        let mut s = String::with_capacity(self.width * depth);
        for _ in 0..self.width * depth {
            s.push(ch);
        }
        s
    }
}

impl Default for Indent {
    fn default() -> Self {
        Indent {
            ch: IndentChar::Space,
            width: 2,
        }
    }
}

/// Detected (or defaulted) layout choices for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub separator: Option<Separator>,
    pub value_quote: Option<Quote>,
    pub key_style: Option<KeyStyle>,
    /// `true`: always write an explicit `,`. `false`: omit it across a
    /// newline (rely on the newline-as-separator rule).
    pub commas: Option<bool>,
    pub trailing_comma: Option<bool>,
    /// No reliable first-match source (needs a nested element to observe);
    /// always falls back to the default.
    pub indent: Option<Indent>,
    /// No reliable first-match source either; many files never use one.
    pub verbatim_strings: Option<bool>,
}

impl Style {
    pub fn separator(&self) -> Separator {
        self.separator.unwrap_or(Separator::Colon)
    }

    pub fn value_quote(&self) -> Quote {
        self.value_quote.unwrap_or(Quote::Double)
    }

    pub fn key_style(&self) -> KeyStyle {
        self.key_style.unwrap_or(KeyStyle::Bare)
    }

    pub fn commas(&self) -> bool {
        self.commas.unwrap_or(true)
    }

    pub fn trailing_comma(&self) -> bool {
        self.trailing_comma.unwrap_or(false)
    }

    pub fn indent(&self) -> Indent {
        self.indent.unwrap_or_default()
    }

    pub fn verbatim_strings(&self) -> bool {
        self.verbatim_strings.unwrap_or(false)
    }

    pub(crate) fn note_separator(&mut self, s: Separator) {
        self.separator.get_or_insert(s);
    }

    pub(crate) fn note_value_quote(&mut self, q: Quote) {
        self.value_quote.get_or_insert(q);
    }

    pub(crate) fn note_key_style(&mut self, k: KeyStyle) {
        self.key_style.get_or_insert(k);
    }

    pub(crate) fn note_commas(&mut self, c: bool) {
        self.commas.get_or_insert(c);
    }

    pub(crate) fn note_trailing_comma(&mut self, t: bool) {
        self.trailing_comma.get_or_insert(t);
    }

    pub(crate) fn note_verbatim_strings(&mut self, v: bool) {
        self.verbatim_strings.get_or_insert(v);
    }
}
