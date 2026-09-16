# cson_edit

A CSON (Cursive Script Object Notation) parser and editor that preserves
comments and formatting across a parse/edit/write cycle — analogous to
[`toml_edit`](https://docs.rs/toml_edit) for TOML.

This crate started life as an in-tree module of a CSON-flavored fork of
`serde_json`, developed on a sibling branch of this repository, and was
later extracted onto this branch as its own independent crate. It has
no dependency on that fork, or on any `serde_json` internals — the only
dependency is `serde` itself, for the typed read/write layer described
below.

## Why a separate crate/parser at all

`serde::Deserializer` is a pull-based interface: it hands a `Visitor` a
bool, a string, "here's a map, ask for keys/values" — there is no
callback for "there was a `#` comment here." Once a document has been
consumed by *any* `Deserializer`, its comments are gone, whether or not
that Deserializer happens to target `serde_json::Value` or something
else. So a comment-preserving editor can't be a thin layer on top of an
existing `Deserializer`-based parser; it needs its own tree type with a
dedicated place to keep trivia. That's what this crate is.

## Status

Implemented: **Schicht 1** (lexer → comment-reattachment pass → parser →
style-detection → writer) and, on top of it, **Schicht 2**
(`impl serde::Deserializer for &Node`, in `de.rs`) and **Schicht 3**
(`impl serde::Serializer with Ok = Node`, in `ser.rs`).

Not implemented yet: **`merge_from`** — diffing a freshly `Serialize`d
tree against an existing, comment-carrying one, so a typed edit only
touches the fields that actually changed and every other node (and its
comments) is left alone. That's a genuinely separate step from "having
both a Deserializer and a Serializer" — see "What Schicht 2/3 do and
don't give you" below for exactly why, since it's a common
misconception that the two together already add up to it.

## Quick start

If you just want to read/write typed values and don't need to keep
comments around, use the conventional top-level functions (matching
`serde_json`/`serde_yaml`/`toml`'s own naming):

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    port: u16,
}

let cfg = Config { name: "svc".to_string(), port: 8080 };

let text = cson_edit::to_string(&cfg)?;
let cfg2: Config = cson_edit::from_str(&text)?;
assert_eq!(cfg, cfg2);
# Ok::<(), cson_edit::ParseError>(())
```

`from_slice`/`to_vec` (bytes) and, behind the default `std` feature,
`from_reader`/`to_writer` (`io::Read`/`Write`) round out that API the
same way they do for other serde data formats. All of these are
convenience wrappers: they parse into a `Document` and immediately
discard it, so `T` must be fully owned (`DeserializeOwned`) -- there's no
intermediate value that can hand out zero-copy `&str` borrows once the
function returns.

When you *do* want the comments to survive an edit, work with `Document`
directly instead:

```rust
use cson_edit::{parse, Document};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    name: String,
    port: u16,
}

// Read-only structural access, comments intact:
let doc = parse("name = \"svc\"  # prod\nport = 8080\n")?;
println!("{}", doc);

// Typed read (Schicht 2) -- the Document you read from still has its
// comments; the struct itself, like any plain Rust struct, does not:
let cfg: Config = doc.deserialize()?;

// Typed write (Schicht 3) -- builds a *fresh* tree, no comments, default
// Style, since there's no source layout to take one from:
let fresh = Document::from_serialize(&cfg)?;
println!("{}", fresh.to_cson_string());
# Ok::<(), cson_edit::ParseError>(())
```

## Examples

`examples/` has five runnable, commented programs, each `cargo run
--example NAME` away, and `docs/USAGE.md` walks through them in the
order you're likely to need them:

* `01_typed_read_write.rs` — the five/seven top-level functions
  (`from_str`/`to_string`/…), for when comments don't matter.
* `02_parse_and_inspect.rs` — parsing into a `Document` and walking its
  tree by hand, read-only.
* `03_edit_preserving_comments.rs` — the crate's actual reason to
  exist: change one value, write the document back out, keep every
  comment. Uses the `Node`/`Entry` mutation API described below.
* `04_custom_style.rs` — reading the `Style` `parse` detected and
  overriding it before writing.
* `05_file_roundtrip.rs` — the same edit-preserving-comments pattern
  as `03`, but against an actual file on disk
  (`examples/sample_with_comments.cson`) instead of an in-memory
  string: `fs::read_to_string` in, `fs::write` out.

## Module layout

```
src/
  lib.rs         data model (Document, Node, Value, Entry, Number,
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

```rust,ignore
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
That's deliberate: layout is a document-wide `Style` choice, applied
uniformly by the writer, never stored per node.

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

2. **`trivia::resolve`** runs a line-anchor algorithm over that stream
   once, before the parser sees anything: a comment that isn't the first
   thing on its line gets moved into the trivia slot of whatever token
   *is* first on that line; a line that starts with a closing bracket
   inherits the anchor of the line its matching opener was on. The
   output is one `String` per real token (plus one for the file's
   trailing suffix) — by construction, whatever the parser attaches as a
   node's `prefix` is already exactly the trivia that belongs there.

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
     comma — this is CSON's documented merge-vs-split ambiguity for
     adjacent verbatim fragments, resolved in the spec's favor (merge by
     default),
   - rejects a bare word as a *value* (only `true`/`false`/`null` are
     valid bare-word values; anything else bare is a syntax error there,
     since `bare-string` is a key-only grammar production),
   - latches `Style` fields the first time each corresponding construct
     is seen (see below).

4. **`writer::write_document`** renders the tree back to text using only
   `Style` for every structural decision (separator, quoting, commas,
   indentation, verbatim-or-not) — never anything read back off a node.
   `Node::prefix` content is re-indented per line (`.lines()`, trim, then
   re-indent) rather than reused verbatim, so `Style`'s indentation always
   wins even where the source was inconsistent.

## `Style`

First-match, `Option`-per-field: `separator`, `value_quote`, `key_style`,
`commas`, `trailing_comma` are all set from the first time the parser
observes the corresponding construct and never touched again. `indent`
and `verbatim_strings` are *not* first-match-settable at all (both are
unreliable to detect reliably from a single occurrence) — they only ever
come from `Style::default()` (2-space indent, no verbatim) unless a
caller sets them explicitly via `Document::set_style`.

A single-fragment verbatim string (`a = |hello`, no continuation, no
literal `\n` in its value) deliberately does **not** latch
`verbatim_strings = true`: writing it back as a plain quoted string is
equally valid, and if it did latch, the invariant-4 style-fixpoint test
below would fail for that input (see the test file's comment on this — it
documents a case that used to fail before this rule was added).

## The four round-trip invariants

`tests.rs`'s `assert_invariants` checks, for each fixture:

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
   it; that's the first observation of that dimension, not drift.

44 tests currently exercise this and the Schicht 2/3 layer: comment
placement (before/after values, at EOF, inside empty containers, blank
lines between comment paragraphs), the line-anchor rule specifically
(trailing comments, closer-line comments, multiple closers on one line),
all the syntax variants (mixed `'`/`"`, mixed `:`/`=`, bare keys, bare
root, comma omission across a newline *and* across a comment, trailing
commas), verbatim strings (single fragment, merged fragments, comment
between fragments, comma/blank-line breaking the merge, `#` inside a
fragment not being a comment), the miscellaneous ones (CRLF/LF mixed,
BOM, numbers beyond 2^53, `A` == `"A"`), and typed access (structs,
nested structs, `Vec`, `Option`, externally-tagged enums, `BTreeMap`, the
full `struct -> Document -> text -> Document -> struct` round trip).

## Independence: no `serde_json` dependency

This crate's string- and number-scanning are entirely self-contained
(`lexer.rs`'s `lex_string`/`unescape` and `lex_number`), not shared with
the sibling `serde_json` fork's internals. That's a deliberate trade-off
from the extraction: while this lived as a module inside that fork, its
double-quoted-string decoding briefly reused that crate's private
`Read`/`StrRead` machinery (its exact escape/surrogate-pair handling) —
but that access disappears the moment this becomes an independently
depended-on crate, the same way `toml_edit` doesn't reach into
`serde_json` for anything. Both quote styles (`"..."` and `'...'`) now
share one hand-written scanner instead. The apostrophe-only escape
decoder and the raw-text-preserving number scanner were *already*
self-contained forks even before the extraction (for reasons specific to
each, worth reading in `lexer.rs`'s doc comments if you're touching
either) — the double-quoted path is the one thing that changed shape here.

## Schicht 2 and 3

`de.rs` and `ser.rs` mirror the shape of `serde_json::value`'s own
`impl<'de> Deserializer<'de> for &'de Value` / `Serializer { type Ok =
Value }` (same method bodies, same helper-struct shapes —
`SeqRefDeserializer`/`MapRefDeserializer`/`EnumRefDeserializer`/
`MapKeyDeserializer`/`MapKeySerializer`), adapted to this crate's
`Node`/`Value`/`Entry` types. `ParseError` implements both
`serde::de::Error` and `serde::ser::Error` (both just `fn custom`) to
serve as both layers' `Error` type. Numbers are formatted via
`ToString`/parsed via `FromStr` rather than a dedicated
performance-oriented formatter (no `itoa`/`zmij` dependency, unlike the
sibling `serde_json` fork) — Rust's own float `Display` is already
shortest-round-trip, so nothing is lost, just a bit of raw throughput on
very large documents.

Two entry points, added on `Document`:

```rust,ignore
// Schicht 2: read a parsed (and possibly hand-edited) document into a
// typed value, without going through any intermediate loosely-typed
// value.
let doc = cson_edit::parse(&text)?;
let cfg: Config = doc.deserialize()?;

// Schicht 3: the reverse -- build a fresh Document straight from a
// typed value (default Style, no comments -- there's no source to take
// either from).
let doc = cson_edit::Document::from_serialize(&cfg)?;
let text = doc.to_cson_string();
```

`cson_edit::to_node::<T>(&value)` is also public, for producing a bare
`Node<'static>` (e.g. to build one field's replacement value by hand
rather than a whole document).

On top of those two, the crate root has the conventional free functions
a serde data-format crate is expected to expose (see "Quick start"
above): `from_str`/`to_string` (`alloc` only), `from_slice`/`to_vec`
(bytes), and, behind the default `std` feature, `from_reader`/
`to_writer` (`io::Read`/`Write`). All six are thin wrappers around
`parse`/`Document::deserialize`/`Document::from_serialize`/
`to_cson_string` for callers who don't need the `Document` (or its
comments) to survive past the call, and therefore require `T:
DeserializeOwned` on the read side (no zero-copy `&str` fields) -- the
intermediate `Document` is a local temporary in these wrappers, unlike
when you call `parse` yourself and keep it alive.

### What Schicht 2/3 do and don't give you

They give you: reading a `Document` straight into `T` (Schicht 2), and
building a fresh `Document` straight from a `T` (Schicht 3) — both
without detouring through any intermediate loosely-typed value. They do
**not** give you `Document::merge_from` (still not implemented).
Concretely: if you `doc.deserialize::<Config>()`, mutate one field of
`cfg`, and want to write the change back into `doc` — keeping every
comment and every *other* field's exact source formatting — Schicht 2/3
alone don't get you there. `Document::from_serialize(&cfg)` builds a
**brand new** tree with **no** comments and a **freshly defaulted**
`Style`; substituting it for `doc.root` would silently discard every
comment `doc` had. The only thing that could safely stand in for
`merge_from` today is manually finding the one `Node` you changed (via
`doc.root_mut()` and matching down through `Value::Object`/`Array` by
hand) and overwriting just that node's `value` — via `Node::value_mut`/
`set_value` and `Entry::key_mut`/`value_mut`, building the replacement
`Value` itself with `Value::from_serialize` (which runs it through this
same Schicht 3 machinery) rather than constructing a `Value` variant by
hand. See `examples/03_edit_preserving_comments.rs` and
`docs/USAGE.md` for the pattern end to end. It's exactly the tedious,
error-prone, whole-document-structural-knowledge-required
process `merge_from` exists to automate (object/array diffing by
key/position, `value`-only overwrites so a changed node's `prefix`
survives, an equality check that ignores trivia, numeric rather than
textual equality for numbers). None of that diffing exists yet —
building it is the next step, not a byproduct of already having a
Deserializer and a Serializer.

## Known, accepted precision losses

Two cases where the round trip visibly *moves* something on the first
write, by design, rather than trying to preserve exact source position
(both still satisfy the four invariants — the moved position is just
where it stabilizes from the second run on):

- `b: 2, c: 3  # zu c` — the comment lands on `b`'s line, not `c`'s.
- A comment between two verbatim fragments (`a = |hello\n  # note\n  |world`)
  has no per-fragment prefix slot to live in, so it's folded into the
  merged string node's own prefix — ends up before the whole `a = |hello`
  line instead of between the two fragments.

## License

MIT OR Apache-2.0, matching the rest of this repository.
