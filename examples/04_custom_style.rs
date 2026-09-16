//! `Style` controls every structural choice the writer makes --
//! separator, quote character, commas, indentation -- applied
//! uniformly across the whole document, never per node. By default a
//! parsed document's `Style` is whatever it first-match-detected from
//! the source; this example overrides it explicitly.
//!
//! Run with: `cargo run --example 04_custom_style`

use cson_edit::{parse, Indent, IndentChar, KeyStyle, Quote, Separator, Style};

fn main() -> Result<(), cson_edit::ParseError> {
    let source = r#"name: "svc", server: { host: "localhost", port: 8080 }"#;

    let mut doc = parse(source)?;

    // Detected from the source: `:` was the first separator seen, `"`
    // the first quote character, keys were bare (no key in this source
    // needed quoting), commas were used explicitly.
    let detected = doc.style();
    println!(
        "detected: separator={:?} quote={:?} key_style={:?} commas={:?}",
        detected.separator(),
        detected.value_quote(),
        detected.key_style(),
        detected.commas(),
    );
    println!("as detected:\n{}\n", doc.to_cson_string());

    // Override it: '=' instead of ':', single quotes, 4-space indent,
    // and quote keys instead of leaving them bare.
    doc.set_style(Style {
        separator: Some(Separator::Equals),
        value_quote: Some(Quote::Single),
        key_style: Some(KeyStyle::Double),
        indent: Some(Indent { ch: IndentChar::Space, width: 4 }),
        ..Style::default()
    });
    println!("with a custom Style:\n{}", doc.to_cson_string());

    Ok(())
}
