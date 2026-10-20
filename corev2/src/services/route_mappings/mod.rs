//! Route mappings service: CRUD over `alcedocore_route_mappings` plus shared
//! cache synchronization.
//!
//! The active mapping set is serialized to the shared cache under
//! `alcedocore:routemap:data` (JSON) with a version counter at
//! `alcedocore:routemap:version` (INCR'd on every write). Each replica
//! middleware polls the version key on a staleness window and reloads when
//! it changes.

pub mod matcher;

use serde_json::Value;
use sqlx::Row;

use crate::{
    AppState,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        route_mappings::matcher::{
            RouteMap, RouteMapping, is_reserved_prefix, normalize_domain, normalize_path_prefix,
        },
    },
};

const COLLECTION: &str = "alcedocore_route_mappings";
const CACHE_DATA_KEY: &str = "alcedocore:routemap:data";
const CACHE_VERSION_KEY: &str = "alcedocore:routemap:version";

/// A route mapping row as returned by the API.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RouteMappingResponse {
    pub id: i32,
    pub domain: Option<String>,
    pub path_prefix: Option<String>,
    pub install_id: i64,
    pub is_active: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    /// Resolved install metadata (joined in for the list endpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_version: Option<String>,
    /// Computed effective URL preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_url: Option<String>,
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateRouteMappingRequest {
    pub domain: Option<String>,
    pub path_prefix: Option<String>,
    pub install_id: i64,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct UpdateRouteMappingRequest {
    pub domain: Option<Option<String>>,
    pub path_prefix: Option<Option<String>>,
    pub install_id: Option<i64>,
    pub is_active: Option<bool>,
}

pub struct RouteMappingsService<'a> {
    state: &'a AppState,
}

fn system_context() -> AppContext {
    AppContext::system(RequestSource::API)
}

fn row_to_response(row: &serde_json::Map<String, Value>) -> RouteMappingResponse {
    let str_field = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_string);
    let opt_str_field = |key: &str| row.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let domain = opt_str_field("domain");
    let path_prefix = opt_str_field("path_prefix");
    let effective_url = compute_effective_url(domain.as_deref(), path_prefix.as_deref());

    RouteMappingResponse {
        id: row.get("id").and_then(Value::as_i64).unwrap_or(0) as i32,
        domain,
        path_prefix,
        install_id: row.get("install_id").and_then(Value::as_i64).unwrap_or(0),
        is_active: row
            .get("is_active")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        created_at: str_field("created_at"),
        updated_at: str_field("updated_at"),
        install_slug: opt_str_field("install_slug"),
        install_app: opt_str_field("install_app"),
        install_version: opt_str_field("install_version"),
        effective_url,
    }
}

fn compute_effective_url(domain: Option<&str>, path_prefix: Option<&str>) -> Option<String> {
    match (domain, path_prefix) {
        (Some(d), Some(p)) => Some(format!("https://{}{}", d, p)),
        (Some(d), None) => Some(format!("https://{}", d)),
        (None, Some(p)) => Some(p.to_string()),
        (None, None) => None,
    }
}

impl<'a> RouteMappingsService<'a> {
    pub fn new(state: &'a AppState) -> Self {
        RouteMappingsService { state }
    }

    pub fn validate_and_normalize(
        domain: Option<&str>,
        path_prefix: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), AlcedoError> {
        let norm_domain = domain.and_then(|d| {
            let trimmed = d.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let norm_prefix = path_prefix.and_then(|p| {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        if norm_domain.is_none() && norm_prefix.is_none() {
            return Err(AlcedoError::InvalidInput(
                "At least one of domain or path_prefix must be set".to_string(),
                0,
            ));
        }

        let normalized_domain = match &norm_domain {
            Some(d) => match normalize_domain(d) {
                Some(nd) => Some(nd),
                None => {
                    return Err(AlcedoError::InvalidInput(
                        format!(
                            "Invalid domain: '{}'. Must be a valid FQDN without scheme or port.",
                            d
                        ),
                        0,
                    ));
                }
            },
            None => None,
        };

        let normalized_prefix = match &norm_prefix {
            Some(p) => {
                if is_reserved_prefix(p) {
                    return Err(AlcedoError::InvalidInput(
                        format!(
                            "Path prefix '{}' collides with a reserved platform prefix (api, p, health)",
                            p
                        ),
                        0,
                    ));
                }
                match normalize_path_prefix(p) {
                    Some(np) => Some(np),
                    None => {
                        return Err(AlcedoError::InvalidInput(
                            format!("Invalid path prefix: '{}'", p),
                            0,
                        ));
                    }
                }
            }
            None => None,
        };

        Ok((normalized_domain, normalized_prefix))
    }

    async fn require_install(&self, install_id: i64) -> Result<(), AlcedoError> {
        let row: Option<(i32, bool)> = sqlx::query_as(
            "SELECT id, enabled FROM alcedocore.alcedocore_plugins_installs WHERE id = $1::int4",
        )
        .bind(install_id as i32)
        .fetch_optional(self.state.database_pool.as_ref())
        .await?;

        match row {
            Some((id, _enabled)) => {
                let _ = id;
                Ok(())
            }
            None => Err(AlcedoError::NotFound(
                format!("Plugin install not found: {}", install_id),
                0,
            )),
        }
    }

    /// Check for overlapping mappings (same domain + same path_prefix).
    async fn check_overlap(
        &self,
        domain: Option<&str>,
        path_prefix: Option<&str>,
        exclude_id: Option<i32>,
    ) -> Result<(), AlcedoError> {
        let mut sql = String::from(
            "SELECT id FROM alcedocore.alcedocore_route_mappings WHERE domain IS NOT DISTINCT FROM $1 AND path_prefix IS NOT DISTINCT FROM $2",
        );
        if exclude_id.is_some() {
            sql.push_str(" AND id != $3");
        }
        sql.push_str(" LIMIT 1");

        let mut query = sqlx::query(&sql).bind(domain).bind(path_prefix);
        if let Some(id) = exclude_id {
            query = query.bind(id);
        }

        let row = query
            .fetch_optional(self.state.database_pool.as_ref())
            .await?;
        if row.is_some() {
            return Err(AlcedoError::InvalidInput(
                format!(
                    "A mapping with domain={:?} and path_prefix={:?} already exists",
                    domain, path_prefix
                ),
                0,
            ));
        }
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<RouteMappingResponse>, AlcedoError> {
        let context = system_context();
        let collection = COLLECTION.to_string();
        let service = ItemsService::new(self.state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                sort: vec!["id".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?;

        let install_ids: Vec<i64> = rows
            .iter()
            .filter_map(|r| r.get("install_id").and_then(Value::as_i64))
            .collect();

        let install_meta = self.fetch_install_metadata(&install_ids).await;

        let mut result = Vec::new();
        for row in &rows {
            let mut resp = row_to_response(row);
            if let Some(meta) = install_meta.get(&resp.install_id) {
                resp.install_slug = Some(meta.0.clone());
                resp.install_app = meta.1.clone();
                resp.install_version = meta.2.clone();
            }
            result.push(resp);
        }
        Ok(result)
    }

    async fn fetch_install_metadata(
        &self,
        install_ids: &[i64],
    ) -> std::collections::HashMap<i64, (String, Option<String>, Option<String>)> {
        if install_ids.is_empty() {
            return std::collections::HashMap::new();
        }
        let ids_i32: Vec<i32> = install_ids.iter().map(|&id| id as i32).collect();

        let rows = sqlx::query(
            "SELECT i.id, p.slug, a.api_name, v.version_name \
             FROM alcedocore.alcedocore_plugins_installs i \
             JOIN alcedocore.alcedocore_plugins p ON p.id = i.plugin_id \
             JOIN alcedocore.alcedocore_apps_versions av ON av.id = i.app_version_id \
             LEFT JOIN alcedocore.alcedocore_apps a ON a.id = av.app_id \
             LEFT JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE i.id = ANY($1)",
        )
        .bind(&ids_i32)
        .fetch_all(self.state.database_pool.as_ref())
        .await
        .unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let id: i32 = row.try_get("id").ok()?;
                let slug: String = row.try_get("slug").ok()?;
                let app: Option<String> = row.try_get("api_name").ok().flatten();
                let version: Option<String> = row.try_get("version_name").ok().flatten();
                Some((i64::from(id), (slug, app, version)))
            })
            .collect()
    }

    pub async fn create(
        &self,
        req: CreateRouteMappingRequest,
    ) -> Result<RouteMappingResponse, AlcedoError> {
        let (domain, path_prefix) =
            Self::validate_and_normalize(req.domain.as_deref(), req.path_prefix.as_deref())?;

        self.require_install(req.install_id).await?;
        self.check_overlap(domain.as_deref(), path_prefix.as_deref(), None)
            .await?;

        let context = system_context();
        let collection = COLLECTION.to_string();
        let mut service = ItemsService::new(self.state, &context, &collection);

        let mut map = serde_json::Map::new();
        if let Some(ref d) = domain {
            map.insert("domain".to_string(), Value::String(d.clone()));
        } else {
            map.insert("domain".to_string(), Value::Null);
        }
        if let Some(ref p) = path_prefix {
            map.insert("path_prefix".to_string(), Value::String(p.clone()));
        } else {
            map.insert("path_prefix".to_string(), Value::Null);
        }
        map.insert("install_id".to_string(), Value::from(req.install_id as i32));
        map.insert("is_active".to_string(), Value::from(req.is_active));

        let created = service.create_many(vec![map], &mut None).await?;
        let id = created
            .first()
            .and_then(|pk| pk.parse::<i32>().ok())
            .ok_or_else(|| {
                AlcedoError::SystemError("Route mapping insert returned no id".to_string(), 0)
            })?;

        self.sync_cache().await?;

        let row = self.fetch_by_id(id).await?;
        Ok(row_to_response(&row))
    }

    pub async fn update(
        &self,
        id: i32,
        req: UpdateRouteMappingRequest,
    ) -> Result<RouteMappingResponse, AlcedoError> {
        let existing = self.fetch_by_id(id).await?;
        let existing_domain = existing
            .get("domain")
            .and_then(Value::as_str)
            .map(str::to_string);
        let existing_prefix = existing
            .get("path_prefix")
            .and_then(Value::as_str)
            .map(str::to_string);
        let existing_install = existing
            .get("install_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let final_domain = match &req.domain {
            Some(Some(d)) => {
                let trimmed = d.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Some(None) => None,
            None => existing_domain.clone(),
        };
        let final_prefix = match &req.path_prefix {
            Some(Some(p)) => {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Some(None) => None,
            None => existing_prefix.clone(),
        };
        let final_install = req.install_id.unwrap_or(existing_install);

        let (norm_domain, norm_prefix) =
            Self::validate_and_normalize(final_domain.as_deref(), final_prefix.as_deref())?;

        if final_install != existing_install {
            self.require_install(final_install).await?;
        }
        self.check_overlap(norm_domain.as_deref(), norm_prefix.as_deref(), Some(id))
            .await?;

        let context = system_context();
        let collection = COLLECTION.to_string();
        let mut service = ItemsService::new(self.state, &context, &collection);

        let mut map = serde_json::Map::new();
        map.insert(
            "domain".to_string(),
            match &norm_domain {
                Some(d) => Value::String(d.clone()),
                None => Value::Null,
            },
        );
        map.insert(
            "path_prefix".to_string(),
            match &norm_prefix {
                Some(p) => Value::String(p.clone()),
                None => Value::Null,
            },
        );
        map.insert("install_id".to_string(), Value::from(final_install as i32));
        if let Some(active) = req.is_active {
            map.insert("is_active".to_string(), Value::from(active));
        }
        map.insert(
            "updated_at".to_string(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );

        service
            .update_items_by_query(&mut Query::eq("id", Value::from(id)), map, &mut None)
            .await?;

        self.sync_cache().await?;

        let row = self.fetch_by_id(id).await?;
        Ok(row_to_response(&row))
    }

    pub async fn delete(&self, id: i32) -> Result<(), AlcedoError> {
        let _ = self.fetch_by_id(id).await?;

        let context = system_context();
        let collection = COLLECTION.to_string();
        let mut service = ItemsService::new(self.state, &context, &collection);
        service
            .delete_items_by_query(Query::eq("id", Value::from(id)), &mut None)
            .await?;

        self.sync_cache().await?;
        Ok(())
    }

    async fn fetch_by_id(&self, id: i32) -> Result<serde_json::Map<String, Value>, AlcedoError> {
        let context = system_context();
        let collection = COLLECTION.to_string();
        let service = ItemsService::new(self.state, &context, &collection);

        service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 1,
                ..Query::eq("id", Value::from(id))
            })
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AlcedoError::NotFound(format!("Route mapping not found: {}", id), 0))
    }

    pub async fn sync_cache(&self) -> Result<(), AlcedoError> {
        let context = system_context();
        let collection = COLLECTION.to_string();
        let service = ItemsService::new(self.state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("is_active", Value::Bool(true))
            })
            .await?;

        let mappings: Vec<RouteMapping> = rows
            .iter()
            .filter_map(|row| {
                Some(RouteMapping {
                    id: row.get("id").and_then(Value::as_i64)? as i32,
                    domain: row
                        .get("domain")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    path_prefix: row
                        .get("path_prefix")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    install_id: row.get("install_id").and_then(Value::as_i64)?,
                })
            })
            .collect();

        let json = serde_json::to_string(&mappings).map_err(|e| {
            AlcedoError::SystemError(format!("Failed to serialize route mappings: {}", e), 0)
        })?;

        // Write data first, then bump version (version is the commit marker).
        self.state
            .cache
            .set(CACHE_DATA_KEY.to_string(), json)
            .await?;
        self.state.cache.incr_by(CACHE_VERSION_KEY, 1).await?;

        Ok(())
    }
}
