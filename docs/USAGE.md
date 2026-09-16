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
cargo run --example 05_file_roundtrip
cargo run --example 06_object_editing_api
cargo run --example 07_merge_from_typed
cargo run --example 08_hot_reload
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

This is what the crate is for. Two ways to do it: by hand (this
section) or through your own typed struct (the next one). By hand
means: find the `Node` you want to change, and overwrite **only its
`value`**, never the whole `Node`. A `Node`'s `prefix` is where its
comment lives; replacing the `Node` itself throws the comment away
along with the old value.

```rust
use cson_edit::{parse, Value};

let mut doc = parse(
    "# ops override, see runbook#42\nport = 8080\n"
)?;

if let Some(port) = doc.root_mut().value_mut().get_mut("port") {
    port.set_value(Value::from_serialize(&9090i64)?);
}

let text = doc.to_cson_string();
assert!(text.contains("# ops override"));
assert!(text.contains("9090"));
# Ok::<(), cson_edit::ParseError>(())
```

The API surface this pattern relies on:

* `doc.root_mut()` / `Node::value_mut()` — mutable access down into the
  tree, without disturbing any `prefix` you don't explicitly touch.
* `Value::get_mut(key)` — looks up an entry by key on a
  [`Value::Object`], returning `None` for a missing key or a non-object
  `Value` rather than panicking. For a nested path, chain it:
  `root.get_mut("server").and_then(|s| s.value_mut().get_mut("port"))`.
* `Value::from_serialize(&x)` — builds a fresh `Value` from any
  `Serialize` type (an `i64`, a `String`, a whole nested struct), the
  same way the typed write path (`Document::from_serialize`) does
  internally. This is the normal way to produce a replacement value by
  hand, rather than constructing `Value::Number`/`Value::Str` variants
  directly.
* `Node::set_value` — replaces a node's value in place, leaving its
  `prefix` (comment) untouched. There's also `Entry::key_mut()` for
  renaming a key the same way.

Full runnable version (two edits, nested and top-level, both verified
by checking the comments and the old value are gone/present as
expected): `examples/03_edit_preserving_comments.rs`.

## "I want to edit my own struct and write it back, comments intact"

`Document::merge_from` does the tree-walking for you: deserialize the
document into your type, change it like any other Rust value, then
merge it back into the *same* document.

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Config { name: String, port: u16 }

let mut doc = cson_edit::parse("name: \"svc\"\nport: 8080  # ops override\n")?;

let mut cfg: Config = doc.deserialize()?;
cfg.port = 9090;
doc.merge_from(&cfg)?;

let text = doc.to_cson_string();
assert!(text.contains("9090"));
assert!(text.contains("# ops override"));
# Ok::<(), cson_edit::ParseError>(())
```

Note what this is *not*: `Document::from_serialize(&cfg)` builds a
brand new document with no comments at all, so writing that out would
lose everything. `merge_from` diffs instead — it walks the fresh tree
and the document together and rewrites only what differs:

* unchanged scalars aren't touched at all, so their comments *and*
  their exact source text survive — including numbers, which are
  compared numerically, so a file's `1.50` isn't churned into `1.5`
  just because that's how your `f64` formats;
* changed scalars have only their value replaced, never the `Node`
  around them (which is where the comment lives);
* objects are matched by key, arrays by position; keys/elements only in
  your struct get appended, ones only in the document get removed via
  `Value::remove`/`remove_index` — so even deletions migrate their
  comments instead of dropping them.

Two things to know before relying on it:

* **`#[serde(skip)]` / `skip_serializing_if` fields get removed from
  the document.** In the freshly serialized tree they're simply absent,
  which is indistinguishable from "the caller deleted this key". If
  your type skips fields the file is meant to keep, edit those keys by
  hand (previous section) rather than merging.
* **Duplicate keys**: a serialized struct never has any, but a
  hand-written document can. The first entry with a given key is the
  one merged into; later duplicates are left alone.

Full runnable version: `examples/07_merge_from_typed.rs`.

## "I want to add or delete an entry, not just change a value"

`Value` has a small get/insert/remove API (for `Value::Object`, by key;
`Value::Array`'s equivalents are index-based: `get_index`/
`get_index_mut`/`push`/`remove_index`) — no need to match on the
variant and walk `entries`/`items` by hand for the common cases:

```rust
use cson_edit::{parse, Node, Value};

let mut doc = parse("a: 1\n# about b\nb: 2\nc: 3\n")?;
let root = doc.root_mut().value_mut();

root.insert("d", Node::new(Value::from_serialize(&4i64)?))?; // always appends

let removed = root.remove("b").unwrap();
assert_eq!(removed.prefix(), ""); // its comment didn't just vanish...
let text = doc.to_cson_string();
assert!(text.contains("# about b")); // ...it moved onto `c`, which took its place
# Ok::<(), cson_edit::ParseError>(())
```

`remove`/`remove_index` are the ones worth knowing well: deleting an
entry with `Vec::remove` on `entries`/`items` directly would silently
drop that entry's comment along with it. These instead implement the
crate's deletion rule -- the removed node's comment(s) move onto
whatever now takes its place (the following entry/element's prefix, or
the container's `trailing` slot if the removed one was last) -- so the
comment always describes *something* nearby afterward, never nothing.
The node they return has its own prefix cleared for exactly this
reason: if you keep it and reinsert it elsewhere, its old comment isn't
duplicated (it already lives at the new spot).

`insert`/`push` always append, even if the key already exists -- CSON
keeps duplicate keys rather than silently merging them (see
`Value::Object`'s doc comment) -- so they're for adding an entry that
isn't there yet, not updating one (use `get_mut` + `Node::set_value`
for that, as in the previous section).

Full runnable version, including the array side (`get_index`/`push`/
`remove_index`) and printing the before/after text so the comment
relocation is visible: `examples/06_object_editing_api.rs`.

## "I want to read and write an actual `.cson` file on disk"

There's no `Document::open(path)` — a `Document<'a>` borrows from the
`&str` it was parsed from wherever it can, so *you* read the file into
a `String` first (the same shape as `toml`, `syn`, and most other
borrowing parsers), and `parse` borrows from that:

```rust,no_run
use cson_edit::{parse, Value};
use std::fs;

let source = fs::read_to_string("config.cson")?;
let mut doc = parse(&source)?;

if let Some(port) = doc.root_mut().value_mut().get_mut("port") {
    port.set_value(Value::from_serialize(&9090i64)?);
}

fs::write("config.cson", doc.to_cson_string())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

That's the whole pattern: `fs::read_to_string` in, the "find the
`Node`, overwrite its `value`" edit from the previous section, then
`fs::write` the result of `to_cson_string()` back out — to the same
path to overwrite the file in place, or a different one. Nothing here
is file-specific; it's the in-memory edit pattern above with a
`fs::read_to_string`/`fs::write` on either end.

If you don't need comments to survive (see the first section above),
the same substitution applies to the top-level typed functions:
`from_reader`/`to_writer` (default `std` feature) take any `io::Read`/
`io::Write`, including a plain `std::fs::File`:

```rust,no_run
# use serde::{Deserialize, Serialize};
# #[derive(Serialize, Deserialize)] struct Config { port: u16 }
let f = std::fs::File::open("config.cson")?;
let cfg: Config = cson_edit::from_reader(f)?;

let out = std::fs::File::create("config.cson")?;
cson_edit::to_writer(out, &cfg)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Full runnable version — reads `examples/sample_with_comments.cson`
from disk, edits a nested field and appends an array element, writes
the result to a temp file, then reads that back and checks every
comment survived and both edits landed:
`examples/05_file_roundtrip.rs`.

## "I want to keep a document around, or reload it when the file changes"

`parse` borrows: a `Document<'a>` holds `&'a str` slices of the text it
came from, which is what makes it cheap, but also means it can't
outlive that `String`, be stored in a struct next to it, or be moved to
a thread that doesn't own the source. `Document::into_owned()` copies
every borrowed slice (comments included) into the document itself,
giving you a `Document<'static>`:

```rust
use cson_edit::Document;

fn load(path: &str) -> Result<Document<'static>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(cson_edit::parse(&text)?.into_owned()) // `text` dies here; the document doesn't
}
```

Anything already owned is moved through untouched, so re-owning costs
only the walk, and the result is an ordinary document — editing,
`merge_from` and writing all work on it as before.

That's the piece hot reloading needs. The watching itself isn't in this
crate on purpose: a file watcher (`notify`) is a std-only,
platform-specific dependency tree, and the interesting parts are policy
your app owns — how long to debounce (editors emit several events per
save), what to do when you catch a file mid-write and it doesn't parse
(keep the last good config, don't clobber it), whether to watch the
directory rather than the file (editors save by rename, which replaces
the inode), and how to avoid your own writes retriggering your own
watcher.

`examples/08_hot_reload.rs` is a complete, runnable version of exactly
that — `Arc<Mutex<Document<'static>>>` swapped from a watcher thread,
with debouncing and last-known-good handling — written to be copied
into your app and adjusted rather than called. It uses `notify` as a
**dev**-dependency, so it costs nothing to anyone depending on the
crate.

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
