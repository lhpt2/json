//! Schicht 2 (`impl Deserializer for &Node`) and Schicht 3
//! (`impl Serializer with Ok = Node`) round-trip tests.

use super::*;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;
use serde::{Deserialize, Serialize};

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

#[test]
fn top_level_from_str_and_to_string_round_trip() {
    let cfg = Config {
        name: "svc".to_string(),
        server: Address { host: "localhost".to_string(), port: 8080 },
        tags: vec!["a".to_string()],
        retries: Some(1),
    };
    let text = crate::to_string(&cfg).unwrap();
    let cfg2: Config = crate::from_str(&text).unwrap();
    assert_eq!(cfg, cfg2);
}

#[test]
fn top_level_from_str_sees_comments_but_the_struct_does_not_need_to_care() {
    let cfg: Config = crate::from_str(
        r#"
        # top-level config
        name = "svc"
        server: { host: "localhost", port: 8080 }
        tags: []
        retries: null
        "#,
    )
    .unwrap();
    assert_eq!(cfg.name, "svc");
}

#[test]
fn top_level_from_slice_and_to_vec_round_trip() {
    let cfg = Address { host: "localhost".to_string(), port: 8080 };
    let bytes = crate::to_vec(&cfg).unwrap();
    let cfg2: Address = crate::from_slice(&bytes).unwrap();
    assert_eq!(cfg, cfg2);
}

#[cfg(feature = "std")]
#[test]
fn top_level_from_reader_and_to_writer_round_trip() {
    let cfg = Address { host: "localhost".to_string(), port: 8080 };
    let mut buf: Vec<u8> = Vec::new();
    crate::to_writer(&mut buf, &cfg).unwrap();
    let cfg2: Address = crate::from_reader(&buf[..]).unwrap();
    assert_eq!(cfg, cfg2);
}

// ---------------------------------------------------------------------
// merge_from (see lib.rs's Document::merge_from doc comment)
// ---------------------------------------------------------------------

#[test]
fn merge_from_with_an_unchanged_value_changes_nothing() {
    // The invariant that catches most merge bugs: parse, merge back an
    // untouched struct, write -- must equal writing without merging at
    // all, down to the byte.
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
    let baseline = parse(src).unwrap().to_cson_string();

    let mut doc = parse(src).unwrap();
    let cfg: Config = doc.deserialize().unwrap();
    doc.merge_from(&cfg).unwrap();

    assert_eq!(doc.to_cson_string(), baseline);
}

#[test]
fn merge_from_changes_one_scalar_and_keeps_every_comment() {
    let src = "# header\nname = \"svc\"\nserver: {\n  host: \"localhost\"\n  # ops override\n  port: 8080\n}\ntags: []\nretries: null\n";
    let mut doc = parse(src).unwrap();
    let mut cfg: Config = doc.deserialize().unwrap();

    cfg.server.port = 9090;
    doc.merge_from(&cfg).unwrap();

    let text = doc.to_cson_string();
    assert!(text.contains("9090"));
    assert!(!text.contains("8080"));
    assert!(text.contains("# header"));
    assert!(text.contains("# ops override"));
    // untouched siblings keep their exact text
    assert!(text.contains("\"localhost\""));
}

#[test]
fn merge_from_keeps_the_raw_literal_when_a_number_is_numerically_equal() {
    #[derive(Serialize, Deserialize)]
    struct Ratios {
        a: f64,
        b: f64,
    }
    // 1.50 parses to 1.5; re-serializing the struct would format it as
    // "1.5", so a textual comparison would rewrite it on every save.
    let mut doc = parse("a: 1.50\nb: 2.0\n").unwrap();
    let cfg: Ratios = doc.deserialize().unwrap();
    doc.merge_from(&cfg).unwrap();

    let text = doc.to_cson_string();
    assert!(text.contains("1.50"), "raw literal should survive: {text}");
    assert!(text.contains("2.0"));
}

#[test]
fn merge_from_adds_new_keys_and_removes_absent_ones_preserving_comments() {
    #[derive(Serialize, Deserialize)]
    struct Before {
        keep: i32,
        // about to disappear from the type
        drop_me: i32,
    }
    #[derive(Serialize, Deserialize)]
    struct After {
        keep: i32,
        added: i32,
    }

    let mut doc = parse("keep: 1\n# about drop_me\ndrop_me: 2\n").unwrap();
    let _before: Before = doc.deserialize().unwrap();

    doc.merge_from(&After { keep: 1, added: 3 }).unwrap();

    let text = doc.to_cson_string();
    assert!(text.contains("keep: 1"));
    assert!(text.contains("added: 3"));
    // the removed key's comment migrated rather than vanishing (note it
    // mentions "drop_me", so check the *structure* for the key's absence)
    assert!(text.contains("# about drop_me"));
    let reparsed = parse(&text).unwrap();
    let keys: Vec<&str> = match reparsed.root().value() {
        Value::Object { entries, .. } => entries.iter().filter_map(|e| e.key_str()).collect(),
        _ => panic!("expected object"),
    };
    assert_eq!(keys, ["keep", "added"]);
}

#[test]
fn merge_from_grows_and_shrinks_arrays_positionally() {
    #[derive(Serialize, Deserialize)]
    struct Holder {
        items: Vec<String>,
    }

    let mut doc = parse("items: [\n  \"a\"\n  # about b\n  \"b\"\n]\n").unwrap();

    // grow
    doc.merge_from(&Holder {
        items: vec!["a".to_string(), "b".to_string(), "c".to_string()],
    })
    .unwrap();
    let text = doc.to_cson_string();
    assert!(text.contains("\"c\""));
    assert!(text.contains("# about b"));

    // shrink back past the commented element
    doc.merge_from(&Holder { items: vec!["a".to_string()] }).unwrap();
    let text = doc.to_cson_string();
    assert!(text.contains("\"a\""));
    assert!(!text.contains("\"b\""));
    assert!(!text.contains("\"c\""));
    // the dropped element's comment ended up in the array's trailing slot
    assert!(text.contains("# about b"));
    parse(&text).unwrap();
}

#[test]
fn merge_from_replaces_a_node_whose_shape_changed() {
    #[derive(Serialize, Deserialize)]
    struct Scalar {
        thing: i32,
    }
    #[derive(Serialize, Deserialize)]
    struct Nested {
        thing: Address,
    }

    let mut doc = parse("# about thing\nthing: 1\n").unwrap();
    let _: Scalar = doc.deserialize().unwrap();

    doc.merge_from(&Nested { thing: Address { host: "h".to_string(), port: 1 } }).unwrap();

    let text = doc.to_cson_string();
    assert!(text.contains("host"));
    assert!(text.contains("# about thing")); // the Node, and so its prefix, survived
    parse(&text).unwrap();
}

#[test]
fn merge_from_result_stays_a_fixpoint_and_reparses_to_the_merged_value() {
    let src = "# header\nname = \"svc\"\nserver: { host: \"h\", port: 1 }\ntags: [\"x\"]\nretries: 2\n";
    let mut doc = parse(src).unwrap();
    let mut cfg: Config = doc.deserialize().unwrap();
    cfg.tags.push("y".to_string());
    cfg.retries = None;
    doc.merge_from(&cfg).unwrap();

    let a = doc.to_cson_string();
    let b = parse(&a).unwrap().to_cson_string();
    assert_eq!(a, b, "invariant 1: fixpoint from the first write on");

    let reparsed: Config = parse(&a).unwrap().deserialize().unwrap();
    assert_eq!(reparsed, cfg);
}
