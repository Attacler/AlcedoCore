use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::{Pool, Postgres, Transaction};
use tokio::sync::RwLock;

use crate::services::{
    cache::SystemCache, config, errors::AlcedoError, hooks::MultiEventBus,
    postgres::inspector::DatabaseSchema,
};

#[derive(Clone)]
pub struct AppState {
    pub database_pool: Arc<Pool<Postgres>>,
    pub database_schema: Arc<RwLock<DatabaseSchema>>,
    pub event_bus: Arc<MultiEventBus>,
    pub config: config::Config,
    pub cache: SystemCache,
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
}
