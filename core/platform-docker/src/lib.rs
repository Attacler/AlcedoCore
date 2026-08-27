pub mod client;
pub mod docker_service;
pub mod events;
pub mod platform;
pub mod runtime;
pub mod services;
pub mod swarm;

use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use alcedo_common::AppError;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;

static INNER: OnceLock<Arc<Docker>> = OnceLock::new();

pub struct DockerGlobal;

impl Deref for DockerGlobal {
    type Target = Docker;
    fn deref(&self) -> &Self::Target {
        &**INNER.get().expect("Docker not initialized")
    }
}

pub static DOCKER: DockerGlobal = DockerGlobal;

/// Initialize the global Docker client. Must be called once at startup.
/// Returns an error if the Docker daemon is unreachable.
pub fn init_docker(socket_path: &str) -> Result<(), AppError> {
    let docker = Docker::connect_with_socket(socket_path, 60, API_DEFAULT_VERSION).unwrap();
    INNER
        .set(Arc::new(docker))
        .map_err(|_| AppError::Internal("Docker already initialized".into()))?;
    Ok(())
}
