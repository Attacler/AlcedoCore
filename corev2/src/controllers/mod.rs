pub mod apps;
pub mod auth;
pub mod collections;
pub mod developer_keys;
pub mod docs;
pub mod items;
pub mod sessions;
pub mod settings;
pub mod users;
pub mod versions;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
    },
};

pub async fn require_admin(state: &AppState, auth_level: AuthLevel) -> Result<(), AlcedoError> {
    match auth_level {
        AuthLevel::DeveloperKey { .. } => Ok(()),
        AuthLevel::User(user_id) => {
            let context = AppContext::system(RequestSource::API);
            let auth = AuthService::new(state, &context);
            if auth.is_admin(user_id).await? {
                Ok(())
            } else {
                Err(AlcedoError::Forbidden(
                    "Admin access required".to_string(),
                    0,
                ))
            }
        }
        AuthLevel::Public => Err(AlcedoError::UnAuthenticated()),
    }
}
