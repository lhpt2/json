//! Reading and writing actual CSON files on disk, not just in-memory
//! strings -- and, since that's almost always why you'd reach for this
//! crate over `serde_json`, doing it while an edit is applied and every
//! comment in the file survives.
//!
//! `cson_edit` has no dedicated "open this path" function: a `Document`
//! borrows from the `&str` it was parsed from (so it can stay
//! zero-copy where possible), so *you* own reading the file into a
//! `String` first, the same way you would for any other borrowing
//! parser (`toml`, `syn`, ...). Writing is the same: `to_cson_string`
//! gives you a `String`, and you own putting it on disk.
//!
//! Run with: `cargo run --example 05_file_roundtrip`
//! (run from the crate root, e.g. via `cargo run`, so the relative
//! fixture path below resolves -- see the `CARGO_MANIFEST_DIR` note.)

use cson_edit::{parse, Value};
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A fixture file checked into examples/, not a temp string, so this
    // demonstrates a real file read. CARGO_MANIFEST_DIR anchors the path
    // to the crate root regardless of the process's current directory.
    let input_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sample_with_comments.csn");
    let source = fs::read_to_string(&input_path)?;

    let mut doc = parse(&source)?;
    println!("--- read from {} ---\n{source}", input_path.display());

    // Apply an edit the same way 03_edit_preserving_comments.rs does,
    // via the Value::get_mut/push convenience methods described in
    // 06_object_editing_api.rs: bump the port, and append a new button
    // to the array (push doesn't disturb any existing Node, so there's
    // nothing to overwrite for that one).
    let root = doc.root_mut().value_mut();
    if let Some(port) = root.get_mut("server").and_then(|s| s.value_mut().get_mut("port")) {
        port.set_value(Value::from_serialize(&9090i64)?);
    }
    if let Some(buttons) = root.get_mut("buttons") {
        buttons.value_mut().push(cson_edit::Node::new(Value::from_serialize(&"shift")?)).expect("buttons is an array");
    }

    let output = doc.to_cson_string();

    // Write it back out to a *different* file -- this example doesn't
    // touch the checked-in fixture. In a real config-editing tool this
    // would usually be `fs::write(&input_path, output)` to overwrite
    // the file it read from.
    let output_path = std::env::temp_dir().join("cson_edit_example_output.csn");
    fs::write(&output_path, &output)?;
    println!("--- wrote to {} ---\n{output}", output_path.display());

    // Read it back to confirm the round trip: every comment from the
    // source is still there, and both edits landed.
    let reread = fs::read_to_string(&output_path)?;
    assert!(reread.contains("# Device configuration"));
    assert!(reread.contains("# Network settings for the control server"));
    assert!(reread.contains("# overridden in prod, see deploy notes"));
    assert!(reread.contains("# One entry per physical button on the grid"));
    assert!(reread.contains("# long-press stops all channels, not just this one"));
    assert!(reread.contains("9090"));
    assert!(!reread.contains("8080"));
    assert!(reread.contains("\"shift\""));
    println!("(round trip through disk OK: comments intact, both edits applied)");

    fs::remove_file(&output_path)?;

    // If you don't need comments to survive -- just typed values, e.g.
    // loading a config into a struct at startup -- from_reader/
    // to_writer (used with std::fs::File) skip the Document entirely.
    // See 01_typed_read_write.rs for that path in detail; it works the
    // same way over a real file:
    //
    //   let f = std::fs::File::open(&input_path)?;
    //   let cfg: MyConfig = cson_edit::from_reader(f)?;
    //   let out = std::fs::File::create(&output_path)?;
    //   cson_edit::to_writer(out, &cfg)?;

    Ok(())
}
