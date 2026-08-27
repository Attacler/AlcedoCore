//! Field-level diff computation for collection item updates.
//!
//! Provides [`compute_field_diffs`] — a pure function that compares two JSONB
//! values (old vs. new) and returns a structured JSON object capturing which
//! fields changed and their old/new values.
//!
//! # Diff format
//! Returns a JSON object keyed by field name:
//! ```json
//! { "field_name": { "old": <old_value>, "new": <new_value> } }
//! ```
//!
//! # Edge cases handled
//! - **Identical values**: skipped (no entry produced).
//! - **Null values**: null→"value" and "value"→null both produce entries.
//! - **Added/removed keys**: treated as null on the missing side.
//! - **Type coercion**: string `"42"` vs int `42` → treated as different.
//! - **JSONB reordering**: structurally equal objects → no entry.
//! - **Nested objects**: compared atomically (no recursive path flattening).
//! - **Arrays**: compared atomically via Eq.
//! - **Empty objects**: `{}` vs `{}` → no entry.
//! - **Non-object top-level**: returns empty `{}`.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Computes field-level diffs between two JSON object values.
///
/// Compares top-level keys between `old` and `new` JSON objects.
/// For each key that differs, produces an entry of the form:
/// `{ "field_name": { "old": <value>, "new": <value> } }`.
///
/// # Behavior by edge case
/// - **Identical values**: skipped (no entry produced).
/// - **Null values**: null→"value" and "value"→null both produce entries.
///   null→null produces none.
/// - **Added/removed keys**: treated as null on the missing side.
/// - **Type coercion**: string "42" vs int 42 → treated as different (produces entry).
/// - **JSONB reordering**: `{"a":1,"b":2}` vs `{"b":2,"a":1}` → treated as equal
///   (no entry). serde_json::Value's Eq implementation compares by content,
///   not by insertion order.
/// - **Nested objects**: compared atomically. If any nested value differs,
///   the full parent object is reported as old and new.
/// - **Arrays**: compared atomically via Eq.
/// - **Empty objects** ({} vs {}): no entry.
/// - **Non-object top-level** (Value::Null, etc.): returns empty `{}`.
pub fn compute_field_diffs(old: &Value, new: &Value) -> Value {
    let old_obj = match old {
        Value::Object(map) => map,
        _ => return Value::Object(Map::new()),
    };
    let new_obj = match new {
        Value::Object(map) => map,
        _ => return Value::Object(Map::new()),
    };

    // Collect all unique keys from both objects deterministically
    let keys: BTreeSet<&String> = old_obj.keys().chain(new_obj.keys()).collect();

    let mut result = Map::new();

    for key in keys {
        let old_val = old_obj.get(key).unwrap_or(&Value::Null);
        let new_val = new_obj.get(key).unwrap_or(&Value::Null);

        if old_val == new_val {
            continue;
        }

        let mut entry = Map::new();
        entry.insert("old".to_string(), old_val.clone());
        entry.insert("new".to_string(), new_val.clone());
        result.insert(key.clone(), Value::Object(entry));
    }

    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    fn check(old: Value, new: Value, expected: Value) {
        let result = compute_field_diffs(&old, &new);
        assert_eq!(
            result, expected,
            "\nold:      {}\nnew:      {}\nexpected: {}\ngot:      {}",
            old, new, expected, result
        );
    }

    #[rstest]
    #[case::same_object(json!({"a": 1}), json!({"a": 1}), json!({}))]
    #[case::both_null(json!({"field": null}), json!({"field": null}), json!({}))]
    #[case::jsonb_reordering(json!({"a": 1, "b": 2}), json!({"b": 2, "a": 1}), json!({}))]
    #[case::nested_identical(json!({"addr": {"city": "NYC"}}), json!({"addr": {"city": "NYC"}}), json!({}))]
    #[case::empty_objects(json!({}), json!({}), json!({}))]
    #[case::array_identical(json!({"tags": [1, 2]}), json!({"tags": [1, 2]}), json!({}))]
    #[case::non_object_top_level(json!(null), json!(null), json!({}))]
    #[case::non_object_vs_object(json!(null), json!({"a": 1}), json!({}))]
    #[case::deeply_nested_identical(json!({"a": {"b": {"c": 1}}}), json!({"a": {"b": {"c": 1}}}), json!({}))]
    fn test_identical(#[case] old: Value, #[case] new: Value, #[case] expected: Value) {
        check(old, new, expected);
    }

    #[rstest]
    #[case::different_scalar(
        json!({"title": "Old"}), json!({"title": "New"}),
        json!({"title": {"old": "Old", "new": "New"}})
    )]
    #[case::null_to_value(
        json!({"field": null}), json!({"field": "val"}),
        json!({"field": {"old": null, "new": "val"}})
    )]
    #[case::value_to_null(
        json!({"field": "val"}), json!({"field": null}),
        json!({"field": {"old": "val", "new": null}})
    )]
    #[case::added_key(
        json!({}), json!({"new_key": "val"}),
        json!({"new_key": {"old": null, "new": "val"}})
    )]
    #[case::removed_key(
        json!({"old_key": "val"}), json!({}),
        json!({"old_key": {"old": "val", "new": null}})
    )]
    #[case::type_coercion(
        json!({"n": "42"}), json!({"n": 42}),
        json!({"n": {"old": "42", "new": 42}})
    )]
    #[case::nested_changed(
        json!({"addr": {"city": "NYC"}}), json!({"addr": {"city": "LA"}}),
        json!({"addr": {"old": {"city": "NYC"}, "new": {"city": "LA"}}})
    )]
    #[case::empty_vs_non_empty(
        json!({}), json!({"key": "val"}),
        json!({"key": {"old": null, "new": "val"}})
    )]
    #[case::array_different(
        json!({"tags": [1]}), json!({"tags": [1, 2]}),
        json!({"tags": {"old": [1], "new": [1, 2]}})
    )]
    #[case::multiple_fields(
        json!({"a": 1, "b": "old", "c": true}), json!({"a": 2, "b": "old", "c": false}),
        json!({"a": {"old": 1, "new": 2}, "c": {"old": true, "new": false}})
    )]
    #[case::bool_values(
        json!({"flag": true}), json!({"flag": false}),
        json!({"flag": {"old": true, "new": false}})
    )]
    #[case::deeply_nested_different(
        json!({"a": {"b": {"c": 1}}}), json!({"a": {"b": {"c": 2}}}),
        json!({"a": {"old": {"b": {"c": 1}}, "new": {"b": {"c": 2}}}})
    )]
    fn test_changed(#[case] old: Value, #[case] new: Value, #[case] expected: Value) {
        check(old, new, expected);
    }
}
