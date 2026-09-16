//! The whole reason this crate exists (the "toml_edit for CSON" use
//! case): change one value in a document and write it back out with
//! every comment and every other value's exact formatting untouched.
//!
//! There is no automatic "diff a struct against a Document" helper yet
//! (`Document::merge_from`, see the crate README) -- so this shows the
//! manual pattern that stands in for it today: find the `Node` you
//! want to change via `root_mut()`, and overwrite only its *value*
//! (never the whole `Node`, or its `prefix` -- and thus its comment --
//! goes with it).
//!
//! Run with: `cargo run --example 03_edit_preserving_comments`

use cson_edit::{parse, Node, Value};

fn main() -> Result<(), cson_edit::ParseError> {
    let source = r#"
# Application configuration -- do not remove this header
name = "my-service"

server: {
  host: "localhost"
  port: 8080  # ops override, see runbook#42
}

# Feature flags
tags: ["web", "prod"]
"#;

    let mut doc = parse(source)?;

    // Reach into the tree: root -> "server" entry -> its value (an
    // object) -> "port" entry -> its value node.
    let port_node = find_mut(doc.root_mut().value_mut(), &["server", "port"])
        .expect("server.port should exist");

    // The important part: Value::from_serialize builds a fresh, valid
    // value from any Serialize type (here just an i64), and set_value
    // replaces *only* the value -- the node's prefix, which is where
    // "# ops override, see runbook#42" lives, is left exactly alone.
    port_node.set_value(Value::from_serialize(&9090i64)?);

    // Same idea for a plain string field, one level up: replace name's
    // value while keeping the header comment above it untouched.
    if let Value::Object { entries, .. } = doc.root_mut().value_mut() {
        for entry in entries.iter_mut() {
            if entry.key_str() == Some("name") {
                entry.value_mut().set_value(Value::from_serialize(&"renamed-service")?);
            }
        }
    }

    let text = doc.to_cson_string();
    println!("{text}");

    assert!(text.contains("# Application configuration"));
    assert!(text.contains("# ops override, see runbook#42"));
    assert!(text.contains("# Feature flags"));
    assert!(text.contains("9090"));
    assert!(text.contains("renamed-service"));
    assert!(!text.contains("8080"));
    println!("(comments intact, both edits applied)");

    Ok(())
}

/// Walks `value` through a sequence of object keys, returning the final
/// key's `Node` itself (not just its value) -- so the caller can call
/// `set_value` on it directly. A tiny hand-rolled path-finder, standing
/// in for the recursive matching `Document::merge_from` will eventually
/// automate.
fn find_mut<'a, 'doc>(
    value: &'a mut Value<'doc>,
    path: &[&str],
) -> Option<&'a mut Node<'doc>> {
    let Value::Object { entries, .. } = value else {
        return None;
    };
    let (first, rest) = path.split_first()?;
    let entry = entries.iter_mut().find(|e| e.key_str() == Some(*first))?;
    if rest.is_empty() {
        Some(entry.value_mut())
    } else {
        find_mut(entry.value_mut().value_mut(), rest)
    }
}
