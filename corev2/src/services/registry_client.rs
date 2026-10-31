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

/// `http://example.com:8080/` → `example.com:8080`
pub fn authority_of(raw: &str) -> Option<&str> {
    let trimmed = raw.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let authority = without_scheme.split('/').next().unwrap_or_default();
    (!authority.is_empty()).then_some(authority)
}

pub struct HealthResult {
    pub reachable: bool,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RegistryAuth {
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl RegistryAuth {
    pub fn is_none(&self) -> bool {
        !self.auth_type.eq_ignore_ascii_case("basic")
            && !self.auth_type.eq_ignore_ascii_case("bearer")
    }

    fn credentials(&self) -> Option<(&str, &str)> {
        Some((
            self.username.as_deref()?,
            self.password.as_deref().unwrap_or_default(),
        ))
    }
}

fn basic_header(username: &str, password: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    format!(
        "Basic {}",
        STANDARD.encode(format!("{username}:{password}"))
    )
}

fn parse_bearer_challenge(header: &str) -> Option<(String, Option<String>)> {
    let trimmed = header.trim();
    let rest = trimmed
        .strip_prefix("Bearer")
        .or_else(|| trimmed.strip_prefix("bearer"))?;
    let mut realm = None;
    let mut service = None;
    for part in rest.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "realm" => realm = Some(value.to_string()),
            "service" => service = Some(value.to_string()),
            _ => {}
        }
    }
    Some((realm?, service))
}

fn scope_for(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/v2")?.trim_start_matches('/');
    if rest.is_empty() {
        return None;
    }
    if rest == "_catalog" {
        return Some("registry:catalog:*".to_string());
    }
    let name = rest.split('/').next().filter(|s| !s.is_empty())?;
    Some(format!("repository:{name}:pull"))
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn fetch_token(
    client: &RegistryClient,
    auth: &RegistryAuth,
    realm: &str,
    service: Option<&str>,
    scope: Option<&str>,
) -> Result<String, String> {
    let mut url = realm.to_string();
    let mut params: Vec<String> = Vec::new();
    if let Some(service) = service {
        params.push(format!("service={}", urlencode(service)));
    }
    if let Some(scope) = scope {
        params.push(format!("scope={}", urlencode(scope)));
    }
    if !params.is_empty() {
        let separator = if url.contains('?') { '&' } else { '?' };
        url = format!("{url}{separator}{}", params.join("&"));
    }

    let mut builder = Request::builder().method("GET").uri(&url);
    if let Some((username, password)) = auth.credentials() {
        builder = builder.header("authorization", basic_header(username, password));
    }
    let request = builder
        .body(Body::empty())
        .map_err(|e| format!("invalid token url: {e}"))?;

    let response = tokio::time::timeout(REQUEST_TIMEOUT, client.request(request))
        .await
        .map_err(|_| "token request timed out".to_string())?
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("token endpoint returned {}", response.status()));
    }

    let body = axum::body::to_bytes(Body::new(response.into_body()), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    value
        .get("token")
        .or_else(|| value.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "token response contained no token".to_string())
}

async fn send_authenticated(
    client: &RegistryClient,
    method: &str,
    url: &str,
    auth: &RegistryAuth,
    timeout: Duration,
) -> Result<AuthedResponse, String> {
    // Only the path is needed, to derive the token scope.
    let path = match url.split_once("://") {
        Some((_, rest)) => match rest.find('/') {
            Some(index) => rest[index..].to_string(),
            None => "/".to_string(),
        },
        None => "/".to_string(),
    };

    let mut builder = Request::builder().method(method).uri(url);
    if auth.auth_type.eq_ignore_ascii_case("basic")
        && let Some((username, password)) = auth.credentials()
    {
        builder = builder.header("authorization", basic_header(username, password));
    }
    let request = builder
        .body(Body::empty())
        .map_err(|e| format!("invalid url: {e}"))?;

    let response = tokio::time::timeout(timeout, client.request(request))
        .await
        .map_err(|_| "request timed out".to_string())?
        .map_err(|e| e.to_string())?;

    let challenged = response.status() == axum::http::StatusCode::UNAUTHORIZED;
    if !challenged || !auth.auth_type.eq_ignore_ascii_case("bearer") {
        let (parts, body) = response.into_parts();
        return Ok(AuthedResponse {
            status: parts.status,
            headers: parts.headers,
            body: Body::new(body),
        });
    }

    let challenge = response
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_bearer_challenge)
        .ok_or_else(|| "registry challenged without a usable Bearer realm".to_string())?;

    let token = fetch_token(
        client,
        auth,
        &challenge.0,
        challenge.1.as_deref(),
        scope_for(&path).as_deref(),
    )
    .await?;

    let retry = Request::builder()
        .method(method)
        .uri(url)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .map_err(|e| format!("invalid url: {e}"))?;
    let response = tokio::time::timeout(timeout, client.request(retry))
        .await
        .map_err(|_| "request timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let (parts, body) = response.into_parts();
    Ok(AuthedResponse {
        status: parts.status,
        headers: parts.headers,
        body: Body::new(body),
    })
}

struct AuthedResponse {
    status: axum::http::StatusCode,
    headers: axum::http::HeaderMap,
    body: Body,
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

pub async fn get_json(
    client: &RegistryClient,
    url: &str,
    auth: &RegistryAuth,
) -> Result<serde_json::Value, String> {
    let response = send_authenticated(client, "GET", url, auth, REQUEST_TIMEOUT).await?;
    if !response.status.is_success() {
        return Err(format!("HTTP {}", response.status.as_u16()));
    }
    let bytes = axum::body::to_bytes(response.body, 10 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub async fn list_tags(
    client: &RegistryClient,
    base: &str,
    repository: &str,
    auth: &RegistryAuth,
) -> Result<Vec<String>, String> {
    let url = format!("{}/v2/{}/tags/list", base, repository);
    let body = get_json(client, &url, auth).await?;
    let tags = body
        .get("tags")
        .and_then(|t| t.as_array())
        .ok_or_else(|| "registry returned no tags array".to_string())?;
    Ok(tags
        .iter()
        .filter_map(|t| t.as_str().map(str::to_string))
        .collect())
}

pub async fn manifest_size(
    client: &RegistryClient,
    base: &str,
    repository: &str,
    tag: &str,
    auth: &RegistryAuth,
) -> Option<i64> {
    let url = format!(
        "{}/v2/{}/manifests/{}",
        base.trim_end_matches('/'),
        repository,
        tag
    );
    let response = send_authenticated(client, "HEAD", &url, auth, REQUEST_TIMEOUT)
        .await
        .ok()?;
    if !response.status.is_success() {
        return None;
    }
    response
        .headers
        .get(axum::http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

pub async fn check_url(url: &str, auth: &RegistryAuth) -> HealthResult {
    let client = build_client();

    let target = normalize_registry_base(url);

    match send_authenticated(&client, "GET", &target, auth, HEALTH_TIMEOUT).await {
        Err(e) => {
            tracing::error!("[REGISTRY-HEALTH] {} unreachable: {}", target, e);
            HealthResult {
                reachable: false,
                status: Some("Registry unreachable".to_string()),
            }
        }
        Ok(response) => HealthResult {
            reachable: response.status.is_success() || response.status.is_redirection(),
            status: Some(status_text(response.status)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RegistryAuth, authority_of, normalize_registry_base, parse_bearer_challenge, scope_for,
    };

    #[test]
    fn extracts_the_bare_authority() {
        assert_eq!(
            authority_of("http://localhost:8080/"),
            Some("localhost:8080")
        );
        assert_eq!(
            authority_of("https://alcedo.example.com"),
            Some("alcedo.example.com")
        );
        assert_eq!(authority_of("registry:5000"), Some("registry:5000"));
        assert_eq!(authority_of(""), None);
    }

    #[test]
    fn parses_a_bearer_challenge() {
        let (realm, service) = parse_bearer_challenge(
            r#"Bearer realm="https://auth.example.com/token",service="registry.example.com""#,
        )
        .expect("challenge should parse");
        assert_eq!(realm, "https://auth.example.com/token");
        assert_eq!(service.as_deref(), Some("registry.example.com"));

        // A challenge with no service is still usable.
        let (realm, service) =
            parse_bearer_challenge(r#"Bearer realm="https://auth.example.com/token""#)
                .expect("realm-only challenge should parse");
        assert_eq!(realm, "https://auth.example.com/token");
        assert!(service.is_none());

        assert!(parse_bearer_challenge(r#"Basic realm="registry""#).is_none());
        assert!(parse_bearer_challenge("Bearer").is_none());
    }

    #[test]
    fn derives_the_scope_a_challenge_needs() {
        // The ping needs no scope at all.
        assert_eq!(scope_for("/v2/"), None);
        assert_eq!(scope_for("/v2"), None);
        assert_eq!(
            scope_for("/v2/_catalog").as_deref(),
            Some("registry:catalog:*")
        );
        assert_eq!(
            scope_for("/v2/hello-world-node/tags/list").as_deref(),
            Some("repository:hello-world-node:pull")
        );
        assert_eq!(scope_for("/not-v2/thing"), None);
    }

    #[test]
    fn only_basic_and_bearer_count_as_authenticated() {
        assert!(RegistryAuth::default().is_none());
        assert!(
            RegistryAuth {
                auth_type: "none".into(),
                ..Default::default()
            }
            .is_none()
        );
        assert!(
            !RegistryAuth {
                auth_type: "basic".into(),
                ..Default::default()
            }
            .is_none()
        );
        assert!(
            !RegistryAuth {
                auth_type: "bearer".into(),
                ..Default::default()
            }
            .is_none()
        );
    }

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
