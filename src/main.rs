//! Binary entrypoint – see crate-level docs (`lib.rs`) for conceptual overview.

// =========== TESTING BELOW ===========

#![allow(unused_imports)]
#[cfg(feature = "cli")]
use config::*;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{read_to_string, remove_dir_all, remove_file};
use std::path::Path;
use std::str::FromStr;

use positorium::construct::{Database, PersistenceMode};
use positorium::error::{DatabaseError, Result};
use positorium::interface::QueryInterface;
use positorium::traqula::Engine;
use std::sync::Arc;

#[cfg(feature = "cli")]
#[tokio::main]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    if let Err(e) = real_main().await {
        eprintln!("positorium error: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "cli"))]
fn main() {
    println!("Positorium CLI is disabled. Enable the 'cli' feature to use it.");
}

#[cfg(feature = "cli")]
async fn real_main() -> Result<()> {
    let settings = Config::builder()
        .add_source(File::with_name("positorium.json"))
        .build()
        .map_err(|e| DatabaseError::Config(format!("Cannot read config: {e}")))?;
    let temp: HashMap<String, config::Value> = settings
        .try_deserialize()
        .map_err(|e| DatabaseError::Config(format!("Invalid config structure: {e}")))?;
    let settings_lookup: HashMap<String, String> =
        temp.into_iter().map(|(k, v)| (k, v.to_string())).collect();
    let database_file_and_path = settings_lookup
        .get("database_file_and_path")
        .ok_or_else(|| DatabaseError::Config("Missing 'database_file_and_path'".into()))?;
    let enable_persistence = setting(&settings_lookup, "enable_persistence")?.unwrap_or(true);
    let recreate_database_on_startup =
        setting(&settings_lookup, "recreate_database_on_startup")?.unwrap_or(false);
    if recreate_database_on_startup {
        let target = Path::new(database_file_and_path);
        if database_file_and_path.is_empty()
            || matches!(database_file_and_path.as_str(), "/" | "." | "..")
        {
            return Err(DatabaseError::Config(format!(
                "refusing to recreate unsafe store path '{database_file_and_path}'"
            )));
        }
        let removal = if target.is_dir() {
            remove_dir_all(target)
        } else {
            remove_file(target)
        };
        match removal {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => {
                return Err(DatabaseError::Config(format!(
                    "Could not remove store '{database_file_and_path}': {e}"
                )));
            }
        }
    }
    let mode = if enable_persistence {
        println!(
            "Using file-backed persistence at '{}'.",
            database_file_and_path
        );
        PersistenceMode::File(database_file_and_path.clone())
    } else {
        println!("Persistence disabled (ephemeral in-memory engine).");
        PersistenceMode::InMemory
    };
    let engine_db = Database::new(mode)?;
    let db = Arc::new(engine_db);
    let interface = Arc::new(QueryInterface::new(Arc::clone(&db)));
    let traqula_file_to_run_on_startup = settings_lookup
        .get("traqula_file_to_run_on_startup")
        .ok_or_else(|| DatabaseError::Config("Missing 'traqula_file_to_run_on_startup'".into()))?;
    println!(
        "Traqula file to run on startup: {}",
        traqula_file_to_run_on_startup
    );
    let traqula_content = read_to_string(traqula_file_to_run_on_startup)
        .map_err(|e| DatabaseError::Config(format!("Could not read traqula file: {e}")))?;
    // Quietly execute the startup script (suppressing any search result row printing)
    {
        let engine = Engine::new(db.as_ref());
        engine
            .execute_collect(&traqula_content)
            .map_err(|error| DatabaseError::Execution(format!("startup script failed: {error}")))?;
    }
    // Derive listen interface & port (optional in config)
    let listen_interface = settings_lookup
        .get("listen_interface")
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1");
    let listen_port: u16 = setting(&settings_lookup, "listen_port")?.unwrap_or(8080);
    let addr: std::net::SocketAddr = format!("{}:{}", listen_interface, listen_port)
        .parse()
        .map_err(|e| {
            DatabaseError::Config(format!(
                "Invalid listen address {listen_interface}:{listen_port} – {e}"
            ))
        })?;
    let mut server_config = positorium::server::ServerConfig::default();
    if let Some(value) = setting(&settings_lookup, "http_request_body_bytes")? {
        server_config.request_body_bytes = value;
    }
    if let Some(value) = setting(&settings_lookup, "http_default_timeout_ms")? {
        server_config.default_timeout = std::time::Duration::from_millis(value);
    }
    if let Some(origins) = settings_lookup.get("cors_allowed_origins") {
        for origin in origins.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            server_config = server_config.allow_loopback_origin(origin)?;
        }
    }
    if !addr.ip().is_loopback() {
        tracing::warn!(
            ?addr,
            "non-loopback HTTP binding is still a trusted interface without authentication"
        );
    }
    let app = positorium::server::router_with_config(Arc::clone(&interface), server_config)?;
    tracing::info!(?addr, "HTTP server listening");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| DatabaseError::Execution(format!("bind error: {e}")))?;
    let serve_result = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| DatabaseError::Execution(format!("server error: {e}")));
    let flush_result = db.flush();
    serve_result?;
    flush_result
}

#[cfg(feature = "cli")]
fn setting<T>(settings: &HashMap<String, String>, key: &str) -> Result<Option<T>>
where
    T: FromStr,
    T::Err: Display,
{
    settings
        .get(key)
        .map(|value| {
            value.parse::<T>().map_err(|error| {
                DatabaseError::Config(format!("Invalid '{key}' value '{value}': {error}"))
            })
        })
        .transpose()
}

#[cfg(feature = "cli")]
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        match terminate {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to install SIGTERM handler; waiting for Ctrl-C");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(all(test, feature = "cli"))]
mod tests {
    use super::*;

    #[test]
    fn setting_distinguishes_missing_valid_and_invalid_values() {
        let mut settings = HashMap::new();
        assert_eq!(setting::<u16>(&settings, "port").unwrap(), None);

        settings.insert("port".to_string(), "8080".to_string());
        assert_eq!(setting::<u16>(&settings, "port").unwrap(), Some(8080));

        settings.insert("port".to_string(), "not-a-port".to_string());
        let error = setting::<u16>(&settings, "port").unwrap_err();
        assert!(matches!(error, DatabaseError::Config(_)));
        assert!(error.to_string().contains("not-a-port"));
    }
}
