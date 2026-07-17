use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type RedisConn = Arc<Mutex<ConnectionManager>>;

pub async fn check_rate_limit(
    redis: &RedisConn,
    endpoint: &str,
    client_ip: &str,
    max_requests: u32,
    window_seconds: u64,
) -> Result<(bool, u32), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let window_key = now / window_seconds;
    let key = format!("ratelimit:{}:{}:{}", endpoint, client_ip, window_key);

    let mut conn = redis.lock().await;
    let count: u64 = redis::cmd("INCR")
        .arg(&key)
        .query_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    if count == 1 {
        let _: () = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(window_seconds as i64)
            .query_async(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
    }

    let remaining = max_requests.saturating_sub(count as u32);
    Ok((count <= max_requests as u64, remaining))
}
