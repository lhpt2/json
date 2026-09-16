//! The plainest way to use this crate: typed read/write with no need to
//! keep comments around. If that's all you need, these five functions
//! (`from_str`, `to_string`, `from_slice`, `to_vec`, and -- behind the
//! default `std` feature -- `from_reader`/`to_writer`) are the whole
//! API surface you need, matching the conventions of `serde_json`/
//! `serde_yaml`/`toml`.
//!
//! Run with: `cargo run --example 01_typed_read_write`

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    server: Server,
    tags: Vec<String>,
    retries: Option<u32>,
}

fn main() -> Result<(), cson_edit::ParseError> {
    // Parsing accepts the full CSON grammar: comments, bare keys, `=`
    // as well as `:`, both quote styles, trailing commas, verbatim
    // `|` strings -- but from_str() only cares about the resulting
    // value, so any comments in this input are silently dropped. If
    // you need them to survive, see 03_edit_preserving_comments.rs.
    let source = r#"
        # This comment does not survive from_str -- it's gone by the
        # time `cfg` exists, because Config has no field to put it in.
        name = "my-service"
        server: {
            host: "localhost"
            port: 8080
        }
        tags: ["web", "prod"]
        retries: 3
    "#;

    let cfg: Config = cson_edit::from_str(source)?;
    println!("Parsed: {cfg:#?}");

    // The reverse: serialize a typed value straight to a CSON string.
    // No comments (there's nothing to take them from) and the default
    // Style (colon separator, double quotes, bare keys, 2-space indent).
    let text = cson_edit::to_string(&cfg)?;
    println!("\nSerialized back:\n{text}");

    // Round-trips cleanly through the typed layer, even though the
    // *text* changed shape (bare `=` became quoted `:`, for instance).
    let cfg2: Config = cson_edit::from_str(&text)?;
    assert_eq!(cfg, cfg2);
    println!("Round-trip OK.");

    // The byte-oriented siblings work the same way, for when your data
    // is already bytes rather than a `&str`:
    let bytes = cson_edit::to_vec(&cfg)?;
    let cfg3: Config = cson_edit::from_slice(&bytes)?;
    assert_eq!(cfg, cfg3);

    Ok(())
}
