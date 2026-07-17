use serde::Deserialize;

#[derive(Deserialize)]
pub struct PutBody {
    pub value: String,
}

#[derive(Deserialize)]
pub struct KeyPrefix {
    pub prefix: Option<String>,
}

#[derive(Deserialize)]
pub struct PutQuery {
    pub ttl: Option<u64>,
}

#[derive(Deserialize)]
pub struct BatchSetItem {
    pub key: String,
    pub value: String,
    pub ttl: Option<u64>,
}

#[derive(Deserialize)]
pub struct BatchGetBody {
    pub keys: Vec<String>,
}

#[derive(Deserialize)]
pub struct BatchDeleteBody {
    pub keys: Vec<String>,
}

#[derive(Deserialize)]
pub struct IncDecBody {
    pub amount: Option<i64>,
}

#[derive(Deserialize)]
pub struct IncDecQuery {
    pub ttl: Option<u64>,
}
