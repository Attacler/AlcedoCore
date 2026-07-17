use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A registered dev session — maps a plugin slug to a local dev server URL.
#[derive(Debug, Clone)]
pub struct DevSession {
    pub slug: String,
    pub url: String,          // e.g., "http://localhost:3000"
    pub started_at: DateTime<Utc>,
    pub ttl_secs: u64,
}

/// Thread-safe registry of dev sessions, keyed by plugin slug.
/// In production this is `None` — zero overhead.
#[derive(Debug, Default)]
pub struct DevSessionRegistry {
    sessions: Arc<RwLock<HashMap<String, DevSession>>>,
}

impl DevSessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a dev session. Overwrites any existing session for the same slug
    /// (single session per slug per decisions in CONTEXT.md deferred section).
    pub async fn register(&self, slug: String, url: String, ttl_secs: u64) -> DevSession {
        let session = DevSession {
            slug: slug.clone(),
            url,
            started_at: Utc::now(),
            ttl_secs,
        };
        self.sessions.write().await.insert(slug, session.clone());
        session
    }

    /// Unregister a dev session. Returns true if a session existed for the slug.
    pub async fn unregister(&self, slug: &str) -> bool {
        self.sessions.write().await.remove(slug).is_some()
    }

    /// Get a dev session for a slug (used by proxy handler).
    /// Returns None if no session exists or if the session has expired.
    pub async fn get(&self, slug: &str) -> Option<DevSession> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(slug)?;
        let elapsed = (Utc::now() - session.started_at).num_seconds() as u64;
        if elapsed >= session.ttl_secs {
            None  // Expired; actual cleanup happens in background task
        } else {
            Some(session.clone())
        }
    }

    /// Remove expired sessions. Called periodically by `spawn_ttl_cleanup`.
    pub async fn cleanup_expired(&self) {
        let mut sessions = self.sessions.write().await;
        let now = Utc::now();
        sessions.retain(|_, s| {
            let elapsed = (now - s.started_at).num_seconds() as u64;
            elapsed < s.ttl_secs
        });
    }
}

/// Spawn a background tokio task that cleans up expired sessions every 60 seconds.
/// Call this during startup when creating a DevSessionRegistry.
pub fn spawn_ttl_cleanup(registry: Arc<DevSessionRegistry>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            registry.cleanup_expired().await;
        }
    });
}
