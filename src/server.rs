use crate::error::DatabaseError;
use crate::interface::QueryInterface;
use crate::traqula::{
    CancellationToken, CollectedResultSet, ExecutionOptions, ExecutionWarning,
    MultiStreamCallbacks, RowSink, SinkFlow, script_counts,
};
use axum::extract::{DefaultBodyLimit, State, rejection::JsonRejection};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{info, warn};

pub const HTTP_API_VERSION: &str = "v1";
pub const DEFAULT_REQUEST_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_COMMANDS: usize = 1_000;
pub const DEFAULT_MAX_ROWS_PER_SEARCH: usize = 100_000;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub request_body_bytes: usize,
    pub default_timeout: Duration,
    pub max_commands: usize,
    pub max_rows_per_search: usize,
    allowed_origins: Vec<HeaderValue>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            request_body_bytes: DEFAULT_REQUEST_BODY_BYTES,
            default_timeout: DEFAULT_TIMEOUT,
            max_commands: DEFAULT_MAX_COMMANDS,
            max_rows_per_search: DEFAULT_MAX_ROWS_PER_SEARCH,
            allowed_origins: Vec::new(),
        }
    }
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), DatabaseError> {
        if self.request_body_bytes == 0 || self.request_body_bytes > MAX_REQUEST_BODY_BYTES {
            return Err(DatabaseError::Config(format!(
                "HTTP request body limit must be between 1 and {MAX_REQUEST_BODY_BYTES} bytes"
            )));
        }
        if self.default_timeout.is_zero() || self.default_timeout > MAX_TIMEOUT {
            return Err(DatabaseError::Config(format!(
                "HTTP default timeout must be between 1 ms and {} ms",
                MAX_TIMEOUT.as_millis()
            )));
        }
        if self.max_commands == 0 || self.max_commands > DEFAULT_MAX_COMMANDS {
            return Err(DatabaseError::Config(format!(
                "HTTP command limit must be between 1 and {DEFAULT_MAX_COMMANDS}"
            )));
        }
        if self.max_rows_per_search == 0 || self.max_rows_per_search > DEFAULT_MAX_ROWS_PER_SEARCH {
            return Err(DatabaseError::Config(format!(
                "HTTP row limit must be between 1 and {DEFAULT_MAX_ROWS_PER_SEARCH}"
            )));
        }
        Ok(())
    }

    /// Allow an exact browser origin. Only loopback HTTP(S) origins are accepted.
    pub fn allow_loopback_origin(mut self, origin: &str) -> Result<Self, DatabaseError> {
        if !is_exact_loopback_origin(origin) {
            return Err(DatabaseError::Config(format!(
                "CORS origin must be an exact loopback HTTP(S) origin: {origin}"
            )));
        }
        let value = HeaderValue::from_str(origin)
            .map_err(|error| DatabaseError::Config(format!("invalid CORS origin: {error}")))?;
        self.allowed_origins.push(value);
        Ok(self)
    }
}

fn is_exact_loopback_origin(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    if authority.is_empty()
        || authority.contains('/')
        || authority.contains('?')
        || authority.contains('#')
        || authority.contains('@')
    {
        return false;
    }
    let (host, port) = if authority.starts_with('[') {
        let Some(end) = authority.find(']') else {
            return false;
        };
        let host = &authority[..=end];
        let suffix = &authority[end + 1..];
        if suffix.is_empty() {
            (host, None)
        } else if let Some(port) = suffix.strip_prefix(':') {
            (host, Some(port))
        } else {
            return false;
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
        && port.is_none_or(|value| !value.is_empty() && value.parse::<u16>().is_ok())
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub script: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub api_version: &'static str,
    pub id: u64,
    pub status: String,
    pub elapsed_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<ExecutionWarning>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_types: Option<Vec<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_sets: Option<Vec<MultiResultSet>>,
}

#[derive(Serialize)]
pub struct MultiResultSet {
    pub columns: Vec<String>,
    pub row_types: Vec<Vec<String>>,
    pub row_count: usize,
    pub limited: bool,
    pub rows: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

#[derive(Clone)]
struct ServerState {
    interface: Arc<QueryInterface>,
    config: ServerConfig,
}

pub fn router(interface: Arc<QueryInterface>) -> Router {
    match router_with_config(interface, ServerConfig::default()) {
        Ok(router) => router,
        Err(error) => panic!("default server configuration is invalid: {error}"),
    }
}

pub fn router_with_config(
    interface: Arc<QueryInterface>,
    config: ServerConfig,
) -> Result<Router, DatabaseError> {
    config.validate()?;
    let mut cors = CorsLayer::new()
        .allow_methods([Method::POST])
        .allow_headers([header::CONTENT_TYPE]);
    if !config.allowed_origins.is_empty() {
        cors = cors.allow_origin(AllowOrigin::list(config.allowed_origins.clone()));
    }
    let body_limit = config.request_body_bytes;
    Ok(Router::new()
        .route("/v1/query", post(query))
        .with_state(ServerState { interface, config })
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(cors))
}

async fn query(
    State(state): State<ServerState>,
    payload: Result<Json<QueryRequest>, JsonRejection>,
) -> Response {
    let started = Instant::now();
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => return error_response(error.status(), started, error.body_text()),
    };
    let (command_count, search_count) = match script_counts(&request.script) {
        Ok(counts) => counts,
        Err(error) => return database_error_response(error, started),
    };
    if command_count > state.config.max_commands {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            started,
            format!(
                "script contains {command_count} commands; maximum is {}",
                state.config.max_commands
            ),
        );
    }
    let timeout = request
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(state.config.default_timeout)
        .min(state.config.default_timeout);
    if timeout.is_zero() {
        return error_response(
            StatusCode::BAD_REQUEST,
            started,
            "timeout_ms must be positive",
        );
    }
    let cancellation = CancellationToken::new();
    let options = ExecutionOptions {
        timeout: Some(timeout),
        cancellation: Some(cancellation.clone()),
        max_rows_per_search: Some(state.config.max_rows_per_search),
        ..ExecutionOptions::default()
    };

    if request.stream && search_count > 0 {
        return streaming_response(
            state.interface,
            request.script,
            search_count,
            options,
            cancellation,
        );
    }

    let interface = state.interface;
    let script = request.script;
    let rows_result = tokio::task::spawn_blocking(move || {
        if search_count > 1 {
            interface
                .execute_collect_multi_with_options(&script, options)
                .map(BufferedResult::Multi)
        } else {
            interface
                .execute_collect_with_options(&script, options)
                .map(BufferedResult::Single)
        }
    })
    .await;
    match rows_result {
        Ok(Ok(BufferedResult::Single(result))) => {
            info!(
                rows = result.row_count,
                limited = result.limited,
                "query complete"
            );
            json_response(
                StatusCode::OK,
                QueryResponse {
                    api_version: HTTP_API_VERSION,
                    id: 0,
                    status: "ok".into(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    warnings: Some(result.metadata.warnings),
                    columns: Some(result.columns),
                    row_types: Some(result.row_types),
                    row_count: Some(result.row_count),
                    limited: Some(result.limited),
                    rows: Some(result.rows),
                    error: None,
                    result_sets: None,
                },
            )
        }
        Ok(Ok(BufferedResult::Multi(result_sets))) => {
            let total_rows = result_sets.iter().map(|set| set.row_count).sum();
            let warnings = result_sets
                .first()
                .map(|set| set.metadata.warnings.clone())
                .unwrap_or_default();
            let result_sets = result_sets.into_iter().map(MultiResultSet::from).collect();
            json_response(
                StatusCode::OK,
                QueryResponse {
                    api_version: HTTP_API_VERSION,
                    id: 0,
                    status: "ok".into(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    warnings: Some(warnings),
                    columns: None,
                    row_types: None,
                    row_count: Some(total_rows),
                    limited: None,
                    rows: None,
                    error: None,
                    result_sets: Some(result_sets),
                },
            )
        }
        Ok(Err(error)) => database_error_response(error, started),
        Err(error) => {
            warn!(%error, "query worker failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                started,
                "query worker failed",
            )
        }
    }
}

enum BufferedResult {
    Single(crate::traqula::CollectedResult),
    Multi(Vec<CollectedResultSet>),
}

impl From<CollectedResultSet> for MultiResultSet {
    fn from(result: CollectedResultSet) -> Self {
        Self {
            columns: result.columns,
            row_types: result.row_types,
            row_count: result.row_count,
            limited: result.limited,
            rows: result.rows,
            search: result.search,
        }
    }
}

fn streaming_response(
    interface: Arc<QueryInterface>,
    script: String,
    search_count: usize,
    options: ExecutionOptions,
    cancellation: CancellationToken,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(128);
    tokio::task::spawn_blocking(move || {
        if search_count == 1 {
            let mut sink = StreamingSink {
                tx: tx.clone(),
                cancellation,
                rows: 0,
            };
            match interface.execute_stream_single_with_options(&script, &mut sink, options) {
                Ok((_columns, limited, row_count)) => {
                    let event = serde_json::json!({
                        "version": 1,
                        "event": "end",
                        "row_count": row_count,
                        "limited": limited
                    });
                    let _ = tx.blocking_send(format!("data: {event}\n\n"));
                }
                Err(error) => send_stream_error(&tx, error),
            }
        } else {
            let mut callbacks = StreamingCallbacks {
                tx: tx.clone(),
                cancellation,
                total_rows: 0,
            };
            match interface.execute_stream_multi_with_options(&script, &mut callbacks, options) {
                Ok(()) => {
                    let event = serde_json::json!({
                        "version": 1,
                        "event": "end",
                        "result_sets": search_count,
                        "total_rows": callbacks.total_rows
                    });
                    let _ = tx.blocking_send(format!("data: {event}\n\n"));
                }
                Err(error) => send_stream_error(&tx, error),
            }
        }
    });
    let body = tokio_stream::wrappers::ReceiverStream::new(rx)
        .map(|chunk| Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(axum::body::Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

struct StreamingSink {
    tx: tokio::sync::mpsc::Sender<String>,
    cancellation: CancellationToken,
    rows: usize,
}

impl RowSink for StreamingSink {
    fn on_warning(&mut self, warning: &ExecutionWarning) -> SinkFlow {
        let event = serde_json::json!({
            "version": 1,
            "event": "warning",
            "warning": warning
        });
        self.send(event)
    }

    fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
        let event = serde_json::json!({
            "version": 1,
            "event": "meta",
            "columns": columns
        });
        self.send(event)
    }

    fn push(&mut self, row: Vec<String>, types: Vec<String>) -> SinkFlow {
        let event = serde_json::json!({
            "version": 1,
            "event": "row",
            "row": row,
            "types": types
        });
        let flow = self.send(event);
        if matches!(flow, SinkFlow::Continue) {
            self.rows += 1;
        }
        flow
    }
}

impl StreamingSink {
    fn send(&self, event: serde_json::Value) -> SinkFlow {
        if self.tx.blocking_send(format!("data: {event}\n\n")).is_ok() {
            SinkFlow::Continue
        } else {
            self.cancellation.cancel();
            SinkFlow::Stop
        }
    }
}

struct StreamingCallbacks {
    tx: tokio::sync::mpsc::Sender<String>,
    cancellation: CancellationToken,
    total_rows: usize,
}

impl MultiStreamCallbacks for StreamingCallbacks {
    fn on_warning(&mut self, warning: &ExecutionWarning) {
        let event = serde_json::json!({
            "version": 1,
            "event": "warning",
            "warning": warning
        });
        self.send(event);
    }

    fn on_result_set_start(&mut self, index: usize, columns: &[String], search: &str) {
        let event = serde_json::json!({
            "version": 1,
            "event": "result_set_start",
            "index": index,
            "columns": columns,
            "search": search
        });
        self.send(event);
    }

    fn on_row(&mut self, index: usize, row: Vec<String>, types: Vec<String>) -> bool {
        let event = serde_json::json!({
            "version": 1,
            "event": "row",
            "index": index,
            "row": row,
            "types": types
        });
        if self.send(event) {
            self.total_rows += 1;
            true
        } else {
            false
        }
    }

    fn on_result_set_end(&mut self, index: usize, row_count: usize, limited: bool) {
        let event = serde_json::json!({
            "version": 1,
            "event": "result_set_end",
            "index": index,
            "row_count": row_count,
            "limited": limited
        });
        self.send(event);
    }
}

impl StreamingCallbacks {
    fn send(&self, event: serde_json::Value) -> bool {
        if self.tx.blocking_send(format!("data: {event}\n\n")).is_ok() {
            true
        } else {
            self.cancellation.cancel();
            false
        }
    }
}

fn send_stream_error(tx: &tokio::sync::mpsc::Sender<String>, error: DatabaseError) {
    warn!(%error, "streaming query failed");
    let event = serde_json::json!({
        "version": 1,
        "event": "error",
        "error": error.to_string()
    });
    let _ = tx.blocking_send(format!("data: {event}\n\n"));
    let end = serde_json::json!({
        "version": 1,
        "event": "end",
        "status": "error"
    });
    let _ = tx.blocking_send(format!("data: {end}\n\n"));
}

fn database_error_response(error: DatabaseError, started: Instant) -> Response {
    let status = match error {
        DatabaseError::Parse { .. }
        | DatabaseError::UnknownRole(_)
        | DatabaseError::InvalidAppearanceSet(_)
        | DatabaseError::Execution(_) => StatusCode::BAD_REQUEST,
        DatabaseError::Cancelled | DatabaseError::Timeout => StatusCode::REQUEST_TIMEOUT,
        DatabaseError::Config(_)
        | DatabaseError::Persistence(_)
        | DatabaseError::DataCorruption { .. }
        | DatabaseError::Invariant(_)
        | DatabaseError::Lock(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, started, error.to_string())
}

fn error_response(status: StatusCode, started: Instant, error: impl Into<String>) -> Response {
    json_response(
        status,
        QueryResponse {
            api_version: HTTP_API_VERSION,
            id: 0,
            status: "error".into(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            warnings: None,
            columns: None,
            row_types: None,
            row_count: None,
            limited: None,
            rows: None,
            error: Some(error.into()),
            result_sets: None,
        },
    )
}

fn json_response(status: StatusCode, body: QueryResponse) -> Response {
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::{Database, PersistenceMode};

    #[test]
    fn only_exact_loopback_origins_are_accepted() {
        for origin in [
            "http://localhost:8080",
            "https://127.0.0.1",
            "http://[::1]:3000",
        ] {
            assert!(is_exact_loopback_origin(origin), "{origin}");
        }
        for origin in [
            "*",
            "http://example.com",
            "http://localhost.evil",
            "http://localhost/path",
            "http://user@localhost",
        ] {
            assert!(!is_exact_loopback_origin(origin), "{origin}");
        }
    }

    #[test]
    fn configuration_enforces_hard_caps() {
        let too_large = ServerConfig {
            request_body_bytes: MAX_REQUEST_BODY_BYTES + 1,
            ..ServerConfig::default()
        };
        assert!(too_large.validate().is_err());
        let too_slow = ServerConfig {
            default_timeout: MAX_TIMEOUT + Duration::from_millis(1),
            ..ServerConfig::default()
        };
        assert!(too_slow.validate().is_err());
    }

    #[test]
    fn command_count_uses_the_grammar_not_literal_semicolons() {
        let script = "add role label; add posit [{(+thing, label)}, \"a;b\", @NOW];";
        assert_eq!(script_counts(script).unwrap(), (2, 0));
    }

    #[tokio::test]
    async fn validation_errors_keep_their_http_status() {
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(Arc::new(
                Database::new(PersistenceMode::InMemory).unwrap(),
            ))),
            config: ServerConfig::default(),
        };
        let response = query(
            State(state),
            Ok(Json(QueryRequest {
                script: "not traqula".into(),
                stream: false,
                timeout_ms: None,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
