use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Top-level query request
// ---------------------------------------------------------------------------

/// JSON body for `POST /api/collections/:name/items/query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionQueryRequest {
    pub filter: Option<FilterCondition>,
    pub fields: Option<Vec<String>>,
    pub sort: Option<Vec<SortField>>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    #[serde(default = "return_true")]
    pub backlink: bool,
}

fn return_true() -> bool { true }

/// Walk through nested non-operator objects to build a dot-notation field path,
/// then extract the operator and value at the leaf.
/// e.g. `{"customer": {"name": {"_eq": "Acme"}}}` → field="customer.name", op="_eq", val="Acme"
fn parse_nested_rule_path(mut field_path: String, val: Value) -> Result<FilterCondition, String> {
    let obj = val.as_object().ok_or_else(|| {
        format!("rule value for '{}' must be an object", field_path)
    })?;
    if obj.len() != 1 {
        return Err(format!("rule for '{}' must have exactly one key", field_path));
    }
    let (key, inner_val) = obj.into_iter().next().unwrap();

    // If key starts with '_', it's an operator — leaf reached
    if key.starts_with('_') {
        let operator = ComparisonOperator::from_str(key.as_str())
            .ok_or_else(|| format!("unknown operator '{}'", key))?;
        let value = match operator {
            ComparisonOperator::IsNull | ComparisonOperator::IsNotNull => None,
            _ => {
                if inner_val.is_null() { None } else { Some(inner_val.clone()) }
            }
        };
        return Ok(FilterCondition::Rule {
            field: field_path,
            operator,
            value,
        });
    }

    // Key doesn't start with '_' — it's a nested field traversal
    field_path.push('.');
    field_path.push_str(key.as_str());
    parse_nested_rule_path(field_path, inner_val.clone())
}

// ---------------------------------------------------------------------------
// Grouped query request / response (Phase 39 — Kanban view)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedQueryRequest {
    pub group_by: String,
    pub filter: Option<FilterCondition>,
    pub sort: Option<Vec<SortField>>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupResult {
    pub value: Option<String>,
    pub count: i64,
    pub items: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupedQueryResponse {
    pub groups: Vec<GroupResult>,
    pub total: i64,
}

// ---------------------------------------------------------------------------
// Sort
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortField {
    pub field: String,
    #[serde(default)]
    pub order: String,
}

// ---------------------------------------------------------------------------
// Logical operator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum LogicOperator {
    And,
    Or,
}

// ---------------------------------------------------------------------------
// Comparison operator (short-form: _eq, _neq, ... _nin, _nnull)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ComparisonOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    StartsWith,
    EndsWith,
    In,
    NotIn,
    IsNull,
    IsNotNull,
}

impl ComparisonOperator {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Eq => "_eq",
            Self::Neq => "_neq",
            Self::Gt => "_gt",
            Self::Gte => "_gte",
            Self::Lt => "_lt",
            Self::Lte => "_lte",
            Self::Contains => "_contains",
            Self::StartsWith => "_starts_with",
            Self::EndsWith => "_ends_with",
            Self::In => "_in",
            Self::NotIn => "_nin",
            Self::IsNull => "_null",
            Self::IsNotNull => "_nnull",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "_eq" => Some(Self::Eq),
            "_neq" => Some(Self::Neq),
            "_gt" => Some(Self::Gt),
            "_gte" => Some(Self::Gte),
            "_lt" => Some(Self::Lt),
            "_lte" => Some(Self::Lte),
            "_contains" => Some(Self::Contains),
            "_starts_with" => Some(Self::StartsWith),
            "_ends_with" => Some(Self::EndsWith),
            "_in" => Some(Self::In),
            "_nin" => Some(Self::NotIn),
            "_null" => Some(Self::IsNull),
            "_nnull" => Some(Self::IsNotNull),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// FilterCondition — short-form JSON: { "_and": [...] } / { "field": {"_op": v} }
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FilterCondition {
    Group {
        operator: LogicOperator,
        conditions: Vec<FilterCondition>,
    },
    Rule {
        field: String,
        operator: ComparisonOperator,
        value: Option<Value>,
    },
}

// ---- Custom Serialize -----------------------------------------------------

impl Serialize for FilterCondition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            FilterCondition::Group { operator, conditions } => {
                let key = match operator {
                    LogicOperator::And => "_and",
                    LogicOperator::Or => "_or",
                };
                let mut map = Map::with_capacity(1);
                map.insert(key.to_string(), serde_json::to_value(conditions).unwrap_or_default());
                Value::Object(map).serialize(serializer)
            }
            FilterCondition::Rule { field, operator, value } => {
                let op_key = operator.as_str();
                let inner_val = match value {
                    Some(v) if !v.is_null() => v.clone(),
                    _ => Value::Null,
                };
                let mut inner = Map::with_capacity(1);
                inner.insert(op_key.to_string(), inner_val);

                // Split dot-notation field path into nested objects
                // e.g. "customer.name" → {"customer": {"name": {_op: val}}}
                let parts: Vec<&str> = field.split('.').collect();
                if parts.len() > 1 {
                    let mut current = Value::Object(inner);
                    for part in parts.into_iter().rev().skip(1) {
                        let mut layer = Map::with_capacity(1);
                        layer.insert(part.to_string(), current);
                        current = Value::Object(layer);
                    }
                    current.serialize(serializer)
                } else {
                    let mut outer = Map::with_capacity(1);
                    outer.insert(field.clone(), Value::Object(inner));
                    Value::Object(outer).serialize(serializer)
                }
            }
        }
    }
}

// ---- Custom Deserialize ---------------------------------------------------

impl<'de> Deserialize<'de> for FilterCondition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = Value::deserialize(deserializer)?;
        FilterCondition::from_json(val).map_err(de::Error::custom)
    }
}

impl FilterCondition {
    fn from_json(val: Value) -> Result<Self, String> {
        let obj = val.as_object().ok_or_else(|| "filter must be a JSON object".to_string())?;
        if obj.len() != 1 {
            return Err("filter object must have exactly one key".to_string());
        }
        let (key, inner) = obj.into_iter().next().unwrap();

        match key.as_str() {
            "_and" | "_or" => {
                let arr = inner.as_array().ok_or_else(|| format!("'{}' must be an array", key))?;
                let operator = match key.as_str() {
                    "_and" => LogicOperator::And,
                    "_or" => LogicOperator::Or,
                    _ => unreachable!(),
                };
                let mut conditions = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    conditions.push(
                        Self::from_json(item.clone())
                            .map_err(|e| format!("condition {}: {}", i, e))?,
                    );
                }
                Ok(FilterCondition::Group { operator, conditions })
            }
            field_name => {
                parse_nested_rule_path(field_name.to_string(), inner.clone())
            }
        }
    }
}
