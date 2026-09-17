pub mod registries;

pub use registries::{
    RegistriesProvider, RegistriesProviderImpl,
    CreateRegistryRequest, UpdateRegistryRequest,
    ListRegistriesResponse, RegistryDetailResponse, DeleteRegistryResponse,
    HealthCheckResponse, ListImagesResponse,
};