use std::sync::Arc;
use axum::{routing::post, Router, Json};
use futures_util::StreamExt;
use axum::http::header;
use tower_http::cors::{CorsLayer, Any};
use serde::{Deserialize, Serialize};
use axum::http::StatusCode;
use tracing::{info, warn};
use crate::interface::QueryInterface;
use crate::traqula::{Engine, CollectedResultSet, RowSink, SinkFlow, MultiStreamCallbacks};

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
    pub id: u64,
    pub status: String,
    pub elapsed_ms: f64,
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

pub fn router(interface: Arc<QueryInterface>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([axum::http::Method::POST])
        .allow_headers(Any);
    Router::new()
        .route("/v1/query", post(move |Json(req): Json<QueryRequest>| {
        let iface = Arc::clone(&interface);
        async move {
            // We run the query in a blocking thread since Engine is synchronous today.
            let started = std::time::Instant::now();
            let script = req.script.clone();
            let do_stream = req.stream;
            let _timeout = req.timeout_ms.map(std::time::Duration::from_millis);
            if do_stream {
                // Attempt streaming if exactly one 'search' token (tokenized) appears; else fall back.
                let search_count = script
                    .split(|c: char| c.is_whitespace() || c==';' || c==',')
                    .filter(|t| *t == "search")
                    .count();
                if search_count == 1 {
                    info!(target: "positorium::server", event="stream_start", "starting streaming execution");
                    let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
                    tokio::task::spawn_blocking(move || {
                        let engine = Engine::new(iface.database());
                        struct StreamingSink { tx: tokio::sync::mpsc::Sender<String>, limited: bool, rows: usize }
                        impl RowSink for StreamingSink {
                            fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
                                let meta = serde_json::json!({"event":"meta","columns": columns, "row_types": [], "row_count": 0, "limited": false});
                                if self.tx.blocking_send(format!("data: {}\n\n", meta)).is_err() { return SinkFlow::Stop; }
                                SinkFlow::Continue
                            }
                            fn push(&mut self, row: Vec<String>, types: Vec<String>) -> SinkFlow {
                                let ev = serde_json::json!({"event":"row","row": row, "types": types});
                                if self.tx.blocking_send(format!("data: {}\n\n", ev)).is_err() { return SinkFlow::Stop; }
                                self.rows +=1; SinkFlow::Continue
                            }
                        }
                        let mut sink = StreamingSink { tx: tx.clone(), limited:false, rows:0 };
                        match engine.execute_stream_single(&script, &mut sink) {
                            Ok((_cols, limited, row_count)) => {
                                sink.limited = limited; sink.rows = row_count; // ensure final values
                                let end = serde_json::json!({"event":"end","row_count": row_count, "limited": limited});
                                let _ = tx.blocking_send(format!("data: {}\n\n", end));
                                info!(target: "positorium::server", event="stream_complete", rows=row_count, limited=limited, "streaming execution finished");
                            }
                            Err(e) => {
                                let err = serde_json::json!({"event":"error","error": format!("{}", e)});
                                let _ = tx.blocking_send(format!("data: {}\n\n", err));
                                let _ = tx.blocking_send("data: {\"event\":\"end\"}\n\n".to_string());
                                warn!(target: "positorium::server", error=%e, event="stream_error", "streaming execution error");
                            }
                        }
                    });
                    let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx)
                        .map(|chunk| Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)));
                    let response = axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::CONNECTION, "keep-alive")
                        .body(axum::body::Body::from_stream(rx_stream))
                        .unwrap();
                    return Ok::<_, (StatusCode, &'static str)>((StatusCode::OK, response));
                } else if search_count > 1 {
                    info!(target: "positorium::server", event="stream_start_multi", searches=search_count, "starting multi-search streaming execution");
                    let (tx, rx) = tokio::sync::mpsc::channel::<String>(128);
                    tokio::task::spawn_blocking(move || {
                        let engine = Engine::new(iface.database());
                        struct MultiCb { tx: tokio::sync::mpsc::Sender<String>, total_rows: usize }
                        impl MultiStreamCallbacks for MultiCb {
                            fn on_result_set_start(&mut self, set_index: usize, columns: &[String], search_text: &str) { let ev=serde_json::json!({"event":"result_set_start","index": set_index, "columns": columns, "search": search_text}); let _=self.tx.blocking_send(format!("data: {}\n\n", ev)); }
                            fn on_row(&mut self, set_index: usize, row: Vec<String>, types: Vec<String>) -> bool { self.total_rows+=1; let ev=serde_json::json!({"event":"row","index": set_index, "row": row, "types": types}); self.tx.blocking_send(format!("data: {}\n\n", ev)).is_ok() }
                            fn on_result_set_end(&mut self, set_index: usize, row_count: usize, limited: bool) { let ev=serde_json::json!({"event":"result_set_end","index": set_index, "row_count": row_count, "limited": limited}); let _=self.tx.blocking_send(format!("data: {}\n\n", ev)); }
                        }
                        let mut cb = MultiCb { tx: tx.clone(), total_rows: 0 };
                        match engine.execute_stream_multi(&script, &mut cb) {
                            Ok(()) => { let end=serde_json::json!({"event":"multi_end","total_rows": cb.total_rows}); let _=tx.blocking_send(format!("data: {}\n\n", end)); let _=tx.blocking_send("data: {\"event\":\"end\"}\n\n".to_string()); info!(target: "positorium::server", event="stream_complete_multi", total_rows=cb.total_rows, "multi-search streaming finished"); },
                            Err(e) => { let err=serde_json::json!({"event":"error","error": format!("{}", e)}); let _=tx.blocking_send(format!("data: {}\n\n", err)); let _=tx.blocking_send("data: {\"event\":\"multi_end\"}\n\n".to_string()); let _=tx.blocking_send("data: {\"event\":\"end\"}\n\n".to_string()); warn!(target: "positorium::server", error=%e, event="stream_error_multi", "multi-search streaming error"); }
                        }
                    });
                    let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx)
                        .map(|chunk| Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)));
                    let response = axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::CONNECTION, "keep-alive")
                        .body(axum::body::Body::from_stream(rx_stream))
                        .unwrap();
                    return Ok::<_, (StatusCode, &'static str)>((StatusCode::OK, response));
                }
                // Else fall through to normal non-stream path if no searches
            }
            let rows_result = tokio::task::spawn_blocking(move || {
                let engine = Engine::new(iface.database());
                let search_count = script.matches("search ").count();
                if search_count > 1 {
                    match engine.execute_collect_multi(&script) {
                        Ok(multi) => Ok::<Result<_, _>, _>(Err(multi)), // Err variant inside Ok signifies multi
                        Err(e) => Err(e),
                    }
                } else {
                    engine.execute_collect(&script).map(|single| Ok(single))
                }
            }).await.map_err(|e| {
                warn!(error=%e, "Join error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Join error")
            })?;
            let total_elapsed = started.elapsed();
            let elapsed_ms_f64 = total_elapsed.as_secs_f64() * 1000.0;
            let (status, body_json) = match rows_result {
                Ok(Ok(result)) => {
                    info!(ms=elapsed_ms_f64, rows=result.row_count, limited=result.limited, "query complete");
                    let body = QueryResponse { id: 0, status: "ok".into(), elapsed_ms: elapsed_ms_f64, columns: Some(result.columns), row_types: Some(result.row_types), row_count: Some(result.row_count), limited: Some(result.limited), rows: Some(result.rows), error: None, result_sets: None };
                    (StatusCode::OK, serde_json::to_string(&body).unwrap())
                }
                Ok(Err(multi_sets)) => {
                    let total_rows: usize = multi_sets.iter().map(|m| m.row_count).sum();
                    info!(ms=elapsed_ms_f64, total_rows, searches=multi_sets.len(), "multi-search complete");
                    let result_sets: Vec<MultiResultSet> = multi_sets.into_iter().map(|m: CollectedResultSet| MultiResultSet { columns: m.columns, row_types: m.row_types, row_count: m.row_count, limited: m.limited, rows: m.rows, search: m.search }).collect();
                    let body = QueryResponse { id: 0, status: "ok".into(), elapsed_ms: elapsed_ms_f64, columns: None, row_types: None, row_count: Some(total_rows), limited: None, rows: None, error: None, result_sets: Some(result_sets) };
                    (StatusCode::OK, serde_json::to_string(&body).unwrap())
                }
                Err(e) => {
                    let is_parse = matches!(e, crate::error::DatabaseError::Parse { .. });
                    let status = if is_parse { StatusCode::BAD_REQUEST } else { StatusCode::INTERNAL_SERVER_ERROR };
                    let msg = format!("{e}");
                    warn!(%msg, code=%status.as_u16(), "query error");
                    let body = QueryResponse { id: 0, status: "error".into(), elapsed_ms: elapsed_ms_f64, columns: None, row_types: None, row_count: None, limited: None, rows: None, error: Some(msg), result_sets: None };
                    (status, serde_json::to_string(&body).unwrap())
                }
            };
            let response = axum::response::Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(body_json))
                .unwrap();
            Ok::<_, (StatusCode, &'static str)>((StatusCode::OK, response))
        }
    }))
    .layer(cors)
}
