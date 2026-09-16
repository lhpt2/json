//! Renders a [`Document`] back to CSON text according to its [`Style`].
//!
//! Per `CLAUDE.md`, layout is *never* read back from a node: every
//! structural newline, indent, comma and bracket is produced uniformly
//! from `Style`, regardless of how unevenly the source file was
//! formatted. Only comments and blank lines travel through `Node::prefix`.

use super::style::{KeyStyle, Quote, Separator, Style};
use super::{CsonStr, Document, Entry, Node, Value};
use alloc::string::String;

pub fn write_document(doc: &Document<'_>) -> String {
    let style = &doc.style;
    let mut out = String::new();
    let mut tail = String::new();
    if doc.bare_root {
        // With no `}` to bound it, a bare root's `trailing` slot and the
        // document's `suffix` denote the same position, so they're
        // written as one run -- a re-parse necessarily folds both into
        // `suffix`, and rendering them separately made that round trip
        // gain a blank line. The parser never fills this slot itself;
        // removing the last entry does, which is where Value::remove
        // migrates its comments to, and dropping it here lost them.
        if let Value::Object { entries, trailing } = &doc.root.value {
            write_object_entries(&mut out, entries, "", 0, style);
            tail.push_str(trailing);
        }
    } else {
        write_break(&mut out, &doc.root.prefix, 0, style);
        write_value(&mut out, &doc.root, 0, style);
    }
    tail.push_str(&doc.suffix);
    write_tail(&mut out, &tail, style);
    // Nothing precedes the very first line, so the leading "\n" that
    // write_break always emits (to move past whatever came before) is
    // spurious here; likewise a comment-only suffix would otherwise leave
    // a trailing blank line.
    while out.starts_with('\n') {
        out.remove(0);
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

/// Writes the trivia following the last token, up to end of file.
///
/// Unlike a node's prefix, the size of the blank run *before* it isn't
/// meaningful: the writer has already ended the previous line itself,
/// and a re-parse folds whatever was emitted back into this same slot.
/// Canonicalizing that leading run to one blank line is what keeps the
/// round trip a fixpoint -- rendering it verbatim makes the blank line
/// grow by one the first time a document is written and re-read. Blank
/// lines *between* comment paragraphs are inside `body` and survive.
fn write_tail(out: &mut String, tail: &str, style: &Style) {
    let body = tail.trim_start_matches(['\n', '\r', ' ', '\t']);
    if body.trim().is_empty() {
        return;
    }
    let mut normalized = String::from("\n");
    normalized.push_str(body);
    write_break(out, &normalized, 0, style);
}

/// Writes `prefix` (blank lines + comments), then unconditionally moves to
/// a fresh, indented line. Used wherever a node always starts its own line
/// (object keys, array items, closing brackets).
fn write_break(out: &mut String, prefix: &str, depth: usize, style: &Style) {
    write_prefix_lines(out, prefix, depth, style);
    out.push('\n');
    out.push_str(&style.indent().render(depth));
}

/// Writes `prefix`, then either a single separating space (no comment --
/// stay on the same line) or a fresh indented line (a comment forces a
/// line break, since a comment always runs to end of line). Used only
/// between a `:`/`=` separator and its value.
fn write_soft(out: &mut String, prefix: &str, depth: usize, style: &Style) {
    if prefix.is_empty() {
        out.push(' ');
    } else {
        write_prefix_lines(out, prefix, depth, style);
        out.push('\n');
        out.push_str(&style.indent().render(depth));
    }
}

fn write_prefix_lines(out: &mut String, prefix: &str, depth: usize, style: &Style) {
    for line in prefix.lines() {
        let t = line.trim();
        out.push('\n');
        if !t.is_empty() {
            out.push_str(&style.indent().render(depth));
            out.push_str(t);
        }
    }
}

fn write_value(out: &mut String, node: &Node<'_>, depth: usize, style: &Style) {
    match &node.value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(n.as_str()),
        Value::Str(s) => write_string_value(out, s, depth, style),
        Value::Array { items, trailing } => write_array(out, items, trailing, depth, style),
        Value::Object { entries, trailing } => write_object(out, entries, trailing, depth, style),
    }
}

fn write_object(out: &mut String, entries: &[Entry<'_>], trailing: &str, depth: usize, style: &Style) {
    out.push('{');
    if entries.is_empty() && trailing.trim().is_empty() {
        out.push('}');
        return;
    }
    write_object_entries(out, entries, trailing, depth, style);
    out.push('}');
}

fn write_object_entries(out: &mut String, entries: &[Entry<'_>], trailing: &str, depth: usize, style: &Style) {
    let inner = depth + 1;
    for (i, entry) in entries.iter().enumerate() {
        write_break(out, &entry.key.prefix, inner, style);
        write_key(out, &entry.key, style);
        write_separator(out, style);
        write_soft(out, &entry.value.prefix, inner, style);
        write_value(out, &entry.value, inner, style);
        let is_last = i + 1 == entries.len();
        if style.commas() && (!is_last || style.trailing_comma()) {
            out.push(',');
        }
    }
    write_break(out, trailing, depth, style);
}

fn write_array(out: &mut String, items: &[Node<'_>], trailing: &str, depth: usize, style: &Style) {
    out.push('[');
    if items.is_empty() && trailing.trim().is_empty() {
        out.push(']');
        return;
    }
    let inner = depth + 1;
    for (i, item) in items.iter().enumerate() {
        write_break(out, &item.prefix, inner, style);
        write_value(out, item, inner, style);
        let is_last = i + 1 == items.len();
        if style.commas() && (!is_last || style.trailing_comma()) {
            out.push(',');
        }
    }
    write_break(out, trailing, depth, style);
    out.push(']');
}

/// Writes just the separator character(s), with no trailing space --
/// the space (or comment-forced line break) between the separator and
/// the value is [`write_soft`]'s job, not this one's.
fn write_separator(out: &mut String, style: &Style) {
    match style.separator() {
        Separator::Colon => out.push(':'),
        Separator::Equals => out.push_str(" ="),
    }
}

fn write_key(out: &mut String, key: &Node<'_>, style: &Style) {
    let text = match &key.value {
        Value::Str(s) => s.as_str(),
        _ => unreachable!("object keys are always strings"),
    };
    match style.key_style() {
        KeyStyle::Bare if is_valid_bare(text) => out.push_str(text),
        KeyStyle::Bare => write_quoted(out, text, style.value_quote()),
        KeyStyle::Double => write_quoted(out, text, Quote::Double),
        KeyStyle::Single => write_quoted(out, text, Quote::Single),
    }
}

fn write_string_value(out: &mut String, s: &CsonStr<'_>, depth: usize, style: &Style) {
    let text = s.as_str();
    if style.verbatim_strings() && text.contains('\n') {
        write_verbatim(out, text, depth, style);
    } else {
        write_quoted(out, text, style.value_quote());
    }
}

fn write_verbatim(out: &mut String, text: &str, depth: usize, style: &Style) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&style.indent().render(depth));
        }
        out.push('|');
        out.push_str(line);
    }
}

fn write_quoted(out: &mut String, text: &str, quote: Quote) {
    let q = match quote {
        Quote::Double => '"',
        Quote::Single => '\'',
    };
    out.push(q);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                out.push_str(&alloc::format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(q);
}

fn is_valid_bare(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if super::lexer::is_id_start(c) => chars.all(super::lexer::is_id_end),
        _ => false,
    }
}
