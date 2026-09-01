use crate::error::DatabaseError;
use crate::interface::QueryInterface;
use crate::terrain::{
    DEFAULT_MAX_RELATIONSHIP_SIGNATURES, DEFAULT_PROJECTED_ROLE_LIMIT, TERRAIN_VERSION,
    TerrainOptions, TerrainReport,
};
use crate::traqula::{
    CancellationToken, CollectedResultSet, ExecutionOptions, ExecutionParameter, ExecutionWarning,
    MultiStreamCallbacks, ResultCell, RowSink, SinkFlow, TRAQULA_VERSION, parse_time_with_now,
    script_counts,
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
pub const SSE_SCHEMA_VERSION: u16 = 1;
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
    pub traqula_version: u16,
    pub script: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub parameters: std::collections::HashMap<String, ExecutionParameter>,
}

#[derive(Debug, Deserialize)]
pub struct TerrainRequest {
    pub terrain_version: u16,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub projected_role_limit: Option<usize>,
    #[serde(default)]
    pub max_relationship_signatures: Option<usize>,
}

#[derive(Serialize)]
pub struct TerrainResponse {
    pub api_version: &'static str,
    pub terrain_version: u16,
    pub status: String,
    pub elapsed_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<TerrainReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub api_version: &'static str,
    pub traqula_version: u16,
    pub id: u64,
    pub status: String,
    pub elapsed_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<ExecutionWarning>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<ResultCell>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_sets: Option<Vec<MultiResultSet>>,
}

#[derive(Serialize)]
pub struct MultiResultSet {
    pub columns: Vec<String>,
    pub row_count: usize,
    pub limited: bool,
    pub rows: Vec<Vec<ResultCell>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

#[derive(Clone)]
struct ServerState {
    interface: Arc<QueryInterface>,
    config: ServerConfig,
}

pub fn router(interface: Arc<QueryInterface>) -> Result<Router, DatabaseError> {
    router_with_config(interface, ServerConfig::default())
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
        .route("/v1/terrain", post(terrain))
        .with_state(ServerState { interface, config })
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(cors))
}

async fn terrain(
    State(state): State<ServerState>,
    payload: Result<Json<TerrainRequest>, JsonRejection>,
) -> Response {
    let started = Instant::now();
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => {
            return terrain_error_response(error.status(), started, error.body_text());
        }
    };
    if request.terrain_version != TERRAIN_VERSION {
        return terrain_error_response(
            StatusCode::BAD_REQUEST,
            started,
            format!(
                "unsupported Terrain version {}; supported version is {TERRAIN_VERSION}",
                request.terrain_version
            ),
        );
    }
    let timeout = request
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(state.config.default_timeout)
        .min(MAX_TIMEOUT);
    if timeout.is_zero() {
        return terrain_error_response(
            StatusCode::BAD_REQUEST,
            started,
            "timeout_ms must be positive",
        );
    }
    let resolved_now = crate::datatype::Time::new();
    let as_of_token = request.as_of.as_deref().unwrap_or("@NOW");
    let Some(as_of) = parse_time_with_now(as_of_token, &resolved_now) else {
        return terrain_error_response(
            StatusCode::BAD_REQUEST,
            started,
            format!("invalid Terrain as_of token '{as_of_token}'"),
        );
    };
    let cancellation = CancellationToken::new();
    let _cancel_on_drop = CancelOnDrop(cancellation.clone());
    let interface = state.interface;
    let options = TerrainOptions {
        as_of: Some(as_of),
        timeout: Some(timeout),
        cancellation: Some(cancellation),
        projected_role_limit: request
            .projected_role_limit
            .unwrap_or(DEFAULT_PROJECTED_ROLE_LIMIT),
        max_relationship_signatures: request
            .max_relationship_signatures
            .unwrap_or(DEFAULT_MAX_RELATIONSHIP_SIGNATURES),
        ..TerrainOptions::default()
    };
    match tokio::task::spawn_blocking(move || interface.terrain_with_options(options)).await {
        Ok(Ok(report)) => terrain_json_response(
            StatusCode::OK,
            TerrainResponse {
                api_version: HTTP_API_VERSION,
                terrain_version: TERRAIN_VERSION,
                status: "ok".into(),
                elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                report: Some(report),
                error: None,
            },
        ),
        Ok(Err(error)) => terrain_database_error_response(error, started),
        Err(error) => {
            warn!(%error, "Terrain worker failed");
            terrain_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                started,
                "Terrain worker failed",
            )
        }
    }
}

struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
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
    if request.traqula_version != TRAQULA_VERSION {
        return error_response(
            StatusCode::BAD_REQUEST,
            started,
            format!(
                "unsupported Traqula version {}; supported version is {TRAQULA_VERSION}",
                request.traqula_version
            ),
        );
    }
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
        parameters: request.parameters,
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
                    traqula_version: TRAQULA_VERSION,
                    id: 0,
                    status: "ok".into(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    warnings: Some(result.metadata.warnings),
                    columns: Some(result.columns),
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
                    traqula_version: TRAQULA_VERSION,
                    id: 0,
                    status: "ok".into(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    warnings: Some(warnings),
                    columns: None,
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
                        "version": SSE_SCHEMA_VERSION,
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
                        "version": SSE_SCHEMA_VERSION,
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
            "version": SSE_SCHEMA_VERSION,
            "event": "warning",
            "warning": warning
        });
        self.send(event)
    }

    fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
        let event = serde_json::json!({
            "version": SSE_SCHEMA_VERSION,
            "event": "meta",
            "columns": columns
        });
        self.send(event)
    }

    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
        let event = serde_json::json!({
            "version": SSE_SCHEMA_VERSION,
            "event": "row",
            "row": row
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
            "version": SSE_SCHEMA_VERSION,
            "event": "warning",
            "warning": warning
        });
        self.send(event);
    }

    fn on_result_set_start(&mut self, index: usize, columns: &[String], search: &str) {
        let event = serde_json::json!({
            "version": SSE_SCHEMA_VERSION,
            "event": "result_set_start",
            "index": index,
            "columns": columns,
            "search": search
        });
        self.send(event);
    }

    fn on_row(&mut self, index: usize, row: Vec<ResultCell>) -> bool {
        let event = serde_json::json!({
            "version": SSE_SCHEMA_VERSION,
            "event": "row",
            "index": index,
            "row": row
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
            "version": SSE_SCHEMA_VERSION,
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
        "version": SSE_SCHEMA_VERSION,
        "event": "error",
        "error": error.to_string()
    });
    let _ = tx.blocking_send(format!("data: {event}\n\n"));
    let end = serde_json::json!({
        "version": SSE_SCHEMA_VERSION,
        "event": "end",
        "status": "error"
    });
    let _ = tx.blocking_send(format!("data: {end}\n\n"));
}

fn database_error_response(error: DatabaseError, started: Instant) -> Response {
    let status = database_error_status(&error);
    error_response(status, started, error.to_string())
}

fn database_error_status(error: &DatabaseError) -> StatusCode {
    match error {
        DatabaseError::Parse { .. }
        | DatabaseError::UnknownRole(_)
        | DatabaseError::InvalidAppearanceSet(_)
        | DatabaseError::UnknownVariable(_)
        | DatabaseError::VariableDomain { .. }
        | DatabaseError::InvalidRecall(_)
        | DatabaseError::Comparison(_)
        | DatabaseError::Parameter(_)
        | DatabaseError::Execution(_) => StatusCode::BAD_REQUEST,
        DatabaseError::ResourceLimit { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        DatabaseError::Cancelled | DatabaseError::Timeout => StatusCode::REQUEST_TIMEOUT,
        DatabaseError::Config(_)
        | DatabaseError::Persistence(_)
        | DatabaseError::DataCorruption { .. }
        | DatabaseError::Invariant(_)
        | DatabaseError::Lock(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn terrain_database_error_response(error: DatabaseError, started: Instant) -> Response {
    terrain_error_response(database_error_status(&error), started, error.to_string())
}

fn terrain_error_response(
    status: StatusCode,
    started: Instant,
    error: impl Into<String>,
) -> Response {
    terrain_json_response(
        status,
        TerrainResponse {
            api_version: HTTP_API_VERSION,
            terrain_version: TERRAIN_VERSION,
            status: "error".into(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            report: None,
            error: Some(error.into()),
        },
    )
}

fn terrain_json_response(status: StatusCode, body: TerrainResponse) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn error_response(status: StatusCode, started: Instant, error: impl Into<String>) -> Response {
    json_response(
        status,
        QueryResponse {
            api_version: HTTP_API_VERSION,
            traqula_version: TRAQULA_VERSION,
            id: 0,
            status: "error".into(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            warnings: None,
            columns: None,
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

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

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

    #[test]
    fn only_searches_with_return_create_result_sets() {
        let script = r#"
            search [{(?thing, label), ...}, "a", *]
            or add posit [{(+thing, label)}, "a", @NOW];
            search [{(?thing, label), ...}, ?value, *] return ?thing, ?value;
        "#;
        assert_eq!(script_counts(script).unwrap(), (2, 1));
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
                traqula_version: TRAQULA_VERSION,
                script: "not traqula".into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unsupported_traqula_versions_are_rejected_before_execution() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(Arc::clone(&database))),
            config: ServerConfig::default(),
        };
        let response = query(
            State(state),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION + 1,
                script: "add role never_created;".into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!database.contains_role("never_created").unwrap());
    }

    #[tokio::test]
    async fn cross_request_recall_is_a_typed_error_and_does_not_poison_the_server() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(Arc::clone(&database))),
            config: ServerConfig::default(),
        };

        let first = query(
            State(state.clone()),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION,
                script: r#"add role name; add posit [{(+alice, name)}, "Alice", '2024-01-01'];"#
                    .into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);

        let invalid_recall = query(
            State(state.clone()),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION,
                script: r#"add posit [{(alice, name)}, "Alice Updated", '2025-01-01'];"#.into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        let invalid_status = invalid_recall.status();
        let body = json_body(invalid_recall).await;
        assert_eq!(invalid_status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("not bound in a prior command")
        );

        let healthy_follow_up = query(
            State(state),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION,
                script: "search [{(?person, name)}, ?value, *] return ?value;".into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        assert_eq!(healthy_follow_up.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_or_add_http_requests_publish_nothing_in_either_data_state() {
        const INVALID_SCRIPT: &str = r#"
            search [{(?registry, registry_code), ...}, ?code, *]
            return ?registry, ?code
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
        "#;

        for matched in [false, true] {
            let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
            let engine = crate::traqula::Engine::new(&database);
            engine.execute("add role registry_code;").unwrap();
            if matched {
                engine
                    .execute("add posit [{(+existing, registry_code)}, \"CODE\", '2024-01-01'];")
                    .unwrap();
            }
            let before = engine
                .execute_collect(
                    "search [{(?registry, registry_code), ...}, *, *] return ?registry;",
                )
                .unwrap()
                .row_count;
            let state = ServerState {
                interface: Arc::new(QueryInterface::new(Arc::clone(&database))),
                config: ServerConfig::default(),
            };

            let buffered = query(
                State(state.clone()),
                Ok(Json(QueryRequest {
                    traqula_version: TRAQULA_VERSION,
                    script: INVALID_SCRIPT.into(),
                    stream: false,
                    timeout_ms: None,
                    parameters: std::collections::HashMap::new(),
                })),
            )
            .await;
            assert_eq!(buffered.status(), StatusCode::BAD_REQUEST);
            let body = json_body(buffered).await;
            assert_eq!(body["status"], "error");
            assert!(body.get("rows").is_none() || body["rows"].is_null());
            assert!(
                body["error"]
                    .as_str()
                    .unwrap()
                    .contains("fallback cannot supply")
            );

            let streamed = query(
                State(state),
                Ok(Json(QueryRequest {
                    traqula_version: TRAQULA_VERSION,
                    script: INVALID_SCRIPT.into(),
                    stream: true,
                    timeout_ms: None,
                    parameters: std::collections::HashMap::new(),
                })),
            )
            .await;
            assert_eq!(streamed.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(streamed.into_body(), usize::MAX)
                .await
                .unwrap();
            let events = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(events.contains(r#""event":"error""#), "{events}");
            assert!(!events.contains(r#""event":"meta""#), "{events}");
            assert!(!events.contains(r#""event":"row""#), "{events}");
            assert!(
                !events.contains(r#""event":"result_set_start""#),
                "{events}"
            );

            let after = crate::traqula::Engine::new(&database)
                .execute_collect(
                    "search [{(?registry, registry_code), ...}, *, *] return ?registry;",
                )
                .unwrap()
                .row_count;
            assert_eq!(after, before);
        }
    }

    #[tokio::test]
    async fn information_in_effect_is_identical_in_buffered_and_sse_queries() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        crate::traqula::Engine::new(&database)
            .execute(
                "add role status; \
                 add posit +target [{(+case, status)}, \"open\", '2024-01-01']; \
                 add posit [{(target, posit), (+source, ascertains)}, 80%, '2024-02-01'];",
            )
            .unwrap();
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(database)),
            config: ServerConfig::default(),
        };
        let script = "search [{(?case, status)}, ?state, *] \
                      in effect '2025-01-01', '2025-01-01' \
                      return ?state;";

        let buffered = query(
            State(state.clone()),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION,
                script: script.into(),
                stream: false,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        assert_eq!(buffered.status(), StatusCode::OK);
        let body = json_body(buffered).await;
        assert_eq!(body["rows"][0][0]["text"], "\"open\"");

        let streamed = query(
            State(state),
            Ok(Json(QueryRequest {
                traqula_version: TRAQULA_VERSION,
                script: script.into(),
                stream: true,
                timeout_ms: None,
                parameters: std::collections::HashMap::new(),
            })),
        )
        .await;
        assert_eq!(streamed.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(streamed.into_body(), usize::MAX)
            .await
            .unwrap();
        let events = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(events.contains(r#""text":"\"open\"""#), "{events}");
        assert!(events.contains(r#""event":"end""#), "{events}");
    }

    #[tokio::test]
    async fn terrain_rejects_unknown_versions_before_waiting_for_owner() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(Arc::clone(&database))),
            config: ServerConfig::default(),
        };
        let held_database = Arc::clone(&database);
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _owner = held_database.execution_owner.lock().unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.recv().unwrap();
        let response = terrain(
            State(state),
            Ok(Json(TerrainRequest {
                terrain_version: TERRAIN_VERSION + 1,
                as_of: None,
                timeout_ms: None,
                projected_role_limit: None,
                max_relationship_signatures: None,
            })),
        )
        .await;
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }

    #[tokio::test]
    async fn terrain_parses_cutoffs_clamps_timeout_and_returns_no_store() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        crate::traqula::Engine::new(&database)
            .execute("add role name; add posit [{(+person, name)}, \"Ada\", '2024-01-01'];")
            .unwrap();
        let expected = database
            .terrain_with_options(TerrainOptions {
                as_of: Some(crate::datatype::Time::new_year_month_from("2025-06").unwrap()),
                timeout: Some(MAX_TIMEOUT),
                ..TerrainOptions::default()
            })
            .unwrap();
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(Arc::clone(&database))),
            config: ServerConfig::default(),
        };
        let response = terrain(
            State(state),
            Ok(Json(TerrainRequest {
                terrain_version: TERRAIN_VERSION,
                as_of: Some("'2025-06'".into()),
                timeout_ms: Some(MAX_TIMEOUT.as_millis() as u64 + 10_000),
                projected_role_limit: None,
                max_relationship_signatures: None,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let body = json_body(response).await;
        assert_eq!(body["api_version"], "v1");
        assert_eq!(body["terrain_version"], TERRAIN_VERSION);
        assert_eq!(body["report"]["resolved_as_of"], "'2025-06'");
        assert_eq!(body["report"], serde_json::to_value(expected).unwrap());
    }

    #[tokio::test]
    async fn terrain_uses_structured_resource_limit_errors() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let state = ServerState {
            interface: Arc::new(QueryInterface::new(database)),
            config: ServerConfig::default(),
        };
        let response = terrain(
            State(state),
            Ok(Json(TerrainRequest {
                terrain_version: TERRAIN_VERSION,
                as_of: None,
                timeout_ms: None,
                projected_role_limit: Some(crate::terrain::MAX_PROJECTED_ROLE_LIMIT + 1),
                max_relationship_signatures: None,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = json_body(response).await;
        assert_eq!(body["status"], "error");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("projected attribute Roles")
        );
        assert!(body.get("report").is_none());
    }
}
