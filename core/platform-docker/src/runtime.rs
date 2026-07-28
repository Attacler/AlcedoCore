use crate::client::DockerClient;
use pcl::container::{ContainerDetails, ContainerInfo, ContainerRuntime, ImageInfo};
use pcl::delegate_impl;
use pcl::AppError;
use std::collections::HashMap;

pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

delegate_impl!(ContainerRuntime, DockerRuntime, DockerClient, {
    fn pull_image(image: &str) -> Result<(), AppError>;
    fn create_container(
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        network_mode: Option<&str>,
    ) -> Result<String, AppError>;
    fn start_container(container_id: &str) -> Result<(), AppError>;
    fn stop_container(container_id: &str, timeout_secs: i64) -> Result<(), AppError>;
    fn remove_container(container_id: &str, force: bool) -> Result<(), AppError>;
    fn list_containers() -> Result<Vec<ContainerInfo>, AppError>;
    fn inspect_container(container_id: &str) -> Result<ContainerDetails, AppError>;
    fn get_container_ip(container_id: &str, network_name: &str)
        -> Result<Option<String>, AppError>;
    fn restart_container(container_id: &str) -> Result<(), AppError>;
    fn inspect_image(image_name: &str) -> Result<ImageInfo, AppError>;
    fn get_file_from_image(image_name: &str, file_path: &str) -> Result<String, AppError>;
    fn get_file_from_container(container_id: &str, path: &str) -> Result<Vec<u8>, AppError>;
    fn list_directory_in_container(container_id: &str, path: &str)
        -> Result<Vec<String>, AppError>;
    fn list_directory_recursive_in_container(
        container_id: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError>;
    fn copy_directory_from_container(
        container_id: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError>;
    fn copy_directory_from_image(
        image_name: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError>;
    fn connect_container_to_network(container_id: &str, network_name: &str)
        -> Result<(), AppError>;
});
