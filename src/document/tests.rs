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
