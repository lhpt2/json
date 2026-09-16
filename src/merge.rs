//! `Document::merge_from`: diffing a freshly `Serialize`d tree against
//! an existing, comment-carrying one, so a typed edit only rewrites the
//! values that actually changed.
//!
//! This is the piece Schicht 2 + Schicht 3 don't add up to on their own.
//! `doc.deserialize::<T>()` reads a document into `T`; `to_node(&t)`
//! builds a tree back out of it -- but that tree is brand new and has no
//! comments, so substituting it for `doc`'s root would silently throw
//! every comment away. The merge below is what makes the typed
//! round trip (`deserialize` -> mutate `T` -> `merge_from`) safe: it
//! walks the two trees together and edits `old` in place, so any node
//! whose value didn't change is never touched at all -- keeping not
//! just its comment (which lives in the `Node` around it, and is never
//! reachable from here) but also its exact raw number literal.
//!
//! Deletions -- an object key or array element present in the document
//! but not in the fresh tree -- go through [`Value::remove`]/
//! [`Value::remove_index`], so a removed node's comments migrate onto
//! whatever takes its place instead of vanishing, exactly as they would
//! for a hand-written deletion.

use crate::{CsonStr, Entry, Node, Value};
use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Merges the comment-free `new` tree into the comment-carrying `old`
/// one, in place. See the module doc comment and
/// [`crate::Document::merge_from`].
pub(crate) fn merge_value<'a>(old: &mut Value<'a>, new: Value<'static>) {
    match new {
        Value::Null => {
            if !matches!(old, Value::Null) {
                *old = Value::Null;
            }
        }
        Value::Bool(b) => match old {
            // Unchanged: leave `old` completely alone.
            Value::Bool(a) if *a == b => {}
            _ => *old = Value::Bool(b),
        },
        Value::Number(b) => match old {
            // Numerically, not textually: `1.50` in the file and `1.5`
            // out of the struct are the same value, and comparing the
            // raw literals instead would rewrite every number in the
            // document on every save.
            Value::Number(a) if a.numeric_eq(&b) => {}
            _ => *old = Value::Number(b),
        },
        Value::Str(b) => match old {
            Value::Str(a) if a.as_str() == b.as_str() => {}
            _ => *old = Value::Str(b),
        },
        Value::Array { items: new_items, .. } => match old {
            Value::Array { .. } => merge_array(old, new_items),
            // Shape change (old was a scalar/object, new is an array):
            // there's no field-by-field diff to do, so replace it.
            _ => *old = Value::Array { items: new_items, trailing: Cow::Borrowed("") },
        },
        Value::Object { entries: new_entries, .. } => match old {
            Value::Object { .. } => merge_object(old, new_entries),
            _ => *old = Value::Object { entries: new_entries, trailing: Cow::Borrowed("") },
        },
    }
}

/// Position-by-position up to the shorter length, then append or trim.
fn merge_array<'a>(old: &mut Value<'a>, new_items: Vec<Node<'static>>) {
    let new_len = new_items.len();
    let old_len = match old {
        Value::Array { items, .. } => items.len(),
        _ => unreachable!("merge_array called on a non-array"),
    };
    let common = old_len.min(new_len);

    let mut new_iter = new_items.into_iter();
    if let Value::Array { items: old_items, .. } = old {
        for slot in old_items.iter_mut().take(common) {
            let new_item = new_iter.next().expect("common <= new_len");
            merge_value(slot.value_mut(), new_item.into_value());
        }
    }

    if new_len > old_len {
        if let Value::Array { items: old_items, .. } = old {
            old_items.extend(new_iter);
        }
    } else {
        // Trim from the same index repeatedly rather than truncating:
        // remove_index is what carries each dropped element's comment
        // forward, ending up in the array's `trailing` slot.
        for _ in 0..(old_len - new_len) {
            old.remove_index(common);
        }
    }
}

/// Keys in both are merged in place (keeping `old`'s order), keys only
/// in `new` are appended, keys only in `old` are removed.
fn merge_object<'a>(old: &mut Value<'a>, new_entries: Vec<Entry<'static>>) {
    let mut seen_keys: Vec<Cow<'static, str>> = Vec::with_capacity(new_entries.len());

    for new_entry in new_entries {
        let key = match new_entry.key.into_value() {
            Value::Str(s) => s.value,
            // Unreachable for a tree built by this crate's Serializer,
            // which only ever emits string keys; skipping is still the
            // right answer for a hand-built one.
            _ => continue,
        };

        let existing = match old {
            Value::Object { entries, .. } => {
                entries.iter_mut().find(|e| e.key_str() == Some(&*key))
            }
            _ => None,
        };

        if let Some(existing) = existing {
            merge_value(existing.value_mut().value_mut(), new_entry.value.into_value());
        } else if let Value::Object { entries, .. } = old {
            entries.push(Entry {
                key: Node::new(Value::Str(CsonStr::new(key.clone()))),
                value: new_entry.value,
            });
        }

        seen_keys.push(key);
    }

    let stale: Vec<String> = match old {
        Value::Object { entries, .. } => entries
            .iter()
            .filter_map(|e| e.key_str())
            .filter(|k| !seen_keys.iter().any(|seen| seen.as_ref() == *k))
            .map(|k| k.to_string())
            .collect(),
        _ => Vec::new(),
    };

    for key in stale {
        old.remove(&key);
    }
}
