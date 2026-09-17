use std::sync::Arc;

use alcedo_plugins::plugins::health::{PluginHealthMap, PluginHealthStatus};
use alcedo_plugins::services::redis_client::RedisClient;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

#[tokio::test]
async fn health_roundtrip_via_redis() {
    let client = RedisClient::connect(&redis_url())
        .await
        .expect("Redis required for health tests");
    let install_id: i64 = uuid::Uuid::new_v4().as_u128() as i64;
    let slug = format!("test-health-{}", uuid::Uuid::new_v4());

    // Writer populates Redis (and its own in-memory map).
    let writer = PluginHealthMap::new(Some(Arc::new(client.clone())));
    writer
        .update_plugin_health(
            install_id,
            Some(1),
            slug.clone(),
            PluginHealthStatus::Healthy,
            Some("cid-1".to_string()),
        )
        .await;

    // A fresh map has an empty in-memory cache, so this MUST be served from Redis.
    let reader = PluginHealthMap::new(Some(Arc::new(client.clone())));
    let entry = reader
        .get_plugin_health(install_id)
        .await
        .expect("health entry should be read back from Redis, not memory");
    assert_eq!(entry.install_id, install_id);
    assert_eq!(entry.app_version_id, Some(1));
    assert_eq!(entry.slug, slug);
    assert_eq!(entry.status, PluginHealthStatus::Healthy);
    assert_eq!(entry.deployment_id.as_deref(), Some("cid-1"));

    // Clean up the hash (the set index entry self-tolerates).
    let _ = client.del(&format!("plugin_health:{}", install_id)).await;
}

/// Two installs sharing the same slug (a global install and a version install)
/// must not overwrite each other's health.
#[tokio::test]
async fn health_is_scoped_by_install_not_slug() {
    let client = RedisClient::connect(&redis_url())
        .await
        .expect("Redis required for health tests");
    let slug = format!("test-shared-slug-{}", uuid::Uuid::new_v4());
    let global_install: i64 = uuid::Uuid::new_v4().as_u128() as i64;
    let version_install: i64 = global_install.wrapping_add(1);

    let map = PluginHealthMap::new(Some(Arc::new(client.clone())));

    map.update_plugin_health(
        global_install,
        None,
        slug.clone(),
        PluginHealthStatus::Healthy,
        Some("global-cid".to_string()),
    )
    .await;
    map.update_plugin_health(
        version_install,
        Some(7),
        slug.clone(),
        PluginHealthStatus::Failed,
        Some("version-cid".to_string()),
    )
    .await;

    // Fresh map → read from Redis to prove the keys are distinct.
    let reader = PluginHealthMap::new(Some(Arc::new(client.clone())));
    let global = reader.get_plugin_health(global_install).await.unwrap();
    let version = reader.get_plugin_health(version_install).await.unwrap();

    assert_eq!(global.status, PluginHealthStatus::Healthy);
    assert_eq!(global.app_version_id, None);
    assert_eq!(global.deployment_id.as_deref(), Some("global-cid"));

    assert_eq!(version.status, PluginHealthStatus::Failed);
    assert_eq!(version.app_version_id, Some(7));
    assert_eq!(version.deployment_id.as_deref(), Some("version-cid"));

    let all = reader.get_all_health().await;
    assert!(all.iter().any(|e| e.install_id == global_install));
    assert!(all.iter().any(|e| e.install_id == version_install));

    let _ = client.del(&format!("plugin_health:{}", global_install)).await;
    let _ = client.del(&format!("plugin_health:{}", version_install)).await;
}
