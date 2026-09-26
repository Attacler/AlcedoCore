use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

pub const USERS_COLLECTION: &str = "alcedo_users";

const MIN_PASSWORD_LEN: usize = 8;

fn system_context(collection: &str) -> (AppContext, String) {
    (
        AppContext::system(RequestSource::API),
        collection.to_string(),
    )
}

fn without_secret(mut user: Map<String, Value>) -> Map<String, Value> {
    user.remove("password_hash");
    user
}

/// All `alcedo_users` reads and writes live here so the controller only deals
/// with HTTP concerns (auth level, path/body parsing, response envelope).
pub struct UsersService<'a> {
    app_state: &'a AppState,
}

impl UsersService<'_> {
    pub fn new(state: &AppState) -> UsersService<'_> {
        UsersService { app_state: state }
    }

    pub async fn is_admin(&self, user_id: Uuid) -> Result<bool, AlcedoError> {
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        let row = service
            .get_single_item_by_pk(Value::String(user_id.to_string()))
            .await?;
        Ok(row
            .and_then(|row| row.get("is_admin").and_then(Value::as_bool))
            .unwrap_or(false))
    }

    pub async fn list(&self, query: Query) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        Ok(service
            .read_items_by_query(query)
            .await?
            .into_iter()
            .map(without_secret)
            .collect())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self.raw_get(id).await?.map(without_secret))
    }

    pub async fn create(
        &self,
        email: &str,
        password: &str,
        display_name: Option<String>,
        is_admin: bool,
    ) -> Result<Map<String, Value>, AlcedoError> {
        let email = normalize_email(email)?;
        validate_password(password)?;
        if self.email_taken(&email, None).await? {
            return Err(AlcedoError::InvalidInput(
                "A user with this email already exists".to_string(),
                0,
            ));
        }

        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);

        let password_hash = AuthService::hash_password(password).await?;
        let id = Uuid::new_v4();
        let mut user = item_map! {
            "id" => id.to_string(),
            "email" => email,
            "password_hash" => password_hash,
            "is_admin" => is_admin,
        };
        if let Some(display_name) = display_name {
            user.insert("display_name".to_string(), json!(display_name));
        }

        service.create_many(vec![user], &mut None).await?;

        let created = service
            .get_single_item_by_pk(json!(id.to_string()))
            .await?
            .ok_or_else(|| AlcedoError::SystemError("User was not created".to_string(), 0))?;

        Ok(without_secret(created))
    }

    pub async fn update(
        &self,
        id: Uuid,
        email: Option<String>,
        display_name: Option<String>,
        is_admin: Option<bool>,
    ) -> Result<Map<String, Value>, AlcedoError> {
        let mut update = Map::new();
        if let Some(email) = email {
            let email = normalize_email(&email)?;
            if self.email_taken(&email, Some(id)).await? {
                return Err(AlcedoError::InvalidInput(
                    "A user with this email already exists".to_string(),
                    0,
                ));
            }
            update.insert("email".to_string(), json!(email));
        }
        if let Some(display_name) = display_name {
            update.insert("display_name".to_string(), json!(display_name));
        }
        if let Some(is_admin) = is_admin {
            update.insert("is_admin".to_string(), json!(is_admin));
        }

        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);

        if !update.is_empty() {
            let mut query = Query::eq("id", json!(id.to_string()));
            service
                .update_items_by_query(&mut query, update, &mut None)
                .await?;
        }

        service
            .get_single_item_by_pk(json!(id.to_string()))
            .await?
            .map(without_secret)
            .ok_or_else(|| AlcedoError::NotFound("User not found".to_string(), 0))
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool, AlcedoError> {
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        let deleted = service
            .delete_items_by_pks(vec![json!(id.to_string())], None)
            .await?;
        Ok(deleted > 0)
    }

    /// Verifies `current_password` when one is supplied (self-service change),
    /// then stores the new hash. Admins resetting another user pass `None`.
    pub async fn change_password(
        &self,
        id: Uuid,
        current_password: Option<&str>,
        new_password: &str,
    ) -> Result<(), AlcedoError> {
        validate_password(new_password)?;

        if let Some(current) = current_password {
            let user = self
                .raw_get(id)
                .await?
                .ok_or_else(|| AlcedoError::NotFound("User not found".to_string(), 0))?;
            let hash = user
                .get("password_hash")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !AuthService::verify_password(current, hash).await? {
                return Err(AlcedoError::InvalidInput(
                    "Current password is incorrect".to_string(),
                    0,
                ));
            }
        }

        let password_hash = AuthService::hash_password(new_password).await?;
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        let mut update = Map::new();
        update.insert("password_hash".to_string(), json!(password_hash));
        let mut query = Query::eq("id", json!(id.to_string()));
        let updated = service
            .update_items_by_query(&mut query, update, &mut None)
            .await?;
        if updated.is_empty() {
            return Err(AlcedoError::NotFound("User not found".to_string(), 0));
        }
        Ok(())
    }

    async fn raw_get(&self, id: Uuid) -> Result<Option<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        service
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await
    }

    async fn email_taken(&self, email: &str, exclude: Option<Uuid>) -> Result<bool, AlcedoError> {
        let (context, collection) = system_context(USERS_COLLECTION);
        let service = ItemsService::new(self.app_state, &context, &collection);
        let mut query = Query::eq("email", json!(email));
        query.fields = vec!["id".to_string()];
        query.limit = 1;
        let found = service.read_items_by_query(query).await?;
        let found_id = found
            .first()
            .and_then(|row| row.get("id"))
            .and_then(Value::as_str)
            .map(String::from);
        match found_id {
            None => Ok(false),
            Some(id) => Ok(Some(id) != exclude.map(|e| e.to_string())),
        }
    }
}

fn normalize_email(email: &str) -> Result<String, AlcedoError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "Email is required".to_string(),
            0,
        ));
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), AlcedoError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AlcedoError::InvalidInput(
            format!("Password must be at least {} characters", MIN_PASSWORD_LEN),
            0,
        ));
    }
    Ok(())
}
