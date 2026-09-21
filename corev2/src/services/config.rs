use std::env;
use std::fmt::Display;
use std::str::FromStr;

use dotenvy::dotenv;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_port: u16,
    pub listen_ip: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_log_queries: bool,
    pub session_ttl_seconds: i64,
    pub cache_strategy: String,
    pub max_login_attempts: u8,
    pub login_fatal_ttl: u16,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub session_cookie_name: String,
    pub session_cookie_path: String,
    pub session_cookie_http_only: bool,
    pub session_cookie_secure: bool,
    pub session_cookie_same_site: String,
    pub session_cookie_domain: Option<String>,
}

pub fn get_config() -> Config {
    dotenv().ok();
    let database_url = env_required("DATABASE_URL");
    let database_max_connections = env_or_msg("DATABASE_MAX_CONNECTIONS", 5u32, "must be a number");
    let listen_ip = env_str("CORE_IP", "0.0.0.0");
    let listen_port = env_or_msg("CORE_PORT", 3000u16, "must be a number");
    let session_ttl_seconds = env_or_msg("SESSION_TTL_SECONDS", 604800i64, "must be a number");
    let database_log_queries = env_or_msg("DATABASE_LOG_QUERIES", false, "must be a boolean");
    let cache_strategy = env_str("CACHE_STRATEGY", "in_memory");
    let max_login_attempts =
        env_or_msg("MAX_LOGIN_ATTEMPTS", 5u8, "must be a number (under 255)");
    let login_fatal_ttl = env_or_msg("LOGIN_FAIL_TTL", 60 * 30u16, "must be a number (under 26553555)");

    let admin_email = env_opt("ADMIN_EMAIL");
    let admin_password = env_opt("ADMIN_PASSWORD");

    let session_cookie_name = env_str("SECURITY_COOKIE_NAME", "alcedo_session");
    let session_cookie_path = env_str("SECURITY_COOKIE_PATH", "/");
    let session_cookie_http_only = env_or_msg("SECURITY_COOKIE_HTTP_ONLY", true, "must be a boolean");
    let session_cookie_secure = env_or_msg("SECURITY_COOKIE_SECURE", false, "must be a boolean");
    let session_cookie_same_site = env_str("SECURITY_COOKIE_SAMESITE", "lax");
    let session_cookie_domain = env_opt("SECURITY_COOKIE_DOMAIN");

    if session_cookie_same_site.eq_ignore_ascii_case("none") && !session_cookie_secure {
        eprintln!(
            "[CONFIG] SECURITY_COOKIE_SAMESITE=none requires SECURITY_COOKIE_SECURE=true; \
             browsers will reject the session cookie."
        );
    }

    Config {
        listen_port,
        listen_ip,
        database_url,
        database_max_connections,
        database_log_queries,
        session_ttl_seconds,
        cache_strategy,
        max_login_attempts,
        login_fatal_ttl,
        admin_email,
        admin_password,
        session_cookie_name,
        session_cookie_path,
        session_cookie_http_only,
        session_cookie_secure,
        session_cookie_same_site,
        session_cookie_domain,
    }
}

/// Reads a required environment variable, panicking with a descriptive message
/// when it is missing.
fn env_required(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("Environment variable '{}' is not defined!", key))
}

/// Reads an environment variable and parses it into `T`, falling back to
/// `default` when the variable is absent. An invalid value panics with the
/// given message.
fn env_or_msg<T>(key: &str, default: T, message: &str) -> T
where
    T: FromStr,
    T::Err: Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse::<T>()
            .unwrap_or_else(|_| panic!("{} {}", key, message)),
        Err(_) => default,
    }
}

/// Reads an environment variable as a string, falling back to `default` when
/// the variable is absent.
fn env_str(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Reads an optional, non-empty environment variable as a `String`.
fn env_opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}
