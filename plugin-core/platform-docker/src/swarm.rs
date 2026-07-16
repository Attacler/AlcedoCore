use bollard::Docker;
use serde::Serialize;

/// Docker Swarm state — whether Swarm mode is available and initialized.
#[derive(Debug, Clone, Serialize)]
pub struct SwarmState {
    pub enabled: bool,
    pub is_swarm_manager: bool,
}

impl SwarmState {
    pub fn new(enabled: bool, is_swarm_manager: bool) -> Self {
        Self { enabled, is_swarm_manager }
    }
    pub fn disabled() -> Self {
        Self { enabled: false, is_swarm_manager: false }
    }
}

pub async fn detect_swarm(docker: &Docker) -> SwarmState {
    match docker.info().await {
        Ok(info) => {
            let control_available = info.swarm
                .as_ref()
                .and_then(|s| s.control_available)
                .unwrap_or(false);

            if control_available {
                tracing::info!("[SWARM] Docker Swarm mode detected — control available");
                SwarmState::new(true, true)
            } else {
                tracing::info!("[SWARM] Docker is not in Swarm mode — control not available");
                SwarmState::disabled()
            }
        }
        Err(e) => {
            tracing::warn!("[SWARM] Failed to detect Docker Swarm status: {:?}", e);
            SwarmState::disabled()
        }
    }
}

