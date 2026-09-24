use axum::{extract::FromRequestParts, http::request::Parts};
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_qs::axum::QsQuery;
use std::collections::HashMap;

use crate::services::errors::AlcedoError;

pub struct CustomQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for CustomQuery<T>
where
    T: DeserializeOwned + 'static,
    S: Send + Sync,
{
    type Rejection = AlcedoError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        let query_string = uri.query().unwrap_or("").to_string();

        match QsQuery::<T>::from_request_parts(parts, _state).await {
            Ok(QsQuery(value)) => Ok(CustomQuery(value)),
            Err(qs_err) => {
                let mut params: HashMap<String, String> = HashMap::new();
                let mut has_json_values = false;

                for (key, value) in url::form_urlencoded::parse(query_string.as_bytes()) {
                    let key_str = key.to_string();
                    let value_str = value.to_string();

                    if value_str.starts_with('{') || value_str.starts_with('[') {
                        has_json_values = true;
                        params.insert(key_str, value_str);
                    } else {
                        params.insert(key_str, value_str);
                    }
                }

                if has_json_values {
                    let mut json_obj: HashMap<String, Value> = HashMap::new();

                    for (key, value) in params {
                        if value.starts_with('{') || value.starts_with('[') {
                            match serde_json::from_str::<Value>(&value) {
                                Ok(json_val) => {
                                    json_obj.insert(key, json_val);
                                }
                                Err(json_err) => {
                                    return Err(AlcedoError::InvalidInput(
                                        format!(
                                            "Invalid JSON in parameter '{}': {}",
                                            key, json_err
                                        ),
                                        0,
                                    ));
                                }
                            }
                        } else if let Ok(num) = value.parse::<u64>() {
                            json_obj.insert(key, Value::Number(num.into()));
                        } else if let Ok(num) = value.parse::<i64>() {
                            json_obj.insert(key, Value::Number(num.into()));
                        } else if let Ok(num) = value.parse::<f64>() {
                            json_obj.insert(
                                key,
                                Value::Number(serde_json::Number::from_f64(num).unwrap()),
                            );
                        } else if value.to_lowercase() == "true" {
                            json_obj.insert(key, Value::Bool(true));
                        } else if value.to_lowercase() == "false" {
                            json_obj.insert(key, Value::Bool(false));
                        } else if value.to_lowercase() == "null" {
                            json_obj.insert(key, Value::Null);
                        } else {
                            json_obj.insert(key, Value::String(value));
                        }
                    }

                    // A `filter` that is a bare field map (no `_and`/`_or`) is
                    // treated as an implicit `_and`. The admin UI's relational
                    // sections send e.g. `filter={"customer":{"_eq":"..."}}`.
                    let needs_wrapping = match json_obj.get("filter") {
                        Some(Value::Object(map)) => {
                            !map.is_empty() && !map.contains_key("_and") && !map.contains_key("_or")
                        }
                        _ => false,
                    };
                    if needs_wrapping {
                        if let Some(Value::Object(map)) = json_obj.get("filter").cloned() {
                            json_obj.insert(
                                "filter".to_string(),
                                serde_json::json!({ "_and": [Value::Object(map)] }),
                            );
                        }
                    }

                    let json_value = serde_json::to_value(json_obj).map_err(|e| {
                        AlcedoError::InvalidInput(format!("Failed to construct JSON: {}", e), 0)
                    })?;

                    match serde_json::from_value::<T>(json_value) {
                        Ok(value) => Ok(CustomQuery(value)),
                        Err(de_err) => {
                            let error_message = match de_err.classify() {
                                serde_json::error::Category::Data => {
                                    format!("Data type error: {}", de_err)
                                }
                                serde_json::error::Category::Syntax => {
                                    format!("JSON syntax error: {}", de_err)
                                }
                                serde_json::error::Category::Eof => {
                                    format!("Unexpected end of JSON: {}", de_err)
                                }
                                _ => format!("Deserialization error: {}", de_err),
                            };
                            Err(AlcedoError::InvalidInput(error_message, 0))
                        }
                    }
                } else {
                    let error_message = format!("Invalid query parameters: {}", qs_err);

                    Err(AlcedoError::InvalidInput(error_message, 0))
                }
            }
        }
    }
}
