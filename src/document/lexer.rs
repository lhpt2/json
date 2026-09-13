//! Tokenizer for CSON source text.
//!
//! Produces a flat, source-ordered stream of [`Item`]s: real (meaningful)
//! tokens plus the two trivia atoms the line-anchor pass in
//! [`super::trivia`] needs (`Comment`, `Newline`). Plain whitespace is
//! consumed silently and never represented -- the writer regenerates all
//! indentation from `Style`, so no information is lost by dropping it.
//!
//! Verbatim-string fragments (`|...`) are lexed as a single token that
//! swallows the rest of the line, `#` included, so the comment-reattachment
//! pass never has to special-case verbatim content.

use super::{ParseError, ParseResult};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub enum RealTok<'a> {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Equals,
    Comma,
    Number(&'a str),
    /// Decoded string content (escapes already resolved).
    Str(Cow<'a, str>, QuoteKind),
    /// A run of `id-start id-end*` characters; the parser decides whether
    /// this is a keyword (`true`/`false`/`null`), a bare key, or an error.
    Bare(&'a str),
    /// Raw text after `|` up to (excluding) the line break. Escapes are
    /// *not* processed and `#` is *not* a comment start in here.
    VerbatimFragment(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Double,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item<'a> {
    Real(RealTok<'a>, Span),
    /// Full comment text including the leading `#`, excluding the
    /// terminating CR/LF.
    Comment(&'a str),
    /// One logical line break: a lone CR, a lone LF, or a CRLF pair.
    Newline,
}

pub fn lex(input: &str) -> ParseResult<Vec<Item<'_>>> {
    let bytes = input.as_bytes();
    let mut items = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' => {
                i += 1;
            }
            b'\r' => {
                items.push(Item::Newline);
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
            }
            b'\n' => {
                items.push(Item::Newline);
                i += 1;
            }
            b'#' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
                items.push(Item::Comment(&input[start..i]));
            }
            b'{' => {
                items.push(Item::Real(RealTok::LBrace, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b'}' => {
                items.push(Item::Real(RealTok::RBrace, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b'[' => {
                items.push(Item::Real(RealTok::LBracket, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b']' => {
                items.push(Item::Real(RealTok::RBracket, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b':' => {
                items.push(Item::Real(RealTok::Colon, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b'=' => {
                items.push(Item::Real(RealTok::Equals, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b',' => {
                items.push(Item::Real(RealTok::Comma, Span { start: i, end: i + 1 }));
                i += 1;
            }
            b'|' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
                items.push(Item::Real(
                    RealTok::VerbatimFragment(&input[start + 1..i]),
                    Span { start, end: i },
                ));
            }
            b'"' => {
                let (s, end) = tri!(lex_string(input, i, b'"'));
                items.push(Item::Real(RealTok::Str(s, QuoteKind::Double), Span { start: i, end }));
                i = end;
            }
            b'\'' => {
                let (s, end) = tri!(lex_string(input, i, b'\''));
                items.push(Item::Real(RealTok::Str(s, QuoteKind::Single), Span { start: i, end }));
                i = end;
            }
            b'-' | b'0'..=b'9' => {
                if c == b'-' && !matches!(bytes.get(i + 1), Some(b'0'..=b'9')) {
                    // Not a number: falls through to bare-string handling
                    // below (`-` is a valid id-start character).
                    let (text, end) = tri!(lex_bare(input, i));
                    items.push(Item::Real(RealTok::Bare(text), Span { start: i, end }));
                    i = end;
                } else {
                    let (text, end) = tri!(lex_number(input, i));
                    items.push(Item::Real(RealTok::Number(text), Span { start: i, end }));
                    i = end;
                }
            }
            _ => {
                let ch = input[i..].chars().next().unwrap();
                if is_id_start(ch) {
                    let (text, end) = tri!(lex_bare(input, i));
                    items.push(Item::Real(RealTok::Bare(text), Span { start: i, end }));
                    i = end;
                } else {
                    return Err(err_at(input, i, alloc::format!("unexpected character {:?}", ch)));
                }
            }
        }
    }
    Ok(items)
}

fn lex_number(input: &str, start: usize) -> ParseResult<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut i = start;
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    if bytes.get(i) == Some(&b'0') {
        i += 1;
    } else if matches!(bytes.get(i), Some(b'1'..=b'9')) {
        i += 1;
        while matches!(bytes.get(i), Some(b'0'..=b'9')) {
            i += 1;
        }
    } else {
        return Err(err_at(input, start, "invalid number literal".into()));
    }
    if bytes.get(i) == Some(&b'.') {
        let mut j = i + 1;
        if !matches!(bytes.get(j), Some(b'0'..=b'9')) {
            return Err(err_at(input, i, "expected digit after decimal point".into()));
        }
        while matches!(bytes.get(j), Some(b'0'..=b'9')) {
            j += 1;
        }
        i = j;
    }
    if matches!(bytes.get(i), Some(b'e') | Some(b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+') | Some(b'-')) {
            j += 1;
        }
        if !matches!(bytes.get(j), Some(b'0'..=b'9')) {
            return Err(err_at(input, i, "expected digit in exponent".into()));
        }
        while matches!(bytes.get(j), Some(b'0'..=b'9')) {
            j += 1;
        }
        i = j;
    }
    Ok((&input[start..i], i))
}

fn lex_bare(input: &str, start: usize) -> ParseResult<(&str, usize)> {
    let mut end = start;
    let mut chars = input[start..].char_indices();
    let (_, first) = chars.next().unwrap();
    debug_assert!(is_id_start(first));
    end += first.len_utf8();
    for (off, c) in chars {
        if is_id_end(c) {
            end = start + off + c.len_utf8();
        } else {
            break;
        }
    }
    Ok((&input[start..end], end))
}

fn lex_string(input: &str, start: usize, quote: u8) -> ParseResult<(Cow<'_, str>, usize)> {
    let bytes = input.as_bytes();
    let mut i = start + 1;
    let content_start = i;
    let mut needs_owned = false;
    while i < bytes.len() && bytes[i] != quote {
        if bytes[i] == b'\\' {
            needs_owned = true;
            i += 2;
        } else {
            i += 1;
        }
    }
    if i >= bytes.len() {
        return Err(err_at(input, start, "unterminated string literal".into()));
    }
    let raw = &input[content_start..i];
    let end = i + 1;
    if !needs_owned {
        return Ok((Cow::Borrowed(raw), end));
    }
    Ok((Cow::Owned(tri!(unescape(raw, input, content_start))), end))
}

fn unescape(raw: &str, full_input: &str, raw_offset: usize) -> ParseResult<String> {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let esc = match bytes.get(i + 1) {
                Some(b) => *b,
                None => return Err(err_at(full_input, raw_offset + i, "dangling escape".into())),
            };
            match esc {
                b'\'' => {
                    out.push('\'');
                    i += 2;
                }
                b'"' => {
                    out.push('"');
                    i += 2;
                }
                b'\\' => {
                    out.push('\\');
                    i += 2;
                }
                b'/' => {
                    out.push('/');
                    i += 2;
                }
                b'b' => {
                    out.push('\u{8}');
                    i += 2;
                }
                b'f' => {
                    out.push('\u{c}');
                    i += 2;
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                }
                b'r' => {
                    out.push('\r');
                    i += 2;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                }
                b'u' => {
                    let hi = tri!(read_hex4(raw, i + 2, full_input, raw_offset + i));
                    i += 6;
                    let cp = if (0xD800..=0xDBFF).contains(&hi) {
                        if bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&b'u') {
                            let lo = tri!(read_hex4(raw, i + 2, full_input, raw_offset + i));
                            if (0xDC00..=0xDFFF).contains(&lo) {
                                i += 6;
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                return Err(err_at(
                                    full_input,
                                    raw_offset + i,
                                    "unpaired surrogate".into(),
                                ));
                            }
                        } else {
                            return Err(err_at(
                                full_input,
                                raw_offset + i,
                                "unpaired surrogate".into(),
                            ));
                        }
                    } else {
                        hi
                    };
                    match char::from_u32(cp) {
                        Some(ch) => out.push(ch),
                        None => {
                            return Err(err_at(
                                full_input,
                                raw_offset + i,
                                "invalid unicode escape".into(),
                            ))
                        }
                    }
                }
                other => {
                    return Err(err_at(
                        full_input,
                        raw_offset + i,
                        alloc::format!("invalid escape '\\{}'", other as char),
                    ))
                }
            }
        } else {
            let ch = raw[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    Ok(out)
}

fn read_hex4(raw: &str, at: usize, full_input: &str, err_pos: usize) -> ParseResult<u32> {
    let slice = match raw.as_bytes().get(at..at + 4) {
        Some(s) => s,
        None => return Err(err_at(full_input, err_pos, "truncated unicode escape".into())),
    };
    let s = match core::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return Err(err_at(full_input, err_pos, "invalid unicode escape".into())),
    };
    u32::from_str_radix(s, 16)
        .map_err(|_| err_at(full_input, err_pos, "invalid unicode escape".into()))
}

fn err_at(input: &str, byte_pos: usize, message: String) -> ParseError {
    let mut line = 1;
    let mut col = 1;
    for c in input[..byte_pos.min(input.len())].chars() {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    ParseError { message, line, column: col }
}

pub(crate) fn error_at(input: &str, byte_pos: usize, message: String) -> ParseError {
    err_at(input, byte_pos, message)
}

/// `id-start` from the CSON grammar: the union of ECMAScript-5 identifier
/// start characters and XML 1.0 `NameStartChar`, minus `:`, plus `$`/`-`.
/// Hard-coded per `CLAUDE.md` rather than derived from a Unicode crate,
/// since it must track a fixed spec version, not "current Unicode".
pub fn is_id_start(c: char) -> bool {
    matches!(c,
        '$' | '-' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{AA}' | '\u{B5}' | '\u{BA}'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}'
    )
}

pub fn is_id_end(c: char) -> bool {
    is_id_start(c)
        || matches!(c,
            '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'
        )
}
