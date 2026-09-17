use std::sync::Arc;

use serde_json::Value;
use sqlx::{Postgres, Transaction};

use crate::{AppState, services::context::AppContext};

#[derive(Clone)]
pub struct ItemsBeforeCreate {
    pub items: Vec<serde_json::Map<String, Value>>,
    pub collection: String,
}
#[derive(Clone)]
pub struct ItemsAfterCreate {
    pub items: Vec<serde_json::Map<String, Value>>,
    pub collection: String,
}
