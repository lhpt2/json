//! `parse` gives you a [`cson_edit::Document`]: a tree that keeps
//! comments and blank lines, not just values. This example walks that
//! tree by hand and shows the round trip (`parse` -> `to_cson_string`)
//! keeping every comment, without touching any typed struct at all.
//!
//! Run with: `cargo run --example 02_parse_and_inspect`

use cson_edit::{parse, Entry, Value};

fn main() -> Result<(), cson_edit::ParseError> {
    let source = r#"
# Application configuration
name = "my-service"  # shown in logs

server: {
  host: "localhost"
  port: 8080  # non-standard; ops knows why
}

# Feature flags, one per line, no trailing comma needed
tags: [
  "web"
  "prod"
]
"#;

    let doc = parse(source)?;

    // Document::root() is always an object here, since this source
    // parsed as a bare (brace-less) top-level object -- bare_root()
    // confirms it, since CSON never allows a bare top-level *array*.
    println!("bare_root: {}", doc.bare_root());

    // Walk the tree: an object's entries are `Entry { key, value }`
    // pairs, each a full Node (so a comment right before a key has
    // somewhere to live).
    if let Value::Object { entries, .. } = doc.root().value() {
        print_entries(entries, 0);
    }

    // Nothing was edited, so writing it back out reproduces the same
    // comments, same structure. The exact bytes need not match `source`
    // -- Style always renders uniformly -- but a *second* round trip
    // through parse+write is guaranteed to be a fixpoint (see the
    // crate README for the four invariants this guarantees).
    let text = doc.to_cson_string();
    println!("\n--- Written back out ---\n{text}");
    assert!(text.contains("# non-standard"));
    assert!(text.contains("# Feature flags"));
    println!("(all comments still present)");

    Ok(())
}

fn print_entries(entries: &[Entry], depth: usize) {
    let indent = "  ".repeat(depth);
    for entry in entries {
        let key = entry.key_str().unwrap_or("<non-string key>");
        let prefix = entry.key().prefix().trim();
        if !prefix.is_empty() {
            println!("{indent}# comment before `{key}`: {prefix:?}");
        }
        match entry.value().value() {
            Value::Object { entries, .. } => {
                println!("{indent}{key}: {{");
                print_entries(entries, depth + 1);
                println!("{indent}}}");
            }
            Value::Array { items, .. } => {
                println!("{indent}{key}: [{} items]", items.len());
            }
            Value::Str(s) => println!("{indent}{key} = {:?}", s.as_str()),
            Value::Number(n) => println!("{indent}{key} = {}", n.as_str()),
            Value::Bool(b) => println!("{indent}{key} = {b}"),
            Value::Null => println!("{indent}{key} = null"),
        }
    }
}
