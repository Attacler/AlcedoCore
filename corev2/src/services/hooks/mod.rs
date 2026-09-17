use dashmap::DashMap;
use futures::future::BoxFuture;
use sqlx::{Postgres, Transaction};
use std::any::{Any, TypeId};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::services::app_state::AppState;
use crate::services::context::AppContext;

pub mod systemhooks;
pub mod types;

pub struct HookContext<'tx, 'c> {
    pub context: AppContext,
    pub state: AppState,
    pub tx: &'tx mut Transaction<'c, Postgres>,
}

type AsyncHook<T> = Box<
    dyn for<'a> Fn(
            &'a mut T,
            AppContext,
            AppState,
            &'a mut Transaction<'_, Postgres>,
        ) -> BoxFuture<'a, ()>
        + Send
        + Sync,
>;

struct HookRegistry<T> {
    hooks: Vec<AsyncHook<T>>,
}

pub struct MultiEventBus {
    registries: DashMap<(String, TypeId), Arc<RwLock<dyn Any + Send + Sync>>>,
}

impl MultiEventBus {
    pub fn new() -> Self {
        Self {
            registries: DashMap::new(),
        }
    }

    pub async fn on<T: Send + Sync + 'static, F>(&self, table_name: &str, hook_fn: F)
    where
        F: for<'a> Fn(
                &'a mut T,
                AppContext,
                AppState,
                &'a mut Transaction<'_, Postgres>,
            ) -> BoxFuture<'a, ()>
            + Send
            + Sync
            + 'static,
    {
        let key = (table_name.to_string(), TypeId::of::<T>());

        let registry_any = self.registries.entry(key).or_insert_with(|| {
            let reg: HookRegistry<T> = HookRegistry { hooks: Vec::new() };
            Arc::new(RwLock::new(reg))
        });

        let arc_lock = registry_any.value().clone();
        let mut lock = arc_lock.write().await;

        if let Some(reg) = lock.downcast_mut::<HookRegistry<T>>() {
            reg.hooks.push(Box::new(hook_fn));
        }
    }

    /// Triggers all registered hooks for the given event type.
    pub async fn trigger<'tx, 'c, T: Send + Sync + 'static>(
        &self,
        key: &str,
        event: &mut T,
        context: HookContext<'tx, 'c>,
    ) {
        let lookup_key = (key.to_string(), TypeId::of::<T>());

        if let Some(registry_any) = self.registries.get(&lookup_key) {
            let arc_lock = registry_any.value().clone();
            let lock = arc_lock.read().await;

            if let Some(reg) = lock.downcast_ref::<HookRegistry<T>>() {
                let app_context = context.context.clone();
                let app_state = context.state.clone();

                for hook in &reg.hooks {
                    hook(
                        event,
                        app_context.clone(),
                        app_state.clone(),
                        &mut *context.tx,
                    )
                    .await;
                }
            }
        }
    }
}
