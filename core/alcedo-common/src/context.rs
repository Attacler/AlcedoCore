use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RequestSource {
    API,
    FirstMigration,
    Migration,
    SystemTest,
    Inspector,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppContext {
    pub app_name: String,
    pub version: String,
    pub request_source: RequestSource,
}

impl AppContext {
    pub fn schema_name(&self) -> String {
        if self.version.len() == 0 {
            return format!("{}", slugify(&self.app_name));
        }

        return format!("{}010{}", slugify(&self.app_name), slugify(&self.version));
    }

    pub fn app_api_name(&self) -> String {
        slugify(&self.app_name)
    }
    pub fn version_api_name(&self) -> String {
        slugify(&self.version)
    }
}
fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| {
            if &c.to_string() == "$" {
                '_'
            } else if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// pub struct ExtractContext(pub AppContext);

// impl<S> FromRequestParts<S> for ExtractContext
// where
//     S: Send + Sync,
// {
//     type Rejection = Response;

//     async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
//         let app = parts.headers.get("x-app");
//         let version = parts.headers.get("x-version");

//         if let None = app {
//             return Err((
//                 StatusCode::BAD_REQUEST,
//                 Json(json!({ "error": "No app provided" })),
//             )
//                 .into_response());
//         }
//         if let None = version {
//             return Err((
//                 StatusCode::BAD_REQUEST,
//                 Json(json!({ "error": "No version provided" })),
//             )
//                 .into_response());
//         }

//         Ok(ExtractContext(AppContext {
//             app_name: app.unwrap().to_str().unwrap().to_string(),
//             version: version.unwrap().to_str().unwrap().to_string(),
//             request_source: RequestSource::API,
//         }))
//     }
// }
