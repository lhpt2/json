use super::*;
use super::style;
use alloc::string::ToString;

fn value_shape(node: &Node<'_>) -> alloc::string::String {
    fn go(node: &Node<'_>, out: &mut alloc::string::String) {
        match &node.value {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(n.as_str()),
            Value::Str(s) => {
                out.push('"');
                out.push_str(s.as_str());
                out.push('"');
            }
            Value::Array { items, .. } => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    go(item, out);
                }
                out.push(']');
            }
            Value::Object { entries, .. } => {
                out.push('{');
                for (i, e) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    go(&e.key, out);
                    out.push(':');
                    go(&e.value, out);
                }
                out.push('}');
            }
        }
    }
    let mut s = alloc::string::String::new();
    go(node, &mut s);
    s
}

fn all_comments<'a>(doc: &Document<'a>) -> Vec<alloc::string::String> {
    fn collect_from_prefix(prefix: &str, out: &mut Vec<alloc::string::String>) {
        for line in prefix.lines() {
            let t = line.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    fn go(node: &Node<'_>, out: &mut Vec<alloc::string::String>) {
        collect_from_prefix(&node.prefix, out);
        match &node.value {
            Value::Array { items, trailing } => {
                for item in items {
                    go(item, out);
                }
                collect_from_prefix(trailing, out);
            }
            Value::Object { entries, trailing } => {
                for e in entries {
                    go(&e.key, out);
                    go(&e.value, out);
                }
                collect_from_prefix(trailing, out);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    go(&doc.root, &mut out);
    collect_from_prefix(&doc.suffix, &mut out);
    out.sort();
    out
}

type EffectiveStyle = (
    style::Separator,
    style::Quote,
    style::KeyStyle,
    bool,
    bool,
    style::Indent,
    bool,
);

fn effective(style: &Style) -> EffectiveStyle {
    (
        style.separator(),
        style.value_quote(),
        style.key_style(),
        style.commas(),
        style.trailing_comma(),
        style.indent(),
        style.verbatim_strings(),
    )
}

fn assert_invariants(src: &str) {
    let doc1 = parse(src).expect("parse 1");
    let a = doc1.to_cson_string();
    let doc2 = parse(&a).expect("parse a");
    let b = doc2.to_cson_string();

    // 1. Fixpoint from the first run on.
    assert_eq!(a, b, "not a fixpoint for input:\n{}\n---a---\n{}\n---b---\n{}", src, a, b);

    // 2. Comments preserved (as a set -- the first run may relocate them).
    let doc0 = parse(src).expect("parse 0");
    assert_eq!(all_comments(&doc2), all_comments(&doc0), "comments lost for input:\n{}", src);

    // 3. Values unchanged.
    assert_eq!(value_shape(doc2.root()), value_shape(doc0.root()), "value changed for input:\n{}", src);

    // 4. Style fixpoint: re-parsing the output must detect the same style
    // the writer used to produce it. Compared via the effective getters
    // (defaults applied), not raw `Option` equality: a dimension the
    // original source never exercised (`None`) legitimately becomes
    // `Some(default)` once the writer has to commit to *something* to
    // render it -- that's not drift, it's the first observation.
    assert_eq!(effective(doc2.style()), effective(doc1.style()), "style drifted for input:\n{}", src);
}

#[test]
fn empty_document() {
    assert_invariants("");
    assert_invariants("   \n\n  ");
}

#[test]
fn only_comments() {
    assert_invariants("# just a comment\n");
    assert_invariants("# one\n\n# two\n");
}

#[test]
fn braced_object_basic() {
    assert_invariants(r#"{"a": 1, "b": 2}"#);
}

#[test]
fn bare_root_object() {
    assert_invariants("a: 1\nb: 2\n");
}

#[test]
fn bare_root_never_for_arrays() {
    assert!(parse("1, 2, 3").is_err());
}

#[test]
fn nested_structures() {
    assert_invariants(r#"{"a": [1, 2, {"c": true}], "b": null}"#);
}

#[test]
fn comment_before_value() {
    assert_invariants("{\n  # leading comment\n  \"a\": 1\n}\n");
}

#[test]
fn comment_after_last_value_before_close() {
    assert_invariants("{\n  \"a\": 1\n  # trailing\n}\n");
}

#[test]
fn trailing_comment_moves_to_line_start() {
    let src = "{\n  \"a\": 1, # inline note\n  \"b\": 2\n}\n";
    let doc = parse(src).unwrap();
    let comments = all_comments(&doc);
    assert_eq!(comments, alloc::vec!["# inline note".to_string()]);
    assert_invariants(src);
}

#[test]
fn closer_line_comment_anchors_to_opener_line() {
    let src = "things: [\n  1,\n  2\n]  # die Liste\n";
    assert_invariants(src);
    let doc = parse(src).unwrap();
    // the comment must end up as the prefix of the `things` key, not
    // inside the array.
    if let Value::Object { entries, .. } = &doc.root().value {
        assert!(entries[0].key().prefix().contains("die Liste"));
    } else {
        panic!("expected object root");
    }
}

#[test]
fn multiple_closers_same_line_anchor_to_outer_opener() {
    let src = "a: {\n  b: [\n    1\n  ]\n}  # x\n";
    assert_invariants(src);
    let doc = parse(src).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        assert!(entries[0].key().prefix().contains("# x"));
    } else {
        panic!("expected object root");
    }
}

#[test]
fn blank_lines_between_comments_preserved() {
    let src = "{\n  # first\n\n  # second\n  \"a\": 1\n}\n";
    assert_invariants(src);
}

#[test]
fn empty_array_with_comment() {
    assert_invariants("{\"a\": [\n  # empty but noted\n]}\n");
}

#[test]
fn empty_object_with_comment() {
    assert_invariants("{\n  # nothing here yet\n}\n");
}

#[test]
fn mixed_separators_and_quotes() {
    assert_invariants("'a' = 1, \"b\": 2\n");
}

#[test]
fn bare_keys() {
    assert_invariants("$type = 1\n-foo = 2\n");
}

#[test]
fn bare_string_rejected_as_value() {
    assert!(parse("a: foo").is_err());
}

#[test]
fn trailing_comma_object_and_array() {
    assert_invariants("{\"a\": 1, \"b\": 2,}\n");
    assert_invariants("[1, 2, 3,]\n");
}

#[test]
fn comma_omitted_across_newline() {
    assert_invariants("[\n  1\n  2\n  3\n]\n");
}

#[test]
fn comma_omitted_over_comment() {
    let src = "[\n  1\n  # between\n  2\n]\n";
    assert_invariants(src);
}

#[test]
fn verbatim_string_single_fragment() {
    assert_invariants("a = |hello world\n");
}

#[test]
fn verbatim_string_merges_consecutive_fragments() {
    let src = "a = |hello\n    |world\n";
    let doc = parse(src).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        match &entries[0].value().value {
            Value::Str(s) => assert_eq!(s.as_str(), "hello\nworld"),
            _ => panic!("expected string"),
        }
    } else {
        panic!("expected object root");
    }
    assert_invariants(src);
}

#[test]
fn verbatim_fragments_separated_by_comma_stay_two_values() {
    let src = "[|a\n,|b\n]\n";
    let doc = parse(src).unwrap();
    if let Value::Array { items, .. } = &doc.root().value {
        assert_eq!(items.len(), 2);
    } else {
        panic!("expected array root");
    }
    assert_invariants(src);
}

#[test]
fn verbatim_fragments_separated_by_blank_line_stay_two_values() {
    let src = "[\n  |a\n\n  |b\n]\n";
    let doc = parse(src).unwrap();
    if let Value::Array { items, .. } = &doc.root().value {
        assert_eq!(items.len(), 2);
    } else {
        panic!("expected array root");
    }
    assert_invariants(src);
}

#[test]
fn comment_between_verbatim_fragments_is_not_lost() {
    let src = "a = |hello\n  # note\n  |world\n";
    let doc = parse(src).unwrap();
    assert!(all_comments(&doc).iter().any(|c| c == "# note"));
    assert_invariants(src);
}

#[test]
fn hash_inside_verbatim_fragment_is_not_a_comment() {
    let src = "a = |not # a comment\n";
    let doc = parse(src).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        match &entries[0].value().value {
            Value::Str(s) => assert_eq!(s.as_str(), "not # a comment"),
            _ => panic!("expected string"),
        }
    } else {
        panic!("expected object root");
    }
    assert!(all_comments(&doc).is_empty());
}

#[test]
fn hash_inside_string_literal_is_not_a_comment() {
    let doc = parse("{\"a\": \"x # not a comment\"}").unwrap();
    assert!(all_comments(&doc).is_empty());
}

#[test]
fn crlf_and_lf_treated_as_one_newline() {
    assert_invariants("{\r\n  \"a\": 1,\r\n  \"b\": 2\r\n}\r\n");
    assert_invariants("{\n  \"a\": 1,\r\n  \"b\": 2\n}\n");
}

#[test]
fn big_integers_survive() {
    let src = "{\"a\": 9007199254740993}\n";
    let doc = parse(src).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        match &entries[0].value().value {
            Value::Number(n) => assert_eq!(n.as_str(), "9007199254740993"),
            _ => panic!("expected number"),
        }
    } else {
        panic!("expected object root");
    }
    assert_invariants(src);
}

#[test]
fn numeric_forms() {
    assert_invariants("{\"a\": 1.50, \"b\": 1e3, \"c\": -0}\n");
}

#[test]
fn unicode_escape_equals_literal_char() {
    let doc = parse("{\"a\": \"\\u0041\"}").unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        match &entries[0].value().value {
            Value::Str(s) => assert_eq!(s.as_str(), "A"),
            _ => panic!("expected string"),
        }
    } else {
        panic!("expected object root");
    }
}

#[test]
fn bom_is_stripped() {
    assert_invariants("\u{FEFF}{\"a\": 1}\n");
}

#[test]
fn number_equality_ignores_literal_formatting() {
    let a = Number { raw: "1.50".into() };
    let b = Number { raw: "1.5".into() };
    assert!(a.numeric_eq(&b));
}

#[test]
fn leading_zero_followed_by_digit_is_rejected() {
    assert!(parse("{\"a\": 01}").is_err());
}

#[test]
fn apostrophe_escape_works_in_both_quote_styles() {
    let doc = parse(r#"{"a": "it\'s", "b": 'it\'s'}"#).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        for e in entries {
            match &e.value().value {
                Value::Str(s) => assert_eq!(s.as_str(), "it's"),
                _ => panic!("expected string"),
            }
        }
    } else {
        panic!("expected object root");
    }
}

#[test]
fn double_quoted_strings_handle_surrogate_pairs() {
    // \u escape + surrogate pair, decoded via this crate's own unescape()
    // (see lexer.rs's doc comment on why it's self-contained rather than
    // borrowing serde_json's Read/StrRead, now that this is a standalone
    // crate rather than a module inside that fork).
    let doc = parse(r#"{"a": "😀"}"#).unwrap();
    if let Value::Object { entries, .. } = &doc.root().value {
        match &entries[0].value().value {
            Value::Str(s) => assert_eq!(s.as_str(), "\u{1f600}"),
            _ => panic!("expected string"),
        }
    } else {
        panic!("expected object root");
    }
}

#[test]
fn object_get_finds_first_of_duplicate_keys() {
    let doc = parse(r#"{a: 1, b: 2, a: 3}"#).unwrap();
    let root = doc.root().value();
    match root.get("a").unwrap().value() {
        Value::Number(n) => assert_eq!(n.as_str(), "1"),
        _ => panic!("expected number"),
    }
    assert!(root.get("nope").is_none());
    assert!(root.get_index(0).is_none()); // not an array
}

#[test]
fn object_insert_appends_without_deduping() {
    let mut doc = parse(r#"{a: 1}"#).unwrap();
    doc.root_mut()
        .value_mut()
        .insert("b", Node::new(Value::from_serialize(&2i64).unwrap()))
        .unwrap();
    doc.root_mut()
        .value_mut()
        .insert("a", Node::new(Value::from_serialize(&99i64).unwrap()))
        .unwrap();
    if let Value::Object { entries, .. } = doc.root().value() {
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key_str(), Some("a"));
        assert_eq!(entries[1].key_str(), Some("b"));
        assert_eq!(entries[2].key_str(), Some("a"));
    } else {
        panic!("expected object");
    }
    assert_eq!(doc.to_cson_string(), "{\n  a: 1,\n  b: 2,\n  a: 99\n}\n");
}

#[test]
fn insert_and_push_reject_the_wrong_variant() {
    let mut arr = Value::Array { items: alloc::vec::Vec::new(), trailing: Cow::Borrowed("") };
    assert!(arr.insert("x", Node::new(Value::Null)).is_err());
    let mut obj = Value::Object { entries: alloc::vec::Vec::new(), trailing: Cow::Borrowed("") };
    assert!(obj.push(Node::new(Value::Null)).is_err());
}

#[test]
fn object_remove_middle_entry_moves_its_comments_onto_the_next_key() {
    let mut doc = parse("a: 1\n# about b\nb: 2\nc: 3\n").unwrap();
    let removed = doc.root_mut().value_mut().remove("b").unwrap();
    assert_eq!(removed.prefix(), ""); // relocated, not duplicated
    let text = doc.to_cson_string();
    assert!(text.contains("# about b"));
    assert!(!text.contains("b: 2"));
    // moved onto c's prefix, ahead of c itself
    let reparsed = parse(&text).unwrap();
    if let Value::Object { entries, .. } = reparsed.root().value() {
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].key_str(), Some("c"));
        assert!(entries[1].key().prefix().contains("# about b"));
    } else {
        panic!("expected object");
    }
}

#[test]
fn object_remove_last_entry_moves_its_comments_into_trailing() {
    let mut doc = parse("{\n  a: 1\n  # about b\n  b: 2\n}\n").unwrap();
    doc.root_mut().value_mut().remove("b").unwrap();
    let text = doc.to_cson_string();
    let reparsed = parse(&text).unwrap();
    assert!(text.contains("# about b"));
    if let Value::Object { entries, trailing } = reparsed.root().value() {
        assert_eq!(entries.len(), 1);
        assert!(trailing.contains("# about b"));
    } else {
        panic!("expected object");
    }
}

#[test]
fn object_remove_missing_key_is_a_no_op() {
    let mut doc = parse(r#"{a: 1}"#).unwrap();
    assert!(doc.root_mut().value_mut().remove("nope").is_none());
}

#[test]
fn array_remove_index_moves_its_comment_onto_the_next_element() {
    let mut doc = parse("[\n  1\n  # about two\n  2\n  3\n]\n").unwrap();
    let removed = doc.root_mut().value_mut().remove_index(1).unwrap();
    assert_eq!(removed.prefix(), "");
    let text = doc.to_cson_string();
    let reparsed = parse(&text).unwrap();
    if let Value::Array { items, .. } = reparsed.root().value() {
        assert_eq!(items.len(), 2);
        assert!(items[1].prefix().contains("# about two"));
    } else {
        panic!("expected array");
    }
}

#[test]
fn array_remove_last_index_moves_its_comment_into_trailing() {
    let mut doc = parse("[\n  1\n  # about two\n  2\n]\n").unwrap();
    doc.root_mut().value_mut().remove_index(1).unwrap();
    let text = doc.to_cson_string();
    let reparsed = parse(&text).unwrap();
    if let Value::Array { items, trailing } = reparsed.root().value() {
        assert_eq!(items.len(), 1);
        assert!(trailing.contains("# about two"));
    } else {
        panic!("expected array");
    }
}

#[test]
fn array_push_and_get_index() {
    let mut doc = parse(r#"[1, 2]"#).unwrap();
    doc.root_mut()
        .value_mut()
        .push(Node::new(Value::from_serialize(&3i64).unwrap()))
        .unwrap();
    let root = doc.root().value();
    assert_eq!(root.get_index(2).unwrap().value(), &Value::from_serialize(&3i64).unwrap());
    assert!(root.get_index(3).is_none());
}
