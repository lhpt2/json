# `src/document/` — the comment-preserving parser (Schicht 1)

This is the architecture `CLAUDE.md` describes: a CSON parser that keeps
comments and blank lines around a parse/write cycle, independent of
`serde`. It is a from-scratch parser (not the legacy `de.rs`/`read.rs`
pipeline), because comments have no home in a `serde::Deserializer` — see
"Why this is a separate parser" below. For the older, `serde`-integrated
pipeline this exists alongside, see
[`legacy-value-serde-path.md`](./legacy-value-serde-path.md).

## Status

Implemented: **Schicht 1** (lexer → comment-reattachment pass → parser)
plus the style-detection/writer step that has to exist for Schicht 1's own
round-trip invariants to be testable at all.

Not implemented yet, in the order `CLAUDE.md` prescribes building them:
- Schicht 2 — `impl serde::Deserializer for &Node` (typed reads without
  losing the ability to also edit).
- Schicht 3 — `impl serde::Serializer with Ok = Node`, and `merge_from`
  (typed edit-in-place: parse → deserialize → mutate the struct →
  `merge_from` → write, keeping every untouched comment).

Because of that, the public API today is intentionally small: parse a
document, walk it read-only, edit `Node::prefix` directly (validated), and
write it back out. There is no typed access yet.

## Module layout

```
src/document/
  mod.rs     data model (Document, Node, Value, Entry, Number, CsonStr),
             ParseError, the public parse() entry point, Display/
             to_cson_string()
  lexer.rs   text -> flat token/trivia stream
  trivia.rs  the comment line-anchor reattachment pass
  parser.rs  recursive descent: token/trivia stream -> Document tree,
             latching Style along the way
  style.rs   the Style struct and its first-match setters/getters
  writer.rs  Document + Style -> text
  tests.rs   round-trip invariant tests (#[cfg(test)])
```

## Data model

```rust
pub struct Document<'a> {
    root: Node<'a>,
    suffix: Cow<'a, str>,   // trivia after the last token, up to EOF
    bare_root: bool,        // source omitted the outer `{` `}`?
    style: Style,
}

pub struct Node<'a> {
    prefix: Cow<'a, str>,   // whitespace + '#' comments before this node
    value: Value<'a>,
}

pub enum Value<'a> {
    Null, Bool(bool), Number(Number<'a>), Str(CsonStr<'a>),
    Array  { items: Vec<Node<'a>>,  trailing: Cow<'a, str> },
    Object { entries: Vec<Entry<'a>>, trailing: Cow<'a, str> },
}

pub struct Entry<'a> { key: Node<'a>, value: Node<'a> }  // key has its own
                                                          // prefix slot too

pub struct Number<'a> { raw: Cow<'a, str> } // literal text, not f64/i64 --
                                             // see Number::numeric_eq
```

`CsonStr` holds only the decoded string value — no per-node quote style.
That's deliberate: per `CLAUDE.md`, layout is a document-wide `Style`
choice, applied uniformly by the writer, never stored per node.

## Pipeline

1. **`lexer::lex`** turns the source into a flat, ordered `Vec<Item>` of
   real tokens (braces, brackets, `:`/`=`, `,`, strings, numbers, bare
   words, verbatim fragments) interleaved with two trivia kinds,
   `Comment` and `Newline`. Plain whitespace is consumed and never
   represented at all — the writer regenerates every bit of indentation
   from `Style`, so keeping the original bytes would be pointless. CR, LF
   and CRLF all collapse to one `Newline` item. A verbatim fragment
   (`|...` to end of line) is lexed as a single token specifically so `#`
   inside it is never mistaken for a comment start.

2. **`trivia::resolve`** runs the line-anchor algorithm from `CLAUDE.md`
   over that stream once, before the parser sees anything: a comment
   that isn't the first thing on its line gets moved into the trivia slot
   of whatever token *is* first on that line; a line that starts with a
   closing bracket inherits the anchor of the line its matching opener
   was on. The output is one `String` per real token (plus one for the
   file's trailing suffix) — by construction, whatever the parser attaches
   as a node's `prefix` is already exactly the trivia that belongs there.

3. **`parser::parse_document`** is recursive descent over the array of
   real tokens, looking up each token's pre-resolved prefix by index
   (byte offsets no longer matter once trivia is resolved). Along the way
   it:
   - handles the bare-root case (`ws object-items`, object only, never
     array) and empty input (treated as an empty bare object, with
     any comments landing in `Document::suffix`),
   - accepts both `:`/`=` and both quoting styles for values, `'`/`"`
     for keys, plus bare keys,
   - accepts a bare newline as a value-separator wherever a comma would
     also be valid, and trailing commas before `]`/`}`,
   - merges consecutive verbatim fragments into one string (joined with
     `\n`), *unless* they're separated by a blank line or an explicit
     comma — this is the "documented ambiguity" `CLAUDE.md` calls out,
     resolved in the spec's favor (merge by default),
   - rejects a bare word as a *value* (only `true`/`false`/`null` are
     valid bare-word values; anything else bare is a syntax error there,
     since `bare-string` is a `name` production only),
   - latches `Style` fields the first time each corresponding construct
     is seen (see below).

4. **`writer::write_document`** renders the tree back to text using only
   `Style` for every structural decision (separator, quoting, commas,
   indentation, verbatim-or-not) — never anything read back off a node.
   `Node::prefix` content is re-indented per line (`.lines()`, trim, then
   re-indent) rather than reused verbatim, so `Style`'s indentation always
   wins even where the source was inconsistent.

## `Style`

First-match, `Option`-per-field, exactly as `CLAUDE.md` specifies:
`separator`, `value_quote`, `key_style`, `commas`, `trailing_comma` are
all set from the first time the parser observes the corresponding
construct and never touched again. `indent` and `verbatim_strings` are
*not* first-match-settable at all (`CLAUDE.md` calls these two
unreliable-to-detect) — they only ever come from `Style::default()`
(2-space indent, no verbatim) unless a caller sets them explicitly via
`Document::set_style`.

A single-fragment verbatim string (`a = |hello`, no continuation, no
literal `\n` in its value) deliberately does **not** latch
`verbatim_strings = true`: writing it back as a plain quoted string is
equally valid, and if it did latch, the invariant-4 style-fixpoint test
below would fail for that input (see the test file's comment on this — it
documents a case that used to fail before this rule was added).

## The four round-trip invariants

`tests.rs`'s `assert_invariants` checks, for each fixture, exactly the
four invariants `CLAUDE.md` specifies:

1. **Fixpoint from the first run on**: `write(parse(s))` need not equal
   `s`, but `write(parse(write(parse(s))))` must equal `write(parse(s))`.
2. **Comments preserved as a set**: the first run may relocate a comment
   (per the line-anchor rule), but no comment may be gained or lost.
3. **Values unchanged**: parsing the output yields the same value tree
   shape/content as parsing the original.
4. **Style fixpoint**: re-parsing the output must detect the same style
   the writer used to produce it — compared via `Style`'s *effective*
   getters (`separator()`, `value_quote()`, …, which apply the default
   when a field is `None`), not raw `Option` equality. A dimension the
   original source never exercised legitimately becomes an observed
   `Some(default)` the moment the writer has to render *something* for
   it; that's the first observation of that dimension, not drift. Raw
   equality was tried first and produces false failures for exactly this
   reason — see the git history on this file if you want the concrete
   failing case.

36 tests currently exercise this against the fixture categories
`CLAUDE.md` lists: comment placement (before/after values, at EOF, inside
empty containers, blank lines between comment paragraphs), the
line-anchor rule specifically (trailing comments, closer-line comments,
multiple closers on one line), all the syntax variants (mixed `'`/`"`,
mixed `:`/`=`, bare keys, bare root, comma omission across a newline *and*
across a comment, trailing commas), verbatim strings (single fragment,
merged fragments, comment between fragments, comma/blank-line breaking
the merge, `#` inside a fragment not being a comment), and the
miscellaneous ones (CRLF/LF mixed, BOM, numbers beyond 2^53, `A`
== `"A"`).

## Code reuse vs. the legacy path

`CLAUDE.md` is explicit that string-unescaping and number-parsing should
be taken from serde_json's existing (private) implementation rather than
re-derived independently. Where that's structurally possible, it's done:

- **Double-quoted strings** (`"..."`) are decoded via
  `crate::read::StrRead::parse_str` — the exact escape/surrogate-pair/
  WTF-8-handling code the legacy `Deserializer` runs, not a second
  implementation. This is the common case, and it's byte-for-byte
  JSON-compatible.
- **`\'` as a valid escape** was added directly to the shared
  `read::parse_escape`, so both paths gained it in the same place, by
  construction.

Two places stayed as explicit, documented forks instead of shared code,
because the underlying access pattern genuinely doesn't match:

- **Apostrophe-quoted strings** (`'...'`): `crate::read::Read` is a
  *sealed* trait, implemented by four types, that hardcodes `"` as the
  string terminator in a SIMD-ish hot path (`SliceRead::skip_to_escape`
  scans word-sized chunks at a time looking for `"`/`\`/control bytes
  specifically). Threading a runtime quote parameter through all four
  impls is a real, riskier change to a heavily-used hot path — out of
  scope for the session that added apostrophe support, so
  `document::lexer::lex_single_quoted` mirrors `parse_escape`/
  `parse_unicode_escape`'s table and surrogate-pairing logic by hand,
  with a comment pointing back at those functions to keep in sync.
- **Number-literal scanning**: the one existing serde_json scanner that
  preserves raw text instead of computing a value
  (`Deserializer::scan_integer`/`scan_decimal`/`scan_exponent`, gated
  behind the `arbitrary_precision` feature) is written against the
  streaming `Read` trait one byte at a time, because it also has to work
  over `io::Read` sources. `document::parse` always has the whole input
  materialized as one `&str` and scans its bytes directly. Forcing one
  function to serve both access patterns would cost more in generic
  abstraction than it would save in dedup, so `document::lexer::lex_number`
  is a separate function with the same grammar (int → optional frac →
  optional exp), documented as mirroring that scanner. Comparing the two
  side by side while writing this surfaced a real bug that's now fixed:
  `01`-style leading-zero literals weren't being rejected.

## Known, accepted precision losses

Two cases where the round trip visibly *moves* something on the first
write, by design, rather than trying to preserve exact source position
(both still satisfy the four invariants — the moved position is just
where it stabilizes from the second run on):

- `b: 2, c: 3  # zu c` — the comment lands on `b`'s line, not `c`'s
  (`CLAUDE.md`'s own example of an accepted loss).
- A comment between two verbatim fragments (`a = |hello\n  # note\n  |world`)
  has no per-fragment prefix slot to live in, so it's folded into the
  merged string node's own prefix — ends up before the whole `a = |hello`
  line instead of between the two fragments.
