use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{ActivityLogPage, TimelineEntry, TimelineEntrySystem};
use reqwest::Method;

pub struct ActivityLogsResource {
    client: BaseClient,
}

impl_new!(ActivityLogsResource);

impl ActivityLogsResource {
    /// App-scoped item activity: `GET /api/app/logs/collections`.
    pub async fn list_collections(
        &self,
        params: Option<&[(&str, &str)]>,
    ) -> Result<ActivityLogPage<TimelineEntry>, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/app/logs/collections");
        if let Some(p) = params {
            builder = builder.query(p);
        }
        self.client.execute_json(builder).await
    }

    /// App-scoped, non-item activity: `GET /api/app/logs/system`.
    pub async fn list_app_system(
        &self,
        params: Option<&[(&str, &str)]>,
    ) -> Result<ActivityLogPage<TimelineEntrySystem>, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/app/logs/system");
        if let Some(p) = params {
            builder = builder.query(p);
        }
        self.client.execute_json(builder).await
    }

    /// Global/platform activity: `GET /api/platform/logs`.
    pub async fn list_system(
        &self,
        params: Option<&[(&str, &str)]>,
    ) -> Result<ActivityLogPage<TimelineEntrySystem>, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/platform/logs");
        if let Some(p) = params {
            builder = builder.query(p);
        }
        self.client.execute_json(builder).await
    }
}
