use std::sync::Arc;

use file_storage::FileStorage;
use futures::future::BoxFuture;
use sqlx::{Pool, Postgres, Transaction};
use tokio::sync::{Mutex, RwLock};

use crate::services::{
    cache::SystemCache, config, errors::AlcedoError, hooks::MultiEventBus,
    postgres::inspector::DatabaseSchema,
};
const SCHEMA_CACHE_KEY: &str = "system_schema_cache";
const SCHEMA_CACHE_GEN_KEY: &str = "system_schema_cache_gen";

#[derive(Clone)]
pub struct AppState {
    pub database_pool: Arc<Pool<Postgres>>,
    pub database_schema: Arc<RwLock<DatabaseSchema>>,
    pub event_bus: Arc<MultiEventBus>,
    pub config: config::Config,
    pub cache: SystemCache,
    pub kv_cache: SystemCache,
    pub file_storage: Arc<dyn FileStorage>,
    pub schema_cache_gen: Arc<Mutex<String>>,
}
impl AppState {
    pub async fn refresh_schema(&self) {
        let mut lock = self.database_schema.write().await;
        let refreshed = lock.refresh(self).await;
        lock.tables = refreshed.tables;
        lock.columns = refreshed.columns;
        lock.app_versions = refreshed.app_versions;
    }

    pub async fn db_new_transaction<T, F>(&self, work: F) -> Result<T, AlcedoError>
    where
        F: for<'a> FnOnce(
            &'a mut Transaction<'_, Postgres>,
        ) -> BoxFuture<'a, Result<T, AlcedoError>>,
    {
        let mut new_tx = self.database_pool.begin().await?;
        match work(&mut new_tx).await {
            Ok(result) => {
                new_tx.commit().await?;
                Ok(result)
            }
            Err(e) => {
                new_tx.rollback().await?;
                Err(e)
            }
        }
    }

    pub async fn db_transaction<T, F>(
        &self,
        existing_tx: &mut Option<&mut Transaction<'_, Postgres>>,
        work: F,
    ) -> Result<T, AlcedoError>
    where
        F: for<'a> FnOnce(
            &'a mut Transaction<'_, Postgres>,
        ) -> BoxFuture<'a, Result<T, AlcedoError>>,
    {
        match existing_tx {
            Some(tx) => work(tx).await,
            None => self.db_new_transaction(work).await,
        }
    }

    pub async fn save_schema_cache(&self) {
        let raw = {
            let schema = self.database_schema.read().await;
            serde_json::to_string(&*schema).ok()
        };
        let Some(raw) = raw else { return };

        // Blob first, token second: the token is the commit marker, so a reader
        // can never see a token whose blob is not yet written.
        if self
            .cache
            .set(SCHEMA_CACHE_KEY.to_string(), raw)
            .await
            .is_ok()
        {
            let token = uuid::Uuid::new_v4().to_string();
            if self
                .cache
                .set(SCHEMA_CACHE_GEN_KEY.to_string(), token.clone())
                .await
                .is_ok()
            {
                *self.schema_cache_gen.lock().await = token;
            }
        }
    }

    pub async fn load_cached_schema_if_stale(&self) -> Option<DatabaseSchema> {
        let token = self.cache.get(SCHEMA_CACHE_GEN_KEY).await.ok()??;
        if *self.schema_cache_gen.lock().await == token {
            return None;
        }

        let schema = self.load_cached_schema().await?;
        *self.schema_cache_gen.lock().await = token;
        Some(schema)
    }

    /// Loads the cached schema, if present.
    pub async fn load_cached_schema(&self) -> Option<DatabaseSchema> {
        let raw = self.cache.get(SCHEMA_CACHE_KEY).await.ok()??;
        serde_json::from_str(&raw).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schema_round_trips_through_shared_cache_with_meta() {
        use crate::services::collections::schema::SchemaService;

        let state = crate::utils::test_utils::get_app_state().await;
        // The cache is written by the meta pass, not by raw introspection.
        SchemaService::refresh_all_meta(&state).await;

        let cached = state
            .load_cached_schema()
            .await
            .expect("refresh_all_meta should persist the schema to the cache");

        let live = state.database_schema.read().await;
        assert_eq!(cached.tables.len(), live.tables.len());
        assert_eq!(cached.columns.len(), live.columns.len());
        assert_eq!(cached.app_versions.len(), live.app_versions.len());
        drop(live);

        // The writer recorded its own token, so a request on this process
        // doesn't refetch until another replica writes.
        assert!(state.load_cached_schema_if_stale().await.is_none());

        // Simulate another replica's write: clear the seen token, then the next
        // request picks the new schema up exactly once.
        *state.schema_cache_gen.lock().await = String::new();
        assert!(state.load_cached_schema_if_stale().await.is_some());
        assert!(state.load_cached_schema_if_stale().await.is_none());
    }
}
