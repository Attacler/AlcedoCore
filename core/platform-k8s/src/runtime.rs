use async_trait::async_trait;
use alcedo_container::container::{ContainerDetails, ContainerInfo, ImageInfo};
use alcedo_common::AppError;
use std::collections::HashMap;

pub struct K8sRuntime;

impl K8sRuntime {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl alcedo_container::container::ContainerRuntime for K8sRuntime {
    async fn pull_image(&self, _image: &str) -> Result<(), AppError> {
        Ok(())
    }
    async fn create_container(
        &self,
        _slug: &str,
        _version: &str,
        _image: &str,
        _env: HashMap<String, String>,
        _network_mode: Option<&str>,
    ) -> Result<String, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn start_container(&self, _id: &str) -> Result<(), AppError> {
        Ok(())
    }
    async fn stop_container(&self, _id: &str, _timeout: i64) -> Result<(), AppError> {
        Ok(())
    }
    async fn remove_container(&self, _id: &str, _force: bool) -> Result<(), AppError> {
        Ok(())
    }
    async fn copy_directory_from_container(
        &self,
        _id: &str,
        _src: &str,
        _dest: &str,
    ) -> Result<(), AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn copy_directory_from_image(
        &self,
        _image: &str,
        _src: &str,
        _dest: &str,
    ) -> Result<(), AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn get_container_ip(&self, _id: &str, _net: &str) -> Result<Option<String>, AppError> {
        Ok(None)
    }
    async fn inspect_container(&self, _id: &str) -> Result<ContainerDetails, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError> {
        Ok(vec![])
    }
    async fn restart_container(&self, _id: &str) -> Result<(), AppError> {
        Ok(())
    }
    async fn get_file_from_container(&self, _id: &str, _path: &str) -> Result<Vec<u8>, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn list_directory_in_container(
        &self,
        _id: &str,
        _path: &str,
    ) -> Result<Vec<String>, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn list_directory_recursive_in_container(
        &self,
        _id: &str,
        _path: &str,
    ) -> Result<Vec<String>, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn inspect_image(&self, _name: &str) -> Result<ImageInfo, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn get_file_from_image(&self, _name: &str, _path: &str) -> Result<String, AppError> {
        Err(AppError::Internal("Not implemented".to_string()))
    }
    async fn connect_container_to_network(&self, _id: &str, _net: &str) -> Result<(), AppError> {
        Ok(())
    }
}
