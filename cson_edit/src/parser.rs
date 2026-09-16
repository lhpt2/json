//! Recursive-descent parser building a [`super::Document`] tree.
//!
//! Runs after the lexer and the comment-reattachment pass, so every
//! remaining comment is already parked in the trivia slot of the real
//! token that follows it (see [`super::trivia`]). This parser only has to
//! build the value tree and, along the way, latch the first-seen [`Style`]
//! choices.

use super::lexer::{self, Item, QuoteKind, RealTok, Span};
use super::style::{KeyStyle, Separator, Style};
use super::trivia;
use super::{CsonStr, Document, Entry, Node, Number, ParseError, ParseResult, Value};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

struct Parser<'a> {
    input: &'a str,
    toks: Vec<RealTok<'a>>,
    spans: Vec<Span>,
    prefixes: Vec<String>,
    blank_line: Vec<bool>,
    had_newline: Vec<bool>,
    pos: usize,
    style: Style,
}

pub fn parse_document(input: &str) -> ParseResult<Document<'_>> {
    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let items = tri!(lexer::lex(input));
    let info = trivia::resolve(&items);

    let mut toks = Vec::with_capacity(items.len());
    let mut spans = Vec::with_capacity(items.len());
    for item in items {
        if let Item::Real(tok, span) = item {
            toks.push(tok);
            spans.push(span);
        }
    }
    let n = toks.len();

    let mut p = Parser {
        input,
        toks,
        spans,
        prefixes: info.prefixes,
        blank_line: info.blank_line,
        had_newline: info.had_newline,
        pos: 0,
        style: Style::default(),
    };

    if n == 0 {
        let suffix = p.take_prefix(0);
        return Ok(Document {
            root: Node {
                prefix: Cow::Borrowed(""),
                value: Value::Object { entries: Vec::new(), trailing: Cow::Borrowed("") },
            },
            suffix,
            bare_root: true,
            style: p.style,
        });
    }

    let (root, bare_root) = match p.toks[0] {
        RealTok::LBrace => {
            let prefix = p.take_prefix(0);
            p.pos = 1;
            let (entries, trailing) = tri!(p.parse_object_body());
            (Node { prefix, value: Value::Object { entries, trailing } }, false)
        }
        RealTok::LBracket => {
            let prefix = p.take_prefix(0);
            p.pos = 1;
            let (items, trailing) = tri!(p.parse_array_body());
            (Node { prefix, value: Value::Array { items, trailing } }, false)
        }
        _ => {
            let entries = tri!(p.parse_bare_root_entries());
            (
                Node {
                    prefix: Cow::Borrowed(""),
                    value: Value::Object { entries, trailing: Cow::Borrowed("") },
                },
                true,
            )
        }
    };

    if p.pos != n {
        return Err(p.error_here("unexpected trailing content after root value"));
    }
    let suffix = p.take_prefix(n);
    Ok(Document { root, suffix, bare_root, style: p.style })
}

impl<'a> Parser<'a> {
    fn take_prefix(&mut self, idx: usize) -> Cow<'a, str> {
        Cow::Owned(core::mem::take(&mut self.prefixes[idx]))
    }

    fn gap_has_newline(&self, idx: usize) -> bool {
        self.had_newline.get(idx).copied().unwrap_or(false)
    }

    fn error_here(&self, message: &str) -> ParseError {
        let pos = self
            .spans
            .get(self.pos)
            .map(|s| s.start)
            .unwrap_or(self.input.len());
        lexer::error_at(self.input, pos, alloc::string::ToString::to_string(message))
    }

    fn eof_error(&self, message: &str) -> ParseError {
        lexer::error_at(self.input, self.input.len(), alloc::string::ToString::to_string(message))
    }

    fn parse_value(&mut self) -> ParseResult<Node<'a>> {
        let idx = self.pos;
        let prefix = self.take_prefix(idx);
        match self.toks.get(self.pos) {
            None => Err(self.eof_error("expected a value")),
            Some(RealTok::LBrace) => {
                self.pos += 1;
                let (entries, trailing) = tri!(self.parse_object_body());
                Ok(Node { prefix, value: Value::Object { entries, trailing } })
            }
            Some(RealTok::LBracket) => {
                self.pos += 1;
                let (items, trailing) = tri!(self.parse_array_body());
                Ok(Node { prefix, value: Value::Array { items, trailing } })
            }
            Some(RealTok::Number(raw)) => {
                let raw = *raw;
                self.pos += 1;
                Ok(Node { prefix, value: Value::Number(Number { raw: Cow::Borrowed(raw) }) })
            }
            Some(RealTok::Str(s, quote)) => {
                let quote = super::style::Quote::from(*quote);
                self.style.note_value_quote(quote);
                let s = s.clone();
                self.pos += 1;
                Ok(Node { prefix, value: Value::Str(CsonStr { value: s }) })
            }
            Some(RealTok::Bare(text)) => {
                let text = *text;
                self.pos += 1;
                match text {
                    "true" => Ok(Node { prefix, value: Value::Bool(true) }),
                    "false" => Ok(Node { prefix, value: Value::Bool(false) }),
                    "null" => Ok(Node { prefix, value: Value::Null }),
                    _ => Err(self.error_here(
                        "a bare (unquoted) string is only allowed as an object key, not as a value",
                    )),
                }
            }
            Some(RealTok::VerbatimFragment(_)) => self.parse_verbatim_value(prefix),
            Some(_) => Err(self.error_here("expected a value")),
        }
    }

    fn parse_verbatim_value(&mut self, prefix: Cow<'a, str>) -> ParseResult<Node<'a>> {
        let mut parts: Vec<&'a str> = Vec::new();
        match self.toks.get(self.pos) {
            Some(RealTok::VerbatimFragment(text)) => {
                parts.push(text);
                self.pos += 1;
            }
            _ => unreachable!("parse_verbatim_value called on a non-fragment token"),
        }
        let mut extra_prefix = String::new();
        loop {
            let is_next_fragment = matches!(self.toks.get(self.pos), Some(RealTok::VerbatimFragment(_)));
            let breaks_merge = self.blank_line.get(self.pos).copied().unwrap_or(false);
            if is_next_fragment && !breaks_merge {
                // A comment sitting between two fragments is `ws`, not a
                // fragment continuation; there's no per-fragment prefix
                // slot in the data model, so fold it into this node's own
                // prefix. `write(parse(x))` is then a fixpoint from the
                // first run on, even though it visibly relocates the
                // comment -- see CLAUDE.md's "known accepted precision
                // loss" precedent for the analogous same-line case.
                let gap_text = core::mem::take(&mut self.prefixes[self.pos]);
                extra_prefix.push_str(&gap_text);
                if let Some(RealTok::VerbatimFragment(text)) = self.toks.get(self.pos) {
                    parts.push(text);
                }
                self.pos += 1;
            } else {
                break;
            }
        }
        if parts.len() > 1 {
            // A lone fragment with no continuation has no newline in its
            // content, so writing it back as a plain quoted string is
            // equally valid and keeps the verbatim-style latch from
            // firing on a case that wouldn't survive invariant 4 (see the
            // failing-test note this replaced).
            self.style.note_verbatim_strings(true);
        }
        let joined = parts.join("\n");
        let mut prefix_owned = prefix.into_owned();
        prefix_owned.push_str(&extra_prefix);
        Ok(Node { prefix: Cow::Owned(prefix_owned), value: Value::Str(CsonStr { value: Cow::Owned(joined) }) })
    }

    fn parse_key(&mut self) -> ParseResult<Node<'a>> {
        let idx = self.pos;
        let prefix = self.take_prefix(idx);
        match self.toks.get(self.pos) {
            Some(RealTok::Str(s, quote)) => {
                self.style.note_key_style(match quote {
                    QuoteKind::Double => KeyStyle::Double,
                    QuoteKind::Single => KeyStyle::Single,
                });
                let s = s.clone();
                self.pos += 1;
                Ok(Node { prefix, value: Value::Str(CsonStr { value: s }) })
            }
            Some(RealTok::Bare(text)) => {
                self.style.note_key_style(KeyStyle::Bare);
                let text = *text;
                self.pos += 1;
                Ok(Node { prefix, value: Value::Str(CsonStr { value: Cow::Borrowed(text) }) })
            }
            _ => Err(self.error_here("expected an object key")),
        }
    }

    fn expect_separator(&mut self) -> ParseResult<()> {
        match self.toks.get(self.pos) {
            Some(RealTok::Colon) => {
                self.style.note_separator(Separator::Colon);
                self.pos += 1;
                Ok(())
            }
            Some(RealTok::Equals) => {
                self.style.note_separator(Separator::Equals);
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.error_here("expected ':' or '='")),
        }
    }

    fn parse_object_body(&mut self) -> ParseResult<(Vec<Entry<'a>>, Cow<'a, str>)> {
        let mut entries = Vec::new();
        let mut trailing_comma_seen;
        loop {
            trailing_comma_seen = false;
            if matches!(self.toks.get(self.pos), Some(RealTok::RBrace)) {
                break;
            }
            if self.pos >= self.toks.len() {
                return Err(self.eof_error("expected '}'"));
            }
            let key = tri!(self.parse_key());
            tri!(self.expect_separator());
            let value = tri!(self.parse_value());
            entries.push(Entry { key, value });
            match self.toks.get(self.pos) {
                Some(RealTok::Comma) => {
                    self.style.note_commas(true);
                    self.pos += 1;
                    if matches!(self.toks.get(self.pos), Some(RealTok::RBrace)) {
                        trailing_comma_seen = true;
                        break;
                    }
                }
                Some(RealTok::RBrace) => break,
                _ => {
                    if !self.gap_has_newline(self.pos) {
                        return Err(self.error_here("expected ',' or a newline between object entries"));
                    }
                    self.style.note_commas(false);
                }
            }
        }
        if !entries.is_empty() {
            self.style.note_trailing_comma(trailing_comma_seen);
        }
        if !matches!(self.toks.get(self.pos), Some(RealTok::RBrace)) {
            return Err(self.eof_error("expected '}'"));
        }
        let trailing = self.take_prefix(self.pos);
        self.pos += 1;
        Ok((entries, trailing))
    }

    fn parse_array_body(&mut self) -> ParseResult<(Vec<Node<'a>>, Cow<'a, str>)> {
        let mut items = Vec::new();
        let mut trailing_comma_seen;
        loop {
            trailing_comma_seen = false;
            if matches!(self.toks.get(self.pos), Some(RealTok::RBracket)) {
                break;
            }
            if self.pos >= self.toks.len() {
                return Err(self.eof_error("expected ']'"));
            }
            let item = tri!(self.parse_value());
            items.push(item);
            match self.toks.get(self.pos) {
                Some(RealTok::Comma) => {
                    self.style.note_commas(true);
                    self.pos += 1;
                    if matches!(self.toks.get(self.pos), Some(RealTok::RBracket)) {
                        trailing_comma_seen = true;
                        break;
                    }
                }
                Some(RealTok::RBracket) => break,
                _ => {
                    if !self.gap_has_newline(self.pos) {
                        return Err(self.error_here("expected ',' or a newline between array elements"));
                    }
                    self.style.note_commas(false);
                }
            }
        }
        if !items.is_empty() {
            self.style.note_trailing_comma(trailing_comma_seen);
        }
        if !matches!(self.toks.get(self.pos), Some(RealTok::RBracket)) {
            return Err(self.eof_error("expected ']'"));
        }
        let trailing = self.take_prefix(self.pos);
        self.pos += 1;
        Ok((items, trailing))
    }

    fn parse_bare_root_entries(&mut self) -> ParseResult<Vec<Entry<'a>>> {
        let mut entries = Vec::new();
        loop {
            if self.pos >= self.toks.len() {
                break;
            }
            let key = tri!(self.parse_key());
            tri!(self.expect_separator());
            let value = tri!(self.parse_value());
            entries.push(Entry { key, value });
            match self.toks.get(self.pos) {
                Some(RealTok::Comma) => {
                    self.style.note_commas(true);
                    self.pos += 1;
                    if self.pos >= self.toks.len() {
                        break;
                    }
                }
                None => break,
                _ => {
                    if !self.gap_has_newline(self.pos) {
                        return Err(self.error_here("expected ',' or a newline between object entries"));
                    }
                    self.style.note_commas(false);
                }
            }
        }
        Ok(entries)
    }
}
