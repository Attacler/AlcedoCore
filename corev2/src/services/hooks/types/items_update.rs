use serde_json::Value;

use crate::{AppState, services::context::AppContext};

#[derive(Clone)]
pub struct ItemsBeforeUpdate {
    pub keys: Vec<String>,
    pub payload: serde_json::Map<String, Value>,
    pub collection: String,
}
#[derive(Clone)]
pub struct ItemsAfterUpdate {
    pub keys: Vec<String>,
    pub payload: serde_json::Map<String, Value>,
    pub collection: String,
}
