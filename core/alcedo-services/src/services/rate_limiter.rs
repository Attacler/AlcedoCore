use alcedo_infra::services::redis_client::RedisClient;

pub async fn check_rate_limit(
    redis: &RedisClient,
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

    let count = redis
        .incr_with_ttl(&key, window_seconds)
        .await
        .map_err(|e| e.to_string())?;

    let remaining = max_requests.saturating_sub(count as u32);
    Ok((count <= max_requests as i64, remaining))
}
