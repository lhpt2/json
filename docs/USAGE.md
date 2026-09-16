# Using `cson_edit`

A task-oriented walkthrough. For the data model, the parser pipeline, and
the design rationale behind all of it, see the top-level `README.md`
instead — this document only answers "how do I do X," in the order
you're likely to need it, and points at the runnable example that goes
with each answer.

Every example below lives in `examples/` and can be run directly:

```sh
cargo run --example 01_typed_read_write
cargo run --example 02_parse_and_inspect
cargo run --example 03_edit_preserving_comments
cargo run --example 04_custom_style
```

## Installing

```toml
[dependencies]
cson_edit = "0.1"
```

The default `std` feature adds `from_reader`/`to_writer` (`io::Read`/
`Write`). With `default-features = false`, the crate is `#![no_std]` +
`alloc` and drops those two functions; everything else is unaffected.

## "I just want to read/write a typed struct, no comments"

Use the five/seven top-level functions — they match the naming you
already know from `serde_json`/`serde_yaml`/`toml`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Config {
    name: String,
    port: u16,
}

let cfg = Config { name: "svc".into(), port: 8080 };
let text = cson_edit::to_string(&cfg)?;
let cfg2: Config = cson_edit::from_str(&text)?;
# Ok::<(), cson_edit::ParseError>(())
```

`from_slice`/`to_vec` do the same over `&[u8]`/`Vec<u8>`; `from_reader`/
`to_writer` (default `std` feature only) do it over `io::Read`/`Write`.

This is the whole story if your data is disposable — generated, not
hand-maintained. **Any comments in the input are silently dropped**:
`from_str` parses into a `Document` internally and throws it away the
moment `T` comes out, so there is nothing left to write comments back
from. If the file is something a person edits and annotates, keep
reading.

Full runnable version: `examples/01_typed_read_write.rs`.

## "I want to look at a document's structure without a target struct"

Parse into a `Document` and walk it by hand. This is the read-only half
of the editing story below, useful on its own for e.g. writing a linter
or a config summary tool.

```rust
use cson_edit::{parse, Value};

let doc = parse("name = \"svc\"  # prod\nport = 8080\n")?;
if let Value::Object { entries, .. } = doc.root().value() {
    for entry in entries {
        println!("{:?} -> {:?}", entry.key_str(), entry.value().value());
    }
}
# Ok::<(), cson_edit::ParseError>(())
```

`Value` is a plain `match`-able enum (`Null`, `Bool`, `Number`, `Str`,
`Array { items, .. }`, `Object { entries, .. }`); no visitor pattern,
no serde involved. `Entry::key_str()` is a convenience for the common
case where a key is a plain string (always true for CSON — `bare-string`
is only ever a key, never a value, so this can't fail on valid input).

Full runnable version: `examples/02_parse_and_inspect.rs`.

## "I want to change one value and keep every comment"

This is what the crate is for. There is no automatic "diff a struct
against a `Document`" helper yet (`Document::merge_from` — see the
README's "Status" section), so today this means: find the `Node` you
want to change, and overwrite **only its `value`**, never the whole
`Node`. A `Node`'s `prefix` is where its comment lives; replacing the
`Node` itself throws the comment away along with the old value.

```rust
use cson_edit::{parse, Value};

let mut doc = parse(
    "# ops override, see runbook#42\nport = 8080\n"
)?;

if let Value::Object { entries, .. } = doc.root_mut().value_mut() {
    if let Some(entry) = entries.iter_mut().find(|e| e.key_str() == Some("port")) {
        entry.value_mut().set_value(Value::from_serialize(&9090i64)?);
    }
}

let text = doc.to_cson_string();
assert!(text.contains("# ops override"));
assert!(text.contains("9090"));
# Ok::<(), cson_edit::ParseError>(())
```

The API surface this pattern relies on:

* `doc.root_mut()` / `Node::value_mut()` — mutable access down into the
  tree, without disturbing any `prefix` you don't explicitly touch.
* `Value::from_serialize(&x)` — builds a fresh `Value` from any
  `Serialize` type (an `i64`, a `String`, a whole nested struct), the
  same way the typed write path (`Document::from_serialize`) does
  internally. This is the normal way to produce a replacement value by
  hand, rather than constructing `Value::Number`/`Value::Str` variants
  directly.
* `Node::set_value` — replaces a node's value in place, leaving its
  `prefix` (comment) untouched. There's also `Entry::key_mut()` for
  renaming a key the same way.

For a deeper path than one level, write a small recursive helper that
returns `&mut Node` at the end of the path (see `find_mut` in the
example below) — there's no built-in path/pointer API for this yet,
since it would need to make a choice about missing-key behavior
(`merge_from`'s job, eventually) that a general-purpose helper
shouldn't guess at.

Full runnable version (two edits, nested and top-level, both verified
by checking the comments and the old value are gone/present as
expected): `examples/03_edit_preserving_comments.rs`.

## "I want to control how it's written out"

The writer never reads layout back from the source — it always renders
from `Style`, uniformly across the whole document (see the README's
"The two axes" discussion for why). `parse` detects a `Style` from the
first occurrence of each choice in the source (first separator seen,
first quote character seen, and so on); you can read it back with
`doc.style()`, or override it before writing:

```rust
use cson_edit::{parse, Indent, IndentChar, Quote, Separator, Style};

let mut doc = parse(r#"name: "svc""#)?;
doc.set_style(Style {
    separator: Some(Separator::Equals),
    value_quote: Some(Quote::Single),
    indent: Some(Indent { ch: IndentChar::Space, width: 4 }),
    ..Style::default()
});
println!("{}", doc.to_cson_string()); // name = 'svc'
# Ok::<(), cson_edit::ParseError>(())
```

`Style::default()` fills in every field you don't set explicitly (`:`
separator, double-quoted values, bare keys where possible, commas on,
2-space indent). Two fields — indentation and `|`-verbatim strings —
have no reliable first-match in many real files (no nested element to
read indent from; verbatim strings often just aren't used), so their
defaults are exactly that: defaults, not a detection guess.

Full runnable version: `examples/04_custom_style.rs`.

## Error handling

Every fallible function in this crate returns
`Result<T, cson_edit::ParseError>`. `ParseError` carries a byte offset
and 1-based line/column alongside the message (`Display` includes all
three), and implements both `std::error::Error` (default `std` feature)
and both serde `de::Error`/`ser::Error` traits, so it composes with `?`
against typed (de)serialization failures the same way a syntax error
does.

## Where to go next

* `README.md` — architecture, the trivia model, the four round-trip
  invariants, `Style`'s first-match detection rules, and exactly what
  Schicht 2/3 do and don't give you.
* `examples/` — the four programs this guide walks through, plus their
  inline comments cover a few details (e.g. `CsonStr`/`Number`'s raw
  literal storage) this guide doesn't repeat.
