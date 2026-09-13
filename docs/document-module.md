# `src/document/` — the comment-preserving parser (Schicht 1)

This is the architecture `CLAUDE.md` describes: a CSON parser that keeps
comments and blank lines around a parse/write cycle, independent of
`serde`. It is a from-scratch parser (not the legacy `de.rs`/`read.rs`
pipeline), because comments have no home in a `serde::Deserializer` — see
"Why this is a separate parser" below. For the older, `serde`-integrated
pipeline this exists alongside, see
[`legacy-value-serde-path.md`](./legacy-value-serde-path.md).

## Status

Implemented: **Schicht 1** (lexer → comment-reattachment pass → parser),
the style-detection/writer step Schicht 1's own round-trip invariants need
to be testable at all, **Schicht 2** (`impl serde::Deserializer for
&Node`, in `de.rs`) and **Schicht 3** (`impl serde::Serializer with Ok =
Node`, in `ser.rs`).

Not implemented yet: **`merge_from`** — diffing a freshly `Serialize`d
tree against an existing, comment-carrying one, so a typed edit only
touches the fields that actually changed and every other node (and its
comments) is left alone. That's a genuinely separate step from "having
both a Deserializer and a Serializer" — see "What Schicht 2/3 do and
don't give you" below for exactly why, since it's a common
misconception that the two together already add up to it.

## Module layout

```
src/document/
  mod.rs        data model (Document, Node, Value, Entry, Number,
                 CsonStr), ParseError, the public parse() entry point,
                 Display/to_cson_string(), Document::deserialize/
                 from_serialize
  lexer.rs      text -> flat token/trivia stream
  trivia.rs     the comment line-anchor reattachment pass
  parser.rs     recursive descent: token/trivia stream -> Document tree,
                latching Style along the way
  style.rs      the Style struct and its first-match setters/getters
  writer.rs     Document + Style -> text
  de.rs         Schicht 2: impl serde::Deserializer for &Node
  ser.rs        Schicht 3: impl serde::Serializer with Ok = Node
  tests.rs      Schicht 1 round-trip invariant tests (#[cfg(test)])
  serde_tests.rs Schicht 2/3 tests (#[cfg(test)])
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

## Schicht 2 and 3

`de.rs` and `ser.rs` mirror `src/value/de.rs`'s `impl<'de> Deserializer<'de>
for &'de Value` and `src/value/ser.rs`'s `Serializer { type Ok = Value }`
closely on purpose — same method bodies, same helper-struct shapes
(`SeqRefDeserializer`/`MapRefDeserializer`/`EnumRefDeserializer`/
`MapKeyDeserializer`/`MapKeySerializer`), adapted to `Node`/`Value`/`Entry`
instead of `serde_json::Value`/`Map`. `ParseError` grew `impl
serde::de::Error` and `impl serde::ser::Error` (both just `fn custom`) to
serve as both layers' `Error` type. Numbers reuse `itoa`/`zmij` (already
crate dependencies, used the same way in `ser.rs`) to format `Number::raw`
from typed integers/floats, rather than a third number-formatting
implementation.

Two entry points, added on `Document`:

```rust
// Schicht 2: read a parsed (and possibly hand-edited) document into a
// typed value, without ever constructing a plain serde_json::Value.
let doc = document::parse(&text)?;
let cfg: Config = doc.deserialize()?;

// Schicht 3: the reverse -- build a fresh Document straight from a
// typed value (default Style, no comments -- there's no source to take
// either from).
let doc = document::Document::from_serialize(&cfg)?;
let text = doc.to_cson_string();
```

`document::to_node::<T>(&value)` is also public, for producing a bare
`Node<'static>` (e.g. to build one field's replacement value by hand
rather than a whole document).

### What Schicht 2/3 do and don't give you

They give you: reading a `Document` straight into `T` (Schicht 2), and
building a fresh `Document` straight from a `T` (Schicht 3) — both
without detouring through `serde_json::Value`. `serde_tests.rs` exercises
structs, nested structs, `Vec`, `Option`, externally-tagged enums (unit,
newtype, and struct variants), `BTreeMap`, and error propagation for a
shape mismatch, plus the specific round trip `struct -> Document ->
text -> Document -> struct` staying equal, and re-writing that reparsed
document being a fixpoint (same as the Schicht 1 invariant).

They do **not** give you `Document::merge_from` (still not implemented).
Concretely: if you `doc.deserialize::<Config>()`, mutate one field of
`cfg`, and want to write the change back into `doc` — keeping every
comment and every *other* field's exact source formatting — Schicht 2/3
alone don't get you there. `Document::from_serialize(&cfg)` builds a
**brand new** tree with **no** comments and a **freshly defaulted**
`Style`; substituting it for `doc.root` would silently discard every
comment `doc` had. The only thing that could safely stand in for
`merge_from` today is manually finding the one `Node` you changed (via
`doc.root_mut()` and matching down through `Value::Object`/`Array` by
hand) and overwriting just that node's `value` — which is exactly the
tedious, error-prone, whole-document-structural-knowledge-required process
`merge_from` exists to automate (per `CLAUDE.md`: object/array diffing by
key/position, `value`-only overwrites so a changed node's `prefix` survives,
a `value_eq` that ignores trivia, numeric rather than textual equality for
numbers). None of that diffing exists yet — building it is the next step,
not a byproduct of already having a Deserializer and a Serializer.

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
