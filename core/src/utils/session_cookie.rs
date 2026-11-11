use axum::http::{HeaderMap, header};

use crate::services::config::Config;

pub fn read_session_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    for cookie in cookie::Cookie::split_parse(cookie_header) {
        if let Ok(cookie) = cookie {
            if cookie.name() == cookie_name {
                return Some(cookie.value().to_string());
            }
        }
    }

    None
}

fn parse_same_site(value: &str) -> cookie::SameSite {
    match value.to_ascii_lowercase().as_str() {
        "lax" => cookie::SameSite::Lax,
        "none" => cookie::SameSite::None,
        _ => cookie::SameSite::Strict,
    }
}

fn configured_cookie<'c>(
    name: &'c str,
    value: String,
    config: &Config,
) -> cookie::CookieBuilder<'c> {
    let mut builder = cookie::Cookie::build((name, value))
        .path(config.session_cookie_path.clone())
        .http_only(config.session_cookie_http_only)
        .secure(config.session_cookie_secure)
        .same_site(parse_same_site(&config.session_cookie_same_site));

    if let Some(domain) = &config.session_cookie_domain {
        builder = builder.domain(domain.clone());
    }

    builder
}

pub fn build_session_cookie(value: &str, max_age_seconds: i64, config: &Config) -> String {
    configured_cookie(&config.session_cookie_name, value.to_string(), config)
        .max_age(time::Duration::seconds(max_age_seconds))
        .build()
        .to_string()
}

pub fn clear_session_cookie(config: &Config) -> String {
    configured_cookie(&config.session_cookie_name, String::new(), config)
        .max_age(time::Duration::seconds(0))
        .build()
        .to_string()
}
