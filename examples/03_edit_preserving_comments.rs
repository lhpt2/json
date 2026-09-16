//! The whole reason this crate exists (the "toml_edit for CSON" use
//! case): change one value in a document and write it back out with
//! every comment and every other value's exact formatting untouched.
//!
//! This is the by-hand version: find the `Node` you want to change via
//! `root_mut()`, and overwrite only its *value* (never the whole
//! `Node`, or its `prefix` -- and thus its comment -- goes with it).
//! If you'd rather edit a typed struct and have the diffing done for
//! you, `Document::merge_from` does exactly this walk automatically --
//! see 07_merge_from_typed.rs.
//!
//! Run with: `cargo run --example 03_edit_preserving_comments`

use cson_edit::{parse, Value};

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

    // Reach into the tree with Value::get_mut, one object level at a
    // time: root -> "server" entry's value (an object) -> "port"
    // entry's value node. get_mut returns None for a missing key or a
    // non-object Value, rather than panicking -- exactly what `?`/
    // `.expect(...)` want on the other end.
    let port_node = doc
        .root_mut()
        .value_mut()
        .get_mut("server")
        .expect("server should exist")
        .value_mut()
        .get_mut("port")
        .expect("server.port should exist");

    // The important part: Value::from_serialize builds a fresh, valid
    // value from any Serialize type (here just an i64), and set_value
    // replaces *only* the value -- the node's prefix, which is where
    // "# ops override, see runbook#42" lives, is left exactly alone.
    port_node.set_value(Value::from_serialize(&9090i64)?);

    // Same idea for a plain string field, one level up: replace name's
    // value while keeping the header comment above it untouched.
    doc.root_mut()
        .value_mut()
        .get_mut("name")
        .expect("name should exist")
        .set_value(Value::from_serialize(&"renamed-service")?);

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
