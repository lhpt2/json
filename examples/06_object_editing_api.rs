//! `Value::get`/`get_mut`/`insert`/`remove` (for objects, by key) and
//! `Value::get_index`/`get_index_mut`/`push`/`remove_index` (for
//! arrays, by position) -- convenience methods so editing code doesn't
//! have to match on `Value::Object`/`Value::Array` and walk
//! `entries`/`items` by hand every time, the way the earlier examples
//! did. `remove`/`remove_index` are the interesting ones: they carry
//! the removed node's comment(s) forward onto whatever takes its place,
//! per the crate's deletion rule (see `Value::remove`'s doc comment) --
//! this example's whole point is showing that actually happen.
//!
//! Run with: `cargo run --example 06_object_editing_api`

use cson_edit::{parse, Node, Value};

fn main() -> Result<(), cson_edit::ParseError> {
    let source = r#"
name: "grid-controller"

server: {
  # ops override, see runbook#42
  port: 8080
  timeout: 30
}

# One entry per channel
channels: [
  "left"
  # the loud one
  "right"
  "sub"
]
"#;

    let mut doc = parse(source)?;
    let root = doc.root_mut().value_mut();

    // get / get_index: lookup without matching on the Value variant
    // yourself. Both return None rather than panicking on a missing
    // key/index or the wrong variant.
    assert!(root.get("name").is_some());
    assert!(root.get("nope").is_none());
    assert!(root.get_index(0).is_none()); // root is an object, not an array

    // insert: append a new entry. Always appends, even if the key
    // already exists -- CSON keeps duplicate keys rather than silently
    // overwriting (see Value::Object's doc comment) -- so this is for
    // adding a field that isn't there yet, not for updating one (use
    // get_mut + Node::set_value for that, as in 03_edit_preserving_comments.rs).
    root.insert("version", Node::new(Value::from_serialize(&1i64)?)).expect("root is an object");

    // remove: delete "port" from the nested server object. Its comment,
    // "# ops override, see runbook#42", doesn't disappear with it -- it
    // moves onto the entry that's now first, "timeout".
    if let Some(server) = root.get_mut("server") {
        let removed = server.value_mut().remove("port").expect("port should exist");
        assert_eq!(removed.prefix(), ""); // relocated, not duplicated on the returned node
    }

    // remove_index: same rule for an array. "right"'s comment ("# the
    // loud one") moves onto whatever is now at its old position, "sub".
    if let Some(channels) = root.get_mut("channels") {
        channels.value_mut().remove_index(1).expect("index 1 should exist");
        // push: append a new element, same as insert for objects.
        channels.value_mut().push(Node::new(Value::from_serialize(&"aux")?)).expect("channels is an array");
    }

    let text = doc.to_cson_string();
    println!("{text}");

    assert!(text.contains("version: 1"));
    assert!(!text.contains("port"));
    assert!(text.contains("# ops override, see runbook#42"));
    // the comment now sits directly above `timeout`, not `port`
    let timeout_line = text.lines().position(|l| l.contains("timeout")).unwrap();
    let comment_line = text.lines().position(|l| l.contains("# ops override")).unwrap();
    assert_eq!(comment_line + 1, timeout_line);

    assert!(!text.contains("\"right\""));
    assert!(text.contains("# the loud one"));
    let sub_line = text.lines().position(|l| l.contains("\"sub\"")).unwrap();
    let loud_comment_line = text.lines().position(|l| l.contains("# the loud one")).unwrap();
    assert_eq!(loud_comment_line + 1, sub_line);
    assert!(text.contains("\"aux\""));

    println!("(insert/remove/push applied; both removed nodes' comments relocated, not lost)");

    Ok(())
}
