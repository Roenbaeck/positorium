//! Binary entrypoint – see crate-level docs (`lib.rs`) for conceptual overview.

// =========== TESTING BELOW ===========

#![allow(unused_imports)]
#[cfg(feature = "cli")]
use config::*;
use std::collections::HashMap;
use std::fs::{read_to_string, remove_dir_all, remove_file};
use std::path::Path;

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
    let enable_persistence = settings_lookup
        .get("enable_persistence")
        .map(|v| v == "true")
        .unwrap_or(true);
    let recreate_database_on_startup = settings_lookup
        .get("recreate_database_on_startup")
        .map(|v| v == "true")
        .unwrap_or(false);
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
                println!(
                    "Could not remove the store '{}': {}",
                    database_file_and_path, e
                );
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
        match engine.execute_collect(&traqula_content) {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error=%e, "Startup script execution error");
            }
        }
    }
    // Derive listen interface & port (optional in config)
    let listen_interface = settings_lookup
        .get("listen_interface")
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1");
    let listen_port: u16 = settings_lookup
        .get("listen_port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr: std::net::SocketAddr = format!("{}:{}", listen_interface, listen_port)
        .parse()
        .map_err(|e| {
            DatabaseError::Config(format!(
                "Invalid listen address {listen_interface}:{listen_port} – {e}"
            ))
        })?;
    let mut server_config = positorium::server::ServerConfig::default();
    if let Some(value) = settings_lookup
        .get("http_request_body_bytes")
        .and_then(|value| value.parse::<usize>().ok())
    {
        server_config.request_body_bytes = value;
    }
    if let Some(value) = settings_lookup
        .get("http_default_timeout_ms")
        .and_then(|value| value.parse::<u64>().ok())
    {
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
    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|e| DatabaseError::Execution(format!("server error: {e}")))?;
    db.flush()?;
    Ok(())
}
