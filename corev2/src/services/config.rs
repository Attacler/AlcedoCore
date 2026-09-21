use std::env;

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
}

pub fn get_config() -> Config {
    dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").expect("Environment variable 'DATABASE_URL' is not defined!");
    let database_max_connections = env::var("DATABASE_MAX_CONNECTIONS")
        .map(|v| {
            v.parse::<u32>()
                .expect("DATABASE_MAX_CONNECTIONS must be a number")
        })
        .unwrap_or(5);

    let listen_ip = env::var("CORE_IP").unwrap_or_else(|_| "0.0.0.0".to_string());

    let listen_port = env::var("CORE_PORT")
        .map(|v| v.parse::<u16>().expect("CORE_PORT must be a number"))
        .unwrap_or(3000);
    let session_ttl_seconds = env::var("SESSSION_TTL_SECONDS")
        .map(|v| {
            v.parse::<i64>()
                .expect("SESSSION_TTL_SECONDS must be a number")
        })
        .unwrap_or(604800);

    let database_log_queries = env::var("DATABASE_LOG_QUERIES")
        .map(|v| {
            v.parse::<bool>()
                .expect("DATABASE_LOG_QUERIES must be a boolean")
        })
        .unwrap_or(false);

    let cache_strategy = env::var("CACHE_STRATEGY")
        .map(|v| {
            v.parse::<String>()
                .expect("CACHE_STRATEGY must be a string")
        })
        .unwrap_or("in_memory".to_string());
    let max_login_attempts = env::var("MAX_LOGIN_ATTEMPTS")
        .map(|v| {
            v.parse::<u8>()
                .expect("MAX_LOGIN_ATTEMPTS must be a number (under 255)")
        })
        .unwrap_or(5);
    let login_fatal_ttl = env::var("LOGIN_FAIL_TTL")
        .map(|v| {
            v.parse::<u16>()
                .expect("LOGIN_FAIL_TTL must be a number (under 26553555)")
        })
        .unwrap_or(60 * 30);

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
    }
}
