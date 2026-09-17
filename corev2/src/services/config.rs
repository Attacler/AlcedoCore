use std::env;

use dotenvy::dotenv;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_port: i16,
    pub listen_ip: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_log_queries: bool,
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
        .map(|v| v.parse::<i16>().expect("CORE_PORT must be a number"))
        .unwrap_or(3000);

    let database_log_queries = env::var("DATABASE_LOG_QUERIES")
        .map(|v| {
            v.parse::<bool>()
                .expect("DATABASE_LOG_QUERIES must be a boolean")
        })
        .unwrap_or(false);

    Config {
        listen_port,
        listen_ip,
        database_url,
        database_max_connections,
        database_log_queries,
    }
}
