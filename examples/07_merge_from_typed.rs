//! The typed editing path: read a config into your own struct, change
//! a field like any other Rust value, and write it back into the
//! *original* document with `Document::merge_from` -- comments, blank
//! lines, and every untouched value's exact source text preserved.
//!
//! This is the counterpart to 03/06, which edit the tree by hand. The
//! difference from `Document::from_serialize` matters: that builds a
//! brand new tree (no comments, default Style), so it can't be used to
//! write an edit back. `merge_from` diffs instead of replacing, and
//! only rewrites what actually differs.
//!
//! Run with: `cargo run --example 07_merge_from_typed`

use cson_edit::parse;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    name: String,
    server: Server,
    channels: Vec<String>,
    // 1.50 in the file; note it stays 1.50 below, because merge_from
    // compares numbers numerically and leaves an unchanged one's raw
    // literal completely alone.
    gain: f64,
}

fn main() -> Result<(), cson_edit::ParseError> {
    let source = r#"
# Device configuration -- keep these comments!
name: "grid-controller"

# Where the control server listens
server: {
  host: "localhost"
  port: 8080  # ops override, see runbook#42
}

# One entry per channel
channels: [
  "left"
  # the loud one
  "right"
]

gain: 1.50
"#;

    let mut doc = parse(source)?;

    // Read it into your own type (Schicht 2). The Document stays alive
    // and still holds every comment -- `cfg` is just a plain struct.
    let mut cfg: Config = doc.deserialize()?;
    println!("read: {cfg:?}\n");

    // Edit it as ordinary Rust.
    cfg.server.port = 9090;
    cfg.channels.push("sub".to_string());

    // Write it back into the *same* document.
    doc.merge_from(&cfg)?;

    let text = doc.to_cson_string();
    println!("{text}");

    // Every comment survived, including the one on the line whose value
    // changed, and the one attached to an array element.
    assert!(text.contains("# Device configuration"));
    assert!(text.contains("# Where the control server listens"));
    assert!(text.contains("# ops override, see runbook#42"));
    assert!(text.contains("# One entry per channel"));
    assert!(text.contains("# the loud one"));

    // The edits landed...
    assert!(text.contains("9090"));
    assert!(!text.contains("8080"));
    assert!(text.contains("\"sub\""));

    // ...and `gain` was left byte-for-byte alone: the struct's 1.5 and
    // the file's 1.50 are the same number, so merge_from didn't rewrite
    // it. A textual comparison would have churned it on every save.
    assert!(text.contains("1.50"));

    println!("(typed edit applied; comments and the 1.50 literal intact)");

    Ok(())
}
