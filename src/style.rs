//! First-match layout detection.
//!
//! Each field of [`Style`] is set at most once, at the first time the
//! corresponding source construct is seen while parsing. The writer
//! then renders the *entire* output using these choices, uniformly --
//! never per-node.

use super::lexer::QuoteKind;
use alloc::string::String;

/// The key/value separator a [`Style`] renders with: `:` or CSON's `=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    /// `key: value`.
    Colon,
    /// `key = value`.
    Equals,
}

/// The quote character a [`Style`] renders string values (and, per
/// [`KeyStyle::Double`]/[`KeyStyle::Single`], quoted keys) with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// `"value"`.
    Double,
    /// `'value'`.
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

/// How a [`Style`] writes object keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStyle {
    /// Unquoted, e.g. `key: 1` -- used whenever the key text is a valid
    /// bare identifier; a key that doesn't qualify (contains whitespace,
    /// say) falls back to quoting even under this style.
    Bare,
    /// `"key": 1`.
    Double,
    /// `'key': 1`.
    Single,
}

/// The character a [`Style`]'s [`Indent`] is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentChar {
    /// U+0020 SPACE.
    Space,
    /// U+0009 CHARACTER TABULATION.
    Tab,
}

/// One level of indentation: a repeated character and a width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    /// The character to repeat.
    pub ch: IndentChar,
    /// How many times to repeat it, per nesting level.
    pub width: usize,
}

impl Indent {
    /// Renders the indentation for nesting level `depth` (0 = no
    /// indentation).
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
    /// Two spaces per level.
    fn default() -> Self {
        Indent {
            ch: IndentChar::Space,
            width: 2,
        }
    }
}

/// Detected (or defaulted) layout choices for a [`super::Document`].
///
/// Every field is `Option`-typed and first-match latched: [`super::parse`]
/// sets each one the first time it observes the corresponding source
/// construct, and never touches it again, even if a later, different
/// construct appears (a mixed-style source keeps whatever it saw
/// first). A field left `None` -- because the source never happened to
/// exercise that dimension -- falls back to a fixed default; use the
/// getter methods below (not the fields directly) to get that
/// resolved, always-`Some` value. Construct one by hand (or start from
/// [`Style::default`]) and pass it to [`super::Document::set_style`] to
/// override what was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    /// See [`Style::separator`]. Defaults to [`Separator::Colon`].
    pub separator: Option<Separator>,
    /// See [`Style::value_quote`]. Defaults to [`Quote::Double`].
    pub value_quote: Option<Quote>,
    /// See [`Style::key_style`]. Defaults to [`KeyStyle::Bare`].
    pub key_style: Option<KeyStyle>,
    /// `true`: always write an explicit `,`. `false`: omit it across a
    /// newline (rely on the newline-as-separator rule). Defaults to
    /// `true`.
    pub commas: Option<bool>,
    /// Whether to write a trailing comma before the closing bracket.
    /// Defaults to `false`.
    pub trailing_comma: Option<bool>,
    /// No reliable first-match source (needs a nested element to observe);
    /// always falls back to the default (two spaces).
    pub indent: Option<Indent>,
    /// Whether a multi-line string value is written using `|`-prefixed
    /// verbatim lines instead of an escaped quoted string. No reliable
    /// first-match source either -- many documents never use one --
    /// so this defaults to `false` (plain quoting) even for multi-line
    /// values.
    pub verbatim_strings: Option<bool>,
}

impl Style {
    /// The key/value separator to render with, resolved to its default
    /// ([`Separator::Colon`]) if never observed.
    pub fn separator(&self) -> Separator {
        self.separator.unwrap_or(Separator::Colon)
    }

    /// The quote character for string values, resolved to its default
    /// ([`Quote::Double`]) if never observed.
    pub fn value_quote(&self) -> Quote {
        self.value_quote.unwrap_or(Quote::Double)
    }

    /// How to render object keys, resolved to its default
    /// ([`KeyStyle::Bare`]) if never observed.
    pub fn key_style(&self) -> KeyStyle {
        self.key_style.unwrap_or(KeyStyle::Bare)
    }

    /// Whether to write explicit commas between entries/items, resolved
    /// to its default (`true`) if never observed.
    pub fn commas(&self) -> bool {
        self.commas.unwrap_or(true)
    }

    /// Whether to write a trailing comma before a closing bracket,
    /// resolved to its default (`false`) if never observed.
    pub fn trailing_comma(&self) -> bool {
        self.trailing_comma.unwrap_or(false)
    }

    /// The indentation to render with, resolved to its default (two
    /// spaces) if never observed.
    pub fn indent(&self) -> Indent {
        self.indent.unwrap_or_default()
    }

    /// Whether to render multi-line string values as `|`-prefixed
    /// verbatim lines, resolved to its default (`false`) if never
    /// observed.
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
