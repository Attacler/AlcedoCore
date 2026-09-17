use serde_json::Value;

use crate::{AppState, services::context::AppContext};

#[derive(Clone)]
pub struct ItemsBeforeDelete {
    pub keys: Vec<Value>,
    pub collection: String,
}
#[derive(Clone)]
pub struct ItemsAfterDelete {
    pub keys: Vec<Value>,
    pub collection: String,
}
