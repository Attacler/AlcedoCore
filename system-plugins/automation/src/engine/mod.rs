mod sdk;

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use rquickjs::Promise;

/// Execute a JavaScript function and return the full result as JSON.
/// Result: { "output": "...", "logs": [...], "success": true/false, "error": "..." }
pub fn execute_function(
    code: &str,
    event_data: serde_json::Value,
    core_url: &str,
    request_id: &str,
) -> Result<serde_json::Value, String> {
    let runtime = rquickjs::Runtime::new().map_err(|e| format!("Runtime init error: {}", e))?;

    let _ = runtime.set_memory_limit(10 * 1024 * 1024);  // 10 MB max
    let _ = runtime.set_max_stack_size(256 * 1024);       // 256 KB stack

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        std::time::Instant::now() >= deadline
    })));

    let ctx = rquickjs::Context::full(&runtime).map_err(|e| format!("Context init error: {}", e))?;

    let log_counter = Arc::new(AtomicI32::new(0));
    let console_logs: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));

    let result_str: String = ctx
        .with(|ctx| {
            let logs_for_log = console_logs.clone();
            let counter_for_log = log_counter.clone();
            let log_fn = rquickjs::Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
                let n = counter_for_log.fetch_add(1, Ordering::SeqCst) + 1;
                let content = args.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
                if let Ok(mut guard) = logs_for_log.lock() {
                    guard.push(serde_json::json!({
                        "ln": n,
                        "content": content,
                        "type": "log"
                    }));
                }
            })
            .map_err(|e| format!("console.log error: {}", e))?;

            let logs_for_err = console_logs.clone();
            let counter_for_err = log_counter.clone();
            let err_fn = rquickjs::Function::new(ctx.clone(), move |args: rquickjs::function::Rest<rquickjs::Coerced<String>>| {
                let n = counter_for_err.fetch_add(1, Ordering::SeqCst) + 1;
                let content = args.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
                if let Ok(mut guard) = logs_for_err.lock() {
                    guard.push(serde_json::json!({
                        "ln": n,
                        "content": content,
                        "type": "err"
                    }));
                }
            })
            .map_err(|e| format!("console.error error: {}", e))?;

            let console = rquickjs::Object::new(ctx.clone()).map_err(|e| format!("console obj error: {}", e))?;
            console.set("log", log_fn).map_err(|e| format!("console set error: {}", e))?;
            console.set("error", err_fn).map_err(|e| format!("console set error: {}", e))?;
            ctx.globals().set("console", console).map_err(|e| format!("console global error: {}", e))?;

            let sdk = sdk::create_alcedo_sdk(ctx.clone(), core_url, request_id).map_err(|e| format!("sdk error: {}", e))?;
            ctx.globals().set("alcedocore", sdk).map_err(|e| format!("alcedocore error: {}", e))?;

            let event_json = serde_json::to_string(&event_data).map_err(|e| format!("event json error: {}", e))?;
            let event_js: rquickjs::Value = ctx
                .json_parse(event_json.as_bytes())
                .map_err(|e| format!("event parse error: {}", e))?;
            ctx.globals().set("event", event_js).map_err(|e| format!("event global error: {}", e))?;

            let wrapped_code = format!(
                r#"(async function() {{
                    const handler = async (alcedocore, event) => {{
                        {}
                    }};
                    try {{
                        const result = await handler(alcedocore, event);
                        return JSON.stringify({{ success: true, output: result === undefined ? null : result }});
                    }} catch (err) {{
                        return JSON.stringify({{ success: false, error: err.message || String(err) }});
                    }}
                }})()"#,
                code
            );

            let result_value: rquickjs::Value = ctx
                .eval(wrapped_code.as_str())
                .map_err(|e| format!("JS execution error: {}", e.to_string()))?;

            // Wrap the value in a Promise and resolve it synchronously
            let promise = Promise::from_value(result_value).map_err(|e| format!("Promise from_value error: {}", e))?;
            let result_val: rquickjs::Value = promise.finish().map_err(|e| format!("Promise finish error: {}", e))?;
            // Try to get the string value from the resolved promise
            let s: String = if result_val.is_string() {
                rquickjs::FromJs::from_js(&ctx, result_val.clone())
                    .map_err(|e| format!("Convert result string error: {}", e))?
            } else if result_val.is_undefined() {
                // Promise resolved to undefined — something went wrong in JS
                // Try to get error info
                let error_str: String = ctx.eval("if (typeof globalError !== 'undefined') JSON.stringify(globalError) else JSON.stringify({error: 'unknown'})")
                    .unwrap_or_else(|_| "{}".to_string());
                return Err(format!("Async function returned undefined. Error info: {}", error_str));
            } else {
                // Some other type — try JSON stringify
                ctx.json_stringify(result_val)
                    .map_err(|e| format!("JSON stringify error: {}", e))?
                    .ok_or_else(|| "JSON stringify returned None".to_string())?
                    .to_string()
                    .map_err(|e| format!("String conversion error: {}", e))?
            };
            Ok::<_, String>(s)
        })
        .map_err(|e: String| e)?;

    let mut parsed: serde_json::Value =
        serde_json::from_str(&result_str).map_err(|e| format!("Result parse error: {}", e))?;

    // Attach captured logs
    if let Ok(guard) = console_logs.lock() {
        parsed["logs"] = serde_json::Value::Array(guard.clone());
    }

    if parsed.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(parsed)
    } else {
        let error = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
        Err(error.to_string())
    }
}


