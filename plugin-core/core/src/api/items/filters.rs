use serde_json::Value;

use crate::db::filter_condition::FilterCondition;
use crate::db::query_builder::FilterCondition as QueryFilterCondition;
use crate::db::query_builder::FilterOperator;

/// Check whether a FilterCondition contains any dot-notation field paths.
pub fn filter_has_dot_path(filter: &FilterCondition) -> bool {
    match filter {
        FilterCondition::Group { conditions, .. } => {
            conditions.iter().any(|c| filter_has_dot_path(c))
        }
        FilterCondition::Rule { field, .. } => field.contains('.'),
    }
}

/// Convert permission policy filters into query_builder::FilterCondition format.
/// Rules are OR-ed, conditions within a rule are AND-ed.
pub fn permissions_to_query_filter(permissions: &[crate::services::permissions::PolicyPermission]) -> QueryFilterCondition {
    let rules: Vec<QueryFilterCondition> = permissions.iter().map(|perm| {
        let filters = perm.filter.as_array().cloned().unwrap_or_default();
        let conditions: Vec<QueryFilterCondition> = filters.iter().map(|cond: &Value| {
            let field = cond.get("field").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let operator_str = cond.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
            let value = cond.get("value").cloned();

            let operator = match operator_str {
                "eq" => FilterOperator::Eq,
                "not_eq" => FilterOperator::Neq,
                "gt" => FilterOperator::Gt,
                "gte" => FilterOperator::Gte,
                "lt" => FilterOperator::Lt,
                "lte" => FilterOperator::Lte,
                "contains" => FilterOperator::Contains,
                "starts_with" => FilterOperator::StartsWith,
                "ends_with" => FilterOperator::EndsWith,
                "in" => FilterOperator::In,
                "not_in" => FilterOperator::NotIn,
                "is_null" => FilterOperator::Null,
                "is_not_null" => FilterOperator::NotNull,
                _ => FilterOperator::Eq,
            };

            QueryFilterCondition {
                field: Some(field),
                operator: Some(operator),
                value,
                combinator: None,
                conditions: None,
            }
        }).collect();

        if conditions.len() == 1 {
            conditions.into_iter().next().unwrap()
        } else {
            QueryFilterCondition {
                field: None,
                operator: None,
                value: None,
                combinator: Some("and".to_string()),
                conditions: Some(conditions),
            }
        }
    }).collect();

    if rules.len() == 1 {
        rules.into_iter().next().unwrap()
    } else {
        QueryFilterCondition {
            field: None,
            operator: None,
            value: None,
            combinator: Some("or".to_string()),
            conditions: Some(rules),
        }
    }
}
