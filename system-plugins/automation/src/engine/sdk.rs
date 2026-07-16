use rquickjs::{Ctx, Function, Object, Result as JsResult};

pub fn create_alcedo_sdk<'js>(ctx: Ctx<'js>, core_url: &str, request_id: &str) -> JsResult<Object<'js>> {
    let sdk = Object::new(ctx.clone())?;
    let client = reqwest::blocking::Client::new();
    let core_url = core_url.to_string();
    let request_id = request_id.to_string();

    // alcedocore.items.get(collection, id) — via db/query proxy (/p/* bypasses auth)
    let items_get = {
        let client = client.clone();
        let core_url = core_url.clone();
        Function::new(ctx.clone(), move |collection: String, id: String| {
            let url = format!("{}/p/automation/db/query", core_url);
            let body = serde_json::json!({
                "query": format!("SELECT row_to_json(t.*)::text as result FROM \"{}\" t WHERE id = $1::uuid", collection),
                "params": [id],
                "timeout_secs": 10,
                "max_rows": 1,
            });
            match client.post(&url).json(&body).send() {
                Ok(resp) => {
                    let text = resp.text().unwrap_or_default();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        let rows = val.get("rows").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                        if let Some(row) = rows.first() {
                            if let Some(cells) = row.as_array() {
                                if let Some(first) = cells.first() {
                                    return first.as_str().unwrap_or(&text).to_string();
                                }
                            }
                        }
                    }
                    text
                }
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    // alcedocore.items.update(collection, id, data) — PATCH to /api/items/{collection}/{id}
    let items_update = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |collection: String, id: String, data: String| {
            let url = format!("{}/api/items/{}/{}", core_url, collection, id);
            match client.patch(&url)
                .header("Content-Type", "application/json")
                .header("X-Request-ID", &request_id)
                .body(data.clone())
                .send()
            {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let items = Object::new(ctx.clone())?;
    items.set("get", items_get)?;
    items.set("update", items_update)?;
    sdk.set("items", items)?;

    // alcedocore.kv.get(key) — with X-Request-ID for auth
    let kv_get = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |key: String| {
            let url = format!("{}/api/kv/{}", core_url, key);
            match client.get(&url).header("X-Request-ID", &request_id).send() {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().unwrap_or_default();
                    if status.is_success() {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(v) = val.get("data").and_then(|v| v.as_str()) {
                                return v.to_string();
                            }
                            if let Some(v) = val.get("data").and_then(|v| v.as_i64()) {
                                return v.to_string();
                            }
                            if let Some(v) = val.get("data").and_then(|v| v.as_f64()) {
                                return v.to_string();
                            }
                        }
                        text
                    } else {
                        // Key not found or other error — return empty string
                        String::new()
                    }
                }
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let kv_set = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
            let key = args.0.get(0).map(|s| s.as_str().to_string()).unwrap_or_default();
            let value = args.0.get(1).map(|s| s.as_str().to_string()).unwrap_or_default();
            let ttl: Option<u64> = args.0.get(2).and_then(|s| s.as_str().parse().ok());
            let url = format!("{}/api/kv/{}", core_url, key);
            let mut body = serde_json::json!({"value": value});
            if let Some(t) = ttl {
                body["ttl"] = serde_json::json!(t);
            }
            match client.put(&url).header("X-Request-ID", &request_id).json(&body).send() {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let kv_delete = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |key: String| {
            let url = format!("{}/api/kv/{}", core_url, key);
            match client.delete(&url).header("X-Request-ID", &request_id).send() {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let kv_increment = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
            let key = args.0.get(0).map(|s| s.as_str().to_string()).unwrap_or_default();
            let amount: i64 = args.0.get(1).and_then(|s| s.as_str().parse().ok()).unwrap_or(1);
            let ttl: Option<u64> = args.0.get(2).and_then(|s| s.as_str().parse().ok());
            let url = match ttl {
                Some(t) => format!("{}/api/kv/{}/increment?ttl={}", core_url, key, t),
                None => format!("{}/api/kv/{}/increment", core_url, key),
            };
            let body = serde_json::json!({"amount": amount});
            match client.post(&url).header("X-Request-ID", &request_id).json(&body).send() {
                Ok(resp) => {
                    let text = resp.text().unwrap_or_default();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(v) = val.get("value") { return v.to_string(); }
                    }
                    text
                }
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let kv_decrement = {
        let client = client.clone();
        let core_url = core_url.clone();
        let request_id = request_id.clone();
        Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
            let key = args.0.get(0).map(|s| s.as_str().to_string()).unwrap_or_default();
            let amount: i64 = args.0.get(1).and_then(|s| s.as_str().parse().ok()).unwrap_or(1);
            let url = format!("{}/api/kv/{}/decrement", core_url, key);
            let body = serde_json::json!({"amount": amount});
            match client.post(&url).header("X-Request-ID", &request_id).json(&body).send() {
                Ok(resp) => {
                    let text = resp.text().unwrap_or_default();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(v) = val.get("value") { return v.to_string(); }
                    }
                    text
                }
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let kv = Object::new(ctx.clone())?;
    kv.set("get", kv_get)?;
    kv.set("set", kv_set)?;
    kv.set("delete", kv_delete)?;
    kv.set("increment", kv_increment)?;
    kv.set("decrement", kv_decrement)?;
    sdk.set("kv", kv)?;

    // alcedocore.health()
    let health = {
        let client = client.clone();
        let core_url = core_url.clone();
        Function::new(ctx.clone(), move || {
            let url = format!("{}/health", core_url);
            match client.get(&url).send() {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    sdk.set("health", health)?;

    // alcedocore.db.query(sql, params?) — POST to /p/automation/db/query proxy
    let db_query = {
        let client = client.clone();
        let core_url = core_url.clone();
        Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
            let sql = args.0.get(0).map(|s| s.as_str().to_string()).unwrap_or_default();
            let upper = sql.trim_start().to_uppercase();
            let forbidden = ["DROP ", "ALTER ", "TRUNCATE ", "CREATE ", "GRANT ", "REVOKE "];
            if forbidden.iter().any(|f| upper.starts_with(f)) {
                return format!("{{\"error\":\"DDL statements are not allowed: {}\"}}", sql);
            }
            let url = format!("{}/p/automation/db/query", core_url);
            let mut body = serde_json::json!({"query": sql, "params": []});
            if let Some(p) = args.0.get(1) {
                let params_str = p.as_str();
                body["params"] = serde_json::from_str::<serde_json::Value>(&params_str).unwrap_or(serde_json::json!([params_str]));
            }
            match client.post(&url).json(&body).send() {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    // alcedocore.db.execute(sql, params?) — POST to /p/automation/db/execute proxy
    let db_execute = {
        let client = client.clone();
        let core_url = core_url.clone();
        Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
            let sql = args.0.get(0).map(|s| s.as_str().to_string()).unwrap_or_default();
            let upper = sql.trim_start().to_uppercase();
            let forbidden = ["DROP ", "ALTER ", "TRUNCATE ", "CREATE ", "GRANT ", "REVOKE "];
            if forbidden.iter().any(|f| upper.starts_with(f)) {
                return format!("{{\"error\":\"DDL statements are not allowed: {}\"}}", sql);
            }
            let url = format!("{}/p/automation/db/execute", core_url);
            let mut body = serde_json::json!({"query": sql, "params": []});
            if let Some(p) = args.0.get(1) {
                let params_str = p.as_str();
                body["params"] = serde_json::from_str::<serde_json::Value>(&params_str).unwrap_or(serde_json::json!([params_str]));
            }
            match client.post(&url).json(&body).send() {
                Ok(resp) => resp.text().unwrap_or_default(),
                Err(e) => format!("{{\"error\":\"{}\"}}", e),
            }
        })?
    };
    let db = Object::new(ctx.clone())?;
    db.set("query", db_query)?;
    db.set("execute", db_execute)?;
    sdk.set("db", db)?;

    Ok(sdk)
}
