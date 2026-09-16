//! The comment line-anchor pass described in `CLAUDE.md`.
//!
//! A comment that is not the first thing on its source line is moved into
//! the trivia slot belonging to the first token of that line; a line whose
//! first token is a closing bracket inherits the anchor of the line the
//! matching opening bracket was on. This runs once, on the flat lexer
//! output, before the recursive-descent parser ever sees a token -- by the
//! time the parser runs, every comment left "in place" is already on its
//! own line.

use super::lexer::{Item, RealTok};
use alloc::string::String;
use alloc::vec::Vec;

pub struct TriviaInfo {
    /// `prefixes[i]` is the trivia to attach before real token `i`;
    /// `prefixes[n]` (`n` = number of real tokens) is `Document::suffix`.
    pub prefixes: Vec<String>,
    /// `blank_line[i]`: does the gap before real token `i` (or, at index
    /// `n`, the trailing gap before EOF) contain an empty line -- two
    /// newlines with nothing but whitespace between them? Used to resolve
    /// the verbatim-fragment merge ambiguity.
    pub blank_line: Vec<bool>,
    /// `had_newline[i]`: did at least one line break occur in that same
    /// gap? Used to check the "comma or newline" value-separator rule.
    pub had_newline: Vec<bool>,
}

pub fn resolve<'a>(items: &[Item<'a>]) -> TriviaInfo {
    let real_toks: Vec<&RealTok<'a>> = items
        .iter()
        .filter_map(|it| match it {
            Item::Real(t, _) => Some(t),
            _ => None,
        })
        .collect();
    let n = real_toks.len();

    let mut prefixes: Vec<String> = alloc::vec![String::new(); n + 1];
    let mut blank_line: Vec<bool> = alloc::vec![false; n + 1];
    let mut had_newline: Vec<bool> = alloc::vec![false; n + 1];

    let mut at_line_start = true;
    let mut line_anchor: usize = 0;
    let mut stack: Vec<usize> = Vec::new();

    let mut gap_index = 0usize;
    let mut last_was_newline = false;

    for item in items {
        match item {
            Item::Newline => {
                if last_was_newline {
                    blank_line[gap_index] = true;
                }
                had_newline[gap_index] = true;
                last_was_newline = true;
                at_line_start = true;
            }
            Item::Comment(text) => {
                last_was_newline = false;
                if at_line_start {
                    line_anchor = gap_index;
                    at_line_start = false;
                }
                let slot = &mut prefixes[line_anchor];
                slot.push('\n');
                slot.push_str(text);
            }
            Item::Real(tok, _) => {
                last_was_newline = false;
                let is_close = matches!(tok, RealTok::RBrace | RealTok::RBracket);
                let is_open = matches!(tok, RealTok::LBrace | RealTok::LBracket);
                if at_line_start {
                    line_anchor = if is_close {
                        *stack.last().unwrap_or(&0)
                    } else {
                        gap_index
                    };
                    at_line_start = false;
                }
                if is_open {
                    stack.push(line_anchor);
                } else if is_close {
                    stack.pop();
                }
                gap_index += 1;
            }
        }
    }

    TriviaInfo { prefixes, blank_line, had_newline }
}

/// Whether `text` is valid `Node` prefix content: only whitespace and
/// `#`-comments running to end of line.
pub fn is_valid_prefix(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' {
            for c2 in chars.by_ref() {
                if c2 == '\n' || c2 == '\r' {
                    break;
                }
            }
        } else if !c.is_whitespace() {
            return false;
        }
    }
    true
}
