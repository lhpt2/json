//! Schicht 2 (`impl Deserializer for &Node`) and Schicht 3
//! (`impl Serializer with Ok = Node`) round-trip tests.

use super::*;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;

// `crate::serde` is `serde_core` (see `extern crate serde_core as serde;`
// in lib.rs) -- the trait-only crate the rest of this library builds on,
// with no derive macros. These tests want the real, full `serde` dev
// dependency (with the "derive" feature) instead, so it needs its own,
// non-conflicting local name.
extern crate serde as serde_full;
use serde_full::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Address {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    server: Address,
    tags: Vec<String>,
    retries: Option<u32>,
}

#[test]
fn deserialize_struct_from_parsed_document_with_comments() {
    let src = r#"
        # top-level config
        name = "svc"
        server: {
          # where it listens
          host: "localhost"
          port: 8080
        }
        tags: ["a", "b"]
        retries: null
    "#;
    let doc = parse(src).unwrap();
    let cfg: Config = doc.deserialize().unwrap();
    assert_eq!(
        cfg,
        Config {
            name: "svc".to_string(),
            server: Address { host: "localhost".to_string(), port: 8080 },
            tags: vec!["a".to_string(), "b".to_string()],
            retries: None,
        }
    );
}

#[test]
fn deserialize_bare_root_object() {
    let doc = parse("a: 1\nb: 2\n").unwrap();
    let map: BTreeMap<String, i64> = doc.deserialize().unwrap();
    let mut expected = BTreeMap::new();
    expected.insert("a".to_string(), 1);
    expected.insert("b".to_string(), 2);
    assert_eq!(map, expected);
}

#[test]
fn deserialize_numbers_across_types() {
    let doc = parse(r#"{"a": 9007199254740993, "b": 1.5, "c": -3}"#).unwrap();
    #[derive(Deserialize)]
    struct Numbers {
        a: u64,
        b: f64,
        c: i32,
    }
    let n: Numbers = doc.deserialize().unwrap();
    assert_eq!(n.a, 9007199254740993);
    assert_eq!(n.b, 1.5);
    assert_eq!(n.c, -3);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Shape {
    Point,
    Circle(f64),
    Rect { w: f64, h: f64 },
}

#[test]
fn enum_variants_round_trip_through_deserialize() {
    // A CSON document's top level must be an object or array (`JSON-text
    // = object / array / ws object-items`), so a bare top-level string
    // isn't valid CSON to begin with -- every variant is nested in an
    // object here, which is also the realistic shape (an enum is
    // normally a struct field, not the whole document).
    #[derive(Debug, PartialEq, Deserialize)]
    struct Holder {
        shape: Shape,
    }

    let doc = parse(r#"{"shape": "Point"}"#).unwrap();
    let h: Holder = doc.deserialize().unwrap();
    assert_eq!(h.shape, Shape::Point);

    let doc = parse(r#"{"shape": {"Circle": 2.5}}"#).unwrap();
    let h: Holder = doc.deserialize().unwrap();
    assert_eq!(h.shape, Shape::Circle(2.5));

    let doc = parse(r#"{"shape": {"Rect": {"w": 1.0, "h": 2.0}}}"#).unwrap();
    let h: Holder = doc.deserialize().unwrap();
    assert_eq!(h.shape, Shape::Rect { w: 1.0, h: 2.0 });
}

#[test]
fn to_node_produces_expected_shape() {
    let cfg = Config {
        name: "svc".to_string(),
        server: Address { host: "localhost".to_string(), port: 8080 },
        tags: vec!["a".to_string()],
        retries: Some(3),
    };
    let node = to_node(&cfg).unwrap();
    match &node.value {
        Value::Object { entries, .. } => {
            assert_eq!(entries.len(), 4);
            assert_eq!(entries[0].key_str(), Some("name"));
            match &entries[0].value().value {
                Value::Str(s) => assert_eq!(s.as_str(), "svc"),
                _ => panic!("expected string"),
            }
        }
        _ => panic!("expected object"),
    }
}

#[test]
fn from_serialize_then_write_then_reparse_round_trips_the_value() {
    let cfg = Config {
        name: "svc".to_string(),
        server: Address { host: "localhost".to_string(), port: 8080 },
        tags: vec!["a".to_string(), "b".to_string()],
        retries: Some(3),
    };
    let doc = Document::from_serialize(&cfg).unwrap();
    let text = doc.to_cson_string();

    let reparsed = parse(&text).unwrap();
    let cfg2: Config = reparsed.deserialize().unwrap();
    assert_eq!(cfg, cfg2);

    // Writing again from the reparsed document must be a fixpoint, same
    // as the plain Schicht-1 invariant.
    let text2 = reparsed.to_cson_string();
    assert_eq!(text, text2);
}

#[test]
fn from_serialize_handles_enums_options_and_maps() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrapper {
        shape: Shape,
        maybe: Option<i32>,
        nothing: Option<i32>,
        map: BTreeMap<String, i32>,
    }
    let mut map = BTreeMap::new();
    map.insert("x".to_string(), 1);
    map.insert("y".to_string(), 2);
    let w = Wrapper { shape: Shape::Rect { w: 3.0, h: 4.0 }, maybe: Some(5), nothing: None, map };

    let doc = Document::from_serialize(&w).unwrap();
    let text = doc.to_cson_string();
    let w2: Wrapper = parse(&text).unwrap().deserialize().unwrap();
    assert_eq!(w, w2);
}

#[test]
fn deserialize_rejects_wrong_shape_with_an_error() {
    let doc = parse(r#"{"a": "not a number"}"#).unwrap();
    #[derive(Deserialize)]
    struct Numeric {
        #[allow(dead_code)]
        a: i32,
    }
    let result: ParseResult<Numeric> = doc.deserialize();
    assert!(result.is_err());
}
