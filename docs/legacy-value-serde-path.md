# The legacy `Value`/`serde::Deserializer` CSON path

This document describes `src/de.rs`, `src/read.rs`, `src/ser.rs` and
`src/value/`: the original serde_json parser/serializer, incrementally
patched to accept and emit CSON syntax. In the three-layer architecture
`CLAUDE.md` describes, this is the **"Wegwerf-Lesen"** path (`text ->
serde_json::Value` / any `Deserialize` type, directly) — it has no concept
of trivia and cannot round-trip comments. For that, see
[`document-module.md`](./document-module.md).

This file exists to answer one question precisely: **for a given piece of
CSON syntax, does the legacy path actually read it, actually write it, or
neither?** The two are not symmetric.

## Read side (`de.rs`)

| Feature | Supported? | Where |
|---|---|---|
| `#`-comments | Yes, skipped as whitespace (discarded, not preserved) | `Deserializer::parse_whitespace` |
| Array items separated by `,` | Yes | `SeqAccess::next_element_seed` |
| Array items separated by a bare newline | Yes | same, `peek == b'\n' \|\| peek == b','` |
| Trailing comma before `]` | Yes | same |
| Object entries separated by `,` | Yes | `MapAccess::next_key_seed` |
| Object entries separated by a bare newline | **No** — only `MapAccess` lacks the `\n` branch `SeqAccess` has | same |
| Trailing comma before `}` | **No** — explicitly rejected as `ErrorCode::TrailingComma` | same |
| `key = value` (`=` separator) | **No** — `parse_object_colon` only accepts `:` | `Deserializer::parse_object_colon` |
| Bare (unquoted) object keys | **No** — the first byte of a key must be `"` or the whole map fails with `KeyMustBeAString` | `MapAccess::next_key_seed`, `MapKey::deserialize_any` |
| `'single-quoted'` strings | **No** — `Read::parse_str` and friends only ever look for `"` | `read.rs` |
| `\|verbatim strings` | **No** — the lexer has no branch for `\|` at all | `de.rs` (absent) |
| `\'` escape inside a string | Yes (as of this change) — accepted in `"..."` too, since the grammar allows it everywhere | `read::parse_escape` |

The practical implication: **the legacy `Deserializer` can only read the
JSON-shaped subset of CSON**, plus comments (discarded) and the two array
conveniences (newline-as-separator, trailing comma). Anything using `=`,
bare keys, `'...'` strings, or `\|` verbatim strings will fail to parse
through `from_str`/`from_slice`/`from_reader`.

## Write side (`ser.rs`)

`CompactFormatter` is untouched — `to_string`/`to_vec`/`to_writer` produce
plain JSON, no CSON syntax at all.

`PrettyFormatter` (used by `to_string_pretty`/`to_vec_pretty`/
`to_writer_pretty`, or directly via `Serializer::with_formatter`) is where
all the CSON output behavior lives:

- **Bare keys**: a key is written unquoted automatically when it doesn't
  contain whitespace or any of `` |:={}[], `` (see `string_is_complex`).
  There is no way to force quoting of a key that would otherwise qualify
  as bare, or vice versa — it's a heuristic, not a style choice.
- **Verbatim (`|`) strings**: a *value* string is written using `|` blocks
  automatically when it "looks multi-line enough" — more than one line
  break and longer than 5 bytes (`string_is_verbatim_candidate`). This is
  a heuristic too, not configurable, and it never applies to keys.
- **Key/value separator**: `=` or `:`. Previously hardcoded to `=`; now
  configurable, defaulting to the same `=` as before (see below).
- **Quote character**: `"` or `'`. Previously hardcoded to `"`; now
  configurable, defaulting to the same `"` as before (see below).
- **Indentation**: configurable since before this change, via
  `PrettyFormatter::with_indent(indent: &[u8])` (default: two spaces).

### What changed: `with_separator` / `with_quote`

```rust
use serde_json::ser::{PrettyFormatter, Serializer};
use serde_json::document::{Separator, Quote};

let formatter = PrettyFormatter::new()
    .with_separator(Separator::Colon)   // `key: value` instead of `key = value`
    .with_quote(Quote::Single);         // 'value' instead of "value"

let mut out = Vec::new();
let mut ser = Serializer::with_formatter(&mut out, formatter);
value.serialize(&mut ser)?;
```

Both builder methods consume and return `Self`, so they chain onto
`PrettyFormatter::new()` or `PrettyFormatter::with_indent(..)`. Neither
changes the default: a plain `PrettyFormatter::new()` still produces
exactly the `=`/`"` output it always did.

Implementation, for anyone touching this later:

- `Formatter` gained a new method, `fn quote_char(&self) -> u8 { b'"' }`
  (default `"`, so `CompactFormatter` and any third-party `Formatter`
  impl are unaffected). `PrettyFormatter` overrides it to return its own
  `quote` field.
- `Formatter::begin_string`/`end_string`/the `Quote` arm of
  `write_char_escape` all switched from the literal `b'"'` to
  `self.quote_char()`.
- The escape decision for string *contents* used to be a single static
  256-entry lookup table (`ESCAPE`) that always escapes `"` and never
  escapes `'`. That table is still there (it's still correct for the
  default `"`-quoted case and cheap to keep), but it's no longer read
  directly — `escape_byte(byte, quote)` wraps it: the currently active
  quote byte always maps to "needs escaping", and `"` stops needing to
  once it *isn't* the active quote. This matches the grammar directly:
  `dquoted-unescaped` allows a raw `'` inside `"..."`, and
  `squoted-unescaped` allows a raw `"` inside `'...'` — only the
  delimiter itself must be escaped.
- `read::parse_escape` (used by the legacy *read* side) gained a `\'` ⇒
  `'` case, since the grammar's `escaped` production allows `\'`
  unconditionally, not just inside apostrophe strings. This doesn't (by
  itself) make the legacy reader accept `'...'` strings — see the table
  above — it just means a `"...\'..."` string parses correctly if it ever
  shows up.

### The write/read asymmetry this creates

Because the *read* side didn't gain matching support, a document written
with `with_quote(Quote::Single)` — or one containing bare keys or `|`
verbatim strings, which the writer has produced unconditionally since
before this change — **cannot be read back through this same legacy
`Deserializer`**. It can only be read back through the new
[`document` module's parser](./document-module.md), which implements the
full CSON grammar. If you need read/write round-tripping through this
legacy path specifically, stick to `Separator::Equals` (or `Colon`, once
you've verified you're only ever going to *write*, not read, that output)
with `Quote::Double`, and be aware that any bare key or verbatim string
the writer emits is already a one-way trip.

## Tests

- `tests/test.rs`: `test_parse_comments`, `test_verbatim_strings`, plus
  the pre-existing pretty-print fixtures cover the write-side behavior
  above. `test_pretty_formatter_separator_is_configurable` and
  `test_pretty_formatter_quote_is_configurable` cover the new knobs,
  including that defaults are unchanged and that the *other* quote
  character stops being escaped once it isn't the active one.
