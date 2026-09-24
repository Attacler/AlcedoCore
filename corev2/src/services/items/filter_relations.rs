//! Resolves the virtual `one_to_many` relation hops referenced by a filter.
//!
//! A `one_to_many` field has no column on the source table (the FK lives on the
//! child), so the query builder cannot discover it from the introspected
//! schema. This module walks the filter and produces, for every referenced 1:M
//! field, the child schema/table plus the child column that links back to the
//! parent. `query.rs` then turns those into `EXISTS` subqueries.

use std::collections::HashMap;

use crate::services::collections::FieldDefinition;
use crate::services::collections::ddl::related_app_schema;
use crate::services::context::AppContext;
use crate::services::errors::AlcedoError;
use crate::services::items::query::{FieldFilter, FieldValue, Filter, LogicOp};
use crate::services::items::relational::{collection_fields, reverse_fk, target_ctx};
use crate::AppState;

/// The child side of a virtual 1:M relation, keyed by
/// `(source_schema, source_table, field_name)`.
#[derive(Debug, Clone)]
pub(crate) struct OneToManyHop {
    pub child_schema: String,
    pub child_table: String,
    /// FK column on the child that references the parent's primary key.
    pub fk_column: String,
}

pub(crate) type HopKey = (String, String, String);
pub(crate) type HopMap = HashMap<HopKey, OneToManyHop>;

fn iter_filters(logic: &LogicOp) -> impl Iterator<Item = &Filter> {
    logic
        ._and
        .iter()
        .flatten()
        .chain(logic._or.iter().flatten())
}

async fn load_fields(
    state: &AppState,
    ctx: &AppContext,
    collection: &str,
    cache: &mut HashMap<String, Vec<FieldDefinition>>,
) -> Result<Vec<FieldDefinition>, AlcedoError> {
    let key = format!("{}::{}", ctx.schema_name(), collection);
    if let Some(fields) = cache.get(&key) {
        return Ok(fields.clone());
    }
    let fields = collection_fields(state, ctx, collection).await?;
    cache.insert(key, fields.clone());
    Ok(fields)
}

/// Walks `filter` and returns every referenced 1:M hop. Traversal follows the
/// filter structure, which is finite, so no cycle guard is needed.
pub(crate) async fn resolve_filter_relation_hops(
    state: &AppState,
    ctx: &AppContext,
    collection: &str,
    filter: &LogicOp,
) -> Result<HopMap, AlcedoError> {
    let mut hops = HopMap::new();
    let mut cache: HashMap<String, Vec<FieldDefinition>> = HashMap::new();

    let mut queue: Vec<(AppContext, String, LogicOp)> =
        vec![(ctx.clone(), collection.to_string(), filter.clone())];

    while let Some((cur_ctx, cur_coll, logic)) = queue.pop() {
        // Collect nested values first. Field metadata is only loaded when a
        // nested value is present, so metadata queries (which use plain
        // comparisons) never re-enter the collections service.
        let mut nested_values: Vec<(String, FieldFilter)> = Vec::new();
        for item in iter_filters(&logic) {
            match item {
                Filter::Field(field_filter) => {
                    for (field_name, value) in &field_filter.fields {
                        if let FieldValue::Nested(nested) = value {
                            nested_values.push((field_name.clone(), nested.clone()));
                        }
                    }
                }
                Filter::Logic(inner) => {
                    queue.push((cur_ctx.clone(), cur_coll.clone(), inner.clone()));
                }
            }
        }

        if nested_values.is_empty() {
            continue;
        }

        let fields = load_fields(state, &cur_ctx, &cur_coll, &mut cache).await?;

        for (field_name, nested) in nested_values {
            let field = match fields
                .iter()
                .find(|f| f.name == field_name && f.is_relationship())
            {
                Some(f) => f,
                None => continue,
            };
            let related = match field.related_collection.clone() {
                Some(c) => c,
                None => continue,
            };
            let child_ctx = target_ctx(&cur_ctx, &field.related_app);

            if field.is_virtual() {
                if let Some(fk) = reverse_fk(state, &child_ctx, &related, &cur_coll).await? {
                    hops.insert(
                        (cur_ctx.schema_name(), cur_coll.clone(), field_name.clone()),
                        OneToManyHop {
                            child_schema: related_app_schema(&cur_ctx, &field.related_app),
                            child_table: related.clone(),
                            fk_column: fk,
                        },
                    );
                }
            }

            // Descend into the related collection so 1:M hops nested behind
            // this relation are resolved too.
            queue.push((
                child_ctx,
                related,
                LogicOp {
                    _and: Some(vec![Filter::Field(nested)]),
                    _or: None,
                },
            ));
        }
    }

    Ok(hops)
}
