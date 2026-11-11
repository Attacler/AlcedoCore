use axum::{extract::FromRequestParts, http::request::Parts};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::services::errors::AlcedoError;

/// Splits a qs-style key into segments: `aggregate[count][]` becomes
/// `["aggregate", "count", ""]` — an empty segment means "append to array".
fn split_qs_key(key: &str) -> Vec<String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut chars = key.chars();
    while let Some(c) = chars.next() {
        if c == '[' {
            if !cur.is_empty() {
                segs.push(std::mem::take(&mut cur));
            }
            let mut inner = String::new();
            for c2 in chars.by_ref() {
                if c2 == ']' {
                    break;
                }
                inner.push(c2);
            }
            segs.push(inner);
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    segs
}

/// Inserts one query parameter into the JSON object, translating qs bracket
/// syntax (`groupBy[]`, `aggregate[fn]`) into nested arrays/objects.
/// Repeated bracketed keys accumulate into arrays (e.g. multiple `groupBy[]`
/// entries).
fn insert_qs_param(root: &mut Value, key: &str, value: Value) {
    let segs = split_qs_key(key);
    insert_into(root, &segs, value, key.contains('['));
}

fn insert_into(node: &mut Value, segs: &[String], value: Value, bracketed: bool) {
    let seg = &segs[0];
    if seg.is_empty() {
        // `key[]` — append to an array
        if !node.is_array() {
            *node = Value::Array(Vec::new());
        }
        let arr = node.as_array_mut().unwrap();
        if segs.len() == 1 {
            arr.push(value);
        } else {
            if arr.is_empty() {
                arr.push(Value::Null);
            }
            insert_into(arr.last_mut().unwrap(), &segs[1..], value, bracketed);
        }
        return;
    }
    if !node.is_object() {
        *node = Value::Object(serde_json::Map::new());
    }
    let obj = node.as_object_mut().unwrap();
    if segs.len() == 1 {
        if bracketed {
            match obj.get(seg) {
                Some(Value::Array(existing)) => {
                    let mut merged = existing.clone();
                    merged.push(value);
                    obj.insert(seg.clone(), Value::Array(merged));
                }
                Some(existing) => {
                    obj.insert(seg.clone(), Value::Array(vec![existing.clone(), value]));
                }
                None => {
                    obj.insert(seg.clone(), value);
                }
            }
        } else {
            obj.insert(seg.clone(), value);
        }
    } else {
        let entry = obj.entry(seg.clone()).or_insert(Value::Null);
        insert_into(entry, &segs[1..], value, bracketed);
    }
}

/// Coerces one raw query-string value into a JSON leaf: `{...}`/`[...]` are
/// parsed as JSON, scalars become numbers/bools/null, the rest stays a string.
fn json_leaf(key: &str, value: &str) -> Result<Value, AlcedoError> {
    if value.starts_with('{') || value.starts_with('[') {
        return serde_json::from_str(value).map_err(|e| {
            AlcedoError::InvalidInput(format!("Invalid JSON in parameter '{key}': {e}"), 0)
        });
    }
    Ok(if let Ok(n) = value.parse::<u64>() {
        Value::Number(n.into())
    } else if let Ok(n) = value.parse::<i64>() {
        Value::Number(n.into())
    } else if let Ok(f) = value.parse::<f64>() {
        serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string()))
    } else {
        match value.to_lowercase().as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            _ => Value::String(value.to_string()),
        }
    })
}

/// wire compat: `fields=a,b` and `sort=-a,b` arrive as
/// comma-joined strings, and v1 sends the sort direction as a separate
/// `order=asc|desc` param. Split the comma lists into arrays and fold
/// `order` into the sort entries that have no explicit `+`/`-` prefix.
fn normalize_wire_lists(root: &mut Value) {
    let obj = match root.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    for key in ["fields", "sort"] {
        if let Some(Value::String(s)) = obj.get(key) {
            let s = s.clone();
            let arr: Vec<Value> = s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| Value::String(p.to_string()))
                .collect();
            obj.insert(key.to_string(), Value::Array(arr));
        }
    }
    if let Some(Value::String(order)) = obj.get("order").cloned() {
        let desc = order.eq_ignore_ascii_case("desc");
        if let Some(Value::Array(sort)) = obj.get_mut("sort") {
            for entry in sort.iter_mut() {
                if let Value::String(s) = entry {
                    if desc && !s.starts_with('+') && !s.starts_with('-') {
                        *s = format!("-{s}");
                    }
                }
            }
        }
        obj.remove("order");
    }
}

/// Parses a raw query string into `T` by building a JSON value from the form
/// parameters — qs bracket keys become nested arrays/objects, JSON values are
/// parsed, scalars are coerced — and deserializing `T` from it.
pub fn parse_query_string<T: DeserializeOwned>(query_string: &str) -> Result<T, AlcedoError> {
    let mut root = Value::Object(serde_json::Map::new());
    for (key, value) in url::form_urlencoded::parse(query_string.as_bytes()) {
        let leaf = json_leaf(&key, &value)?;
        insert_qs_param(&mut root, &key, leaf);
    }
    normalize_wire_lists(&mut root);

    // A `filter` that is a bare field map (no `_and`/`_or`) is treated as an
    // implicit `_and`. The admin UI's relational sections send e.g.
    // `filter={"customer":{"_eq":"..."}}`.
    if let Some(Value::Object(map)) = root.get("filter") {
        if !map.is_empty() && !map.contains_key("_and") && !map.contains_key("_or") {
            let wrapped = serde_json::json!({ "_and": [Value::Object(map.clone())] });
            root.as_object_mut()
                .unwrap()
                .insert("filter".to_string(), wrapped);
        }
    }

    match serde_json::from_value::<T>(root) {
        Ok(value) => Ok(value),
        Err(de_err) => {
            let error_message = match de_err.classify() {
                serde_json::error::Category::Data => format!("Data type error: {de_err}"),
                serde_json::error::Category::Syntax => format!("JSON syntax error: {de_err}"),
                serde_json::error::Category::Eof => format!("Unexpected end of JSON: {de_err}"),
                _ => format!("Deserialization error: {de_err}"),
            };
            Err(AlcedoError::InvalidInput(error_message, 0))
        }
    }
}

pub struct CustomQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for CustomQuery<T>
where
    T: DeserializeOwned + 'static,
    S: Send + Sync,
{
    type Rejection = AlcedoError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query_string = parts.uri.query().unwrap_or("").to_string();
        parse_query_string(&query_string).map(CustomQuery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::items::query::Query;

    #[test]
    fn bracket_keys_and_json_values_parse_in_one_request() {
        let mut q: Query = parse_query_string(
            "groupBy[]=state&groupBy[]=year(created_at)&aggregate[count]=*&aggregate[sum]=amount&aggregate[sum]=total&filter={\"status\":{\"_eq\":\"open\"}}&includeCount=true&page=2",
        )
        .unwrap();
        assert_eq!(
            q.group_by,
            vec!["state".to_string(), "year(created_at)".to_string()]
        );
        q.normalize_aggregate();
        assert_eq!(q.aggregate.get("count").unwrap().0, vec!["*".to_string()]);
        assert_eq!(
            q.aggregate.get("sum").unwrap().0,
            vec!["amount".to_string(), "total".to_string()]
        );
        assert!(q.include_count);
        assert_eq!(q.page, Some(2));
    }

    #[test]
    fn bare_filter_map_is_wrapped_in_and() {
        let q: Query = parse_query_string("filter={\"status\":{\"_eq\":\"open\"}}").unwrap();
        assert!(q.filter._and.is_some());
        assert!(q.filter._or.is_none());
    }

    #[test]
    fn invalid_json_value_is_a_clean_400() {
        let err = parse_query_string::<Query>("filter={not json}").unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)), "{err}");
    }

    #[test]
    fn sdk_and_v1_wire_shapes_parse() {
        // sdk items.query(): fields comma-joined, sort bare + order param
        let q: Query =
            parse_query_string("fields=name,email&sort=name&order=desc&limit=5").unwrap();
        assert_eq!(q.fields, vec!["name".to_string(), "email".to_string()]);
        assert_eq!(q.sort, vec!["-name".to_string()]);
        assert_eq!(q.limit, 5);

        // Directus-style comma sort with explicit prefixes is left alone
        // (`%2B` is a literal `+`; a raw `+` decodes to a space)
        let q: Query = parse_query_string("sort=-name,%2Bid&order=desc").unwrap();
        assert_eq!(q.sort, vec!["-name".to_string(), "+id".to_string()]);

        // bracket arrays still work alongside
        let q: Query = parse_query_string("fields[]=name&fields[]=email&sort[]=-name").unwrap();
        assert_eq!(q.fields, vec!["name".to_string(), "email".to_string()]);
        assert_eq!(q.sort, vec!["-name".to_string()]);
    }

    #[test]
    fn split_qs_key_segments() {
        assert_eq!(split_qs_key("filter"), vec!["filter".to_string()]);
        assert_eq!(
            split_qs_key("groupBy[]"),
            vec!["groupBy".to_string(), "".to_string()]
        );
        assert_eq!(
            split_qs_key("aggregate[count]"),
            vec!["aggregate".to_string(), "count".to_string()]
        );
    }
}
