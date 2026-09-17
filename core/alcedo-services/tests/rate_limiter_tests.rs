use alcedo_infra::services::redis_client::RedisClient;
use alcedo_services::services::rate_limiter::check_rate_limit;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

#[tokio::test]
async fn allows_up_to_max_then_denies() {
    let client = RedisClient::connect(&redis_url())
        .await
        .expect("Redis required for rate limiter tests");
    let endpoint = format!("/test/{}", uuid::Uuid::new_v4());
    let ip = "203.0.113.7";
    let max = 3u32;
    let window = 60u64;

    for i in 1..=max {
        let (allowed, remaining) = check_rate_limit(&client, &endpoint, ip, max, window)
            .await
            .unwrap();
        assert!(allowed, "request {i} of {max} should be allowed");
        assert_eq!(remaining, max - i);
    }

    let (allowed, remaining) = check_rate_limit(&client, &endpoint, ip, max, window)
        .await
        .unwrap();
    assert!(!allowed, "request over the limit must be denied");
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn separate_windows_are_independent() {
    let client = RedisClient::connect(&redis_url())
        .await
        .expect("Redis required for rate limiter tests");
    let endpoint = format!("/test/{}", uuid::Uuid::new_v4());
    let (allowed_a, _) = check_rate_limit(&client, &endpoint, "198.51.100.1", 1, 60)
        .await
        .unwrap();
    let (allowed_b, _) = check_rate_limit(&client, &endpoint, "198.51.100.2", 1, 60)
        .await
        .unwrap();
    assert!(allowed_a);
    assert!(allowed_b);
}
