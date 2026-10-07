use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Another crate in the tree enables rustls' `aws-lc-rs` feature, so rustls can
/// no longer auto-select a provider. Pin `ring` once, before the first handshake.
static INSTALL_CRYPTO_PROVIDER: std::sync::Once = std::sync::Once::new();

fn install_crypto_provider() {
    INSTALL_CRYPTO_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub type RegistryClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Body>;

pub fn build_client() -> RegistryClient {
    install_crypto_provider();
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

/// `localhost:5000` → `http://localhost:5000`; leaves existing schemes alone.
pub fn normalize_registry_base(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

pub struct HealthResult {
    pub reachable: bool,
    pub status: Option<String>,
}

fn status_text(status: axum::http::StatusCode) -> String {
    let reachable = status.is_success() || status.is_redirection();
    format!(
        "{} {}",
        status.as_u16(),
        status
            .canonical_reason()
            .unwrap_or(if reachable { "OK" } else { "Error" })
    )
}

pub async fn get_json(client: &RegistryClient, url: &str) -> Result<serde_json::Value, String> {
    let request = Request::builder()
        .method("GET")
        .uri(url)
        .body(Body::empty())
        .map_err(|e| format!("invalid url: {}", e))?;

    let response = tokio::time::timeout(REQUEST_TIMEOUT, client.request(request))
        .await
        .map_err(|_| "request timed out".to_string())?
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }

    let body = Body::new(response.into_body());
    let bytes = axum::body::to_bytes(body, 10 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub async fn check_url(url: &str) -> HealthResult {
    let client = build_client();

    let request = match Request::builder()
        .method("GET")
        .uri(url)
        .body(Body::empty())
    {
        Ok(request) => request,
        Err(e) => {
            return HealthResult {
                reachable: false,
                status: Some(format!("Invalid URL: {}", e)),
            };
        }
    };

    match tokio::time::timeout(HEALTH_TIMEOUT, client.request(request)).await {
        Err(_) => HealthResult {
            reachable: false,
            status: Some("Request timed out".to_string()),
        },
        Ok(Err(e)) => HealthResult {
            reachable: false,
            status: Some(e.to_string()),
        },
        Ok(Ok(response)) => {
            let status = response.status();
            HealthResult {
                reachable: status.is_success() || status.is_redirection(),
                status: Some(status_text(status)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_registry_base;

    #[test]
    fn normalizes_registry_base() {
        assert_eq!(
            normalize_registry_base("localhost:5000"),
            "http://localhost:5000"
        );
        assert_eq!(
            normalize_registry_base("http://localhost:5000/"),
            "http://localhost:5000"
        );
        assert_eq!(
            normalize_registry_base("https://reg.example.com"),
            "https://reg.example.com"
        );
    }
}
