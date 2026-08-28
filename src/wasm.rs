use crate::construct::{Database, PersistenceMode};
use crate::terrain::{
    DEFAULT_MAX_RELATIONSHIP_SIGNATURES, DEFAULT_PROJECTED_ROLE_LIMIT, TERRAIN_VERSION,
    TerrainOptions, TerrainReport,
};
use crate::traqula::{Engine, ExecutionOptions, ExecutionParameter};
use std::collections::HashMap;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

pub const WASM_INTERFACE_VERSION: &str = "1";

#[derive(serde::Serialize)]
struct WasmQueryResponse {
    interface_version: &'static str,
    traqula_version: u16,
    result_sets: Vec<crate::traqula::CollectedResultSet>,
}

#[derive(Debug, serde::Deserialize)]
struct WasmTerrainOptions {
    #[serde(default = "terrain_version")]
    terrain_version: u16,
    #[serde(default)]
    as_of: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    projected_role_limit: Option<usize>,
    #[serde(default)]
    max_relationship_signatures: Option<usize>,
}

impl Default for WasmTerrainOptions {
    fn default() -> Self {
        Self {
            terrain_version: TERRAIN_VERSION,
            as_of: None,
            timeout_ms: None,
            projected_role_limit: None,
            max_relationship_signatures: None,
        }
    }
}

fn terrain_version() -> u16 {
    TERRAIN_VERSION
}

#[derive(serde::Serialize)]
struct WasmTerrainResponse {
    interface_version: &'static str,
    terrain_version: u16,
    report: TerrainReport,
}

#[wasm_bindgen]
pub struct WasmEngine {
    db: Arc<Database>,
}

#[wasm_bindgen]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WasmEngine, JsValue> {
        let db = Database::new(PersistenceMode::InMemory)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmEngine { db: Arc::new(db) })
    }

    pub fn execute(&self, script: &str) -> Result<JsValue, JsValue> {
        self.execute_with_options(script, ExecutionOptions::default())
    }

    pub fn execute_with_parameters(
        &self,
        script: &str,
        parameters: JsValue,
    ) -> Result<JsValue, JsValue> {
        let parameters: HashMap<String, ExecutionParameter> =
            serde_wasm_bindgen::from_value(parameters)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.execute_with_options(
            script,
            ExecutionOptions {
                parameters,
                ..ExecutionOptions::default()
            },
        )
    }

    /// Build an authoritative Terrain report. Browser execution is synchronous,
    /// so deadlines and hard limits are enforced cooperatively but JavaScript
    /// cannot deliver a same-thread cancellation call while this method runs.
    pub fn terrain(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let request = if options.is_undefined() || options.is_null() {
            WasmTerrainOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options)
                .map_err(|error| JsValue::from_str(&error.to_string()))?
        };
        if request.terrain_version != TERRAIN_VERSION {
            return Err(JsValue::from_str(&format!(
                "unsupported Terrain version {}; supported version is {TERRAIN_VERSION}",
                request.terrain_version
            )));
        }
        let resolved_now = crate::datatype::Time::new();
        let as_of_token = request.as_of.as_deref().unwrap_or("@NOW");
        let as_of =
            crate::traqula::parse_time_with_now(as_of_token, &resolved_now).ok_or_else(|| {
                JsValue::from_str(&format!("invalid Terrain as_of token '{as_of_token}'"))
            })?;
        let timeout =
            std::time::Duration::from_millis(request.timeout_ms.unwrap_or(5_000).min(30_000));
        if timeout.is_zero() {
            return Err(JsValue::from_str("timeout_ms must be positive"));
        }
        let report = self
            .db
            .terrain_with_options(TerrainOptions {
                as_of: Some(as_of),
                timeout: Some(timeout),
                projected_role_limit: request
                    .projected_role_limit
                    .unwrap_or(DEFAULT_PROJECTED_ROLE_LIMIT),
                max_relationship_signatures: request
                    .max_relationship_signatures
                    .unwrap_or(DEFAULT_MAX_RELATIONSHIP_SIGNATURES),
                ..TerrainOptions::default()
            })
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        serde_wasm_bindgen::to_value(&WasmTerrainResponse {
            interface_version: WASM_INTERFACE_VERSION,
            terrain_version: TERRAIN_VERSION,
            report,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

impl WasmEngine {
    fn execute_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<JsValue, JsValue> {
        let engine = Engine::new(&self.db);
        let result_sets = engine
            .execute_collect_multi_with_options(script, options)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        serde_wasm_bindgen::to_value(&WasmQueryResponse {
            interface_version: WASM_INTERFACE_VERSION,
            traqula_version: crate::traqula::TRAQULA_VERSION,
            result_sets,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use serde::Serialize;
    use wasm_bindgen_test::*;

    // This ensures tests run in a environment with a JS global if using Node or Browser
    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_wasm_engine_initialization() {
        let engine = WasmEngine::new();
        assert!(engine.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_wasm_execution() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        // Simple script to test integration
        let script = "add role person; add posit [{(+a, person)}, \"Alice\", @NOW]; search [{(*, person), ...}, ?name, *] return ?name;";
        let output = engine.execute(script).expect("Execution failed");

        let output: serde_json::Value = serde_wasm_bindgen::from_value(output).unwrap();
        assert_eq!(output["interface_version"], "1");
        assert_eq!(output["traqula_version"], 1);
        assert_eq!(output["result_sets"][0]["rows"][0][0]["text"], "\"Alice\"");
        assert_eq!(output["result_sets"][0]["rows"][0][0]["kind"], "literal");
    }

    #[wasm_bindgen_test]
    fn test_wasm_syntax_error() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        let script = "invalid syntax;";
        let result = engine.execute(script);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn invalid_or_add_wasm_execution_returns_no_result_and_never_mutates() {
        const INVALID_SCRIPT: &str = r#"
            search [{(?registry, registry_code), ...}, ?code, *]
            return ?registry, ?code
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
        "#;

        for matched in [false, true] {
            let engine = WasmEngine::new().expect("Failed to create engine");
            engine.execute("add role registry_code;").unwrap();
            if matched {
                engine
                    .execute("add posit [{(+existing, registry_code)}, \"CODE\", '2024-01-01'];")
                    .unwrap();
            }
            let count = |engine: &WasmEngine| {
                let output = engine
                    .execute("search [{(?registry, registry_code), ...}, *, *] return ?registry;")
                    .unwrap();
                let output: serde_json::Value = serde_wasm_bindgen::from_value(output).unwrap();
                output["result_sets"][0]["row_count"].as_u64().unwrap()
            };
            let before = count(&engine);

            let error = engine.execute(INVALID_SCRIPT).unwrap_err();
            assert!(
                error
                    .as_string()
                    .unwrap()
                    .contains("fallback cannot supply")
            );
            assert_eq!(count(&engine), before);
        }
    }

    #[wasm_bindgen_test]
    fn test_wasm_typed_parameters() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        engine
            .execute("add role number; add posit [{(+item, number)}, +0010.00, @NOW];")
            .unwrap();
        let parameters = serde_wasm_bindgen::to_value(&serde_json::json!({
            "target": { "kind": "literal", "text": "10" }
        }))
        .unwrap();
        let output = engine
            .execute_with_parameters(
                "search [{(?item, number)}, ?value, *] where ?value = $target return ?value;",
                parameters,
            )
            .unwrap();
        let output: serde_json::Value = serde_wasm_bindgen::from_value(output).unwrap();
        assert_eq!(output["result_sets"][0]["rows"][0][0]["text"], "+0010.00");
    }

    #[wasm_bindgen_test]
    fn test_wasm_information_in_effect() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        let output = engine
            .execute(
                "add role status; \
                 add posit +target [{(+case, status)}, \"open\", '2024-01-01']; \
                 add posit [{(target, posit), (+source, ascertains)}, 80%, '2024-02-01']; \
                 search [{(?case, status)}, ?state, *] \
                   in effect '2025-01-01', '2025-01-01' \
                 return ?state;",
            )
            .unwrap();
        let output: serde_json::Value = serde_wasm_bindgen::from_value(output).unwrap();
        assert_eq!(output["result_sets"][0]["rows"][0][0]["text"], "\"open\"");
    }

    #[wasm_bindgen_test]
    fn test_wasm_terrain_matches_the_rust_report() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        engine
            .execute(
                "add role name; \
                 add posit [{(+person, name)}, \"Ada\", '2024-01-01'];",
            )
            .unwrap();
        let options = serde_json::json!({
            "terrain_version": TERRAIN_VERSION,
            "as_of": "'2025-01-01'",
            "projected_role_limit": 8,
            "max_relationship_signatures": 16
        })
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap();
        let output = engine.terrain(options).unwrap();
        let output: serde_json::Value = serde_wasm_bindgen::from_value(output).unwrap();
        let rust_report = engine
            .db
            .terrain_with_options(TerrainOptions {
                as_of: Some(crate::datatype::Time::new_date_from("2025-01-01").unwrap()),
                timeout: Some(std::time::Duration::from_secs(5)),
                ..TerrainOptions::default()
            })
            .unwrap();
        assert_eq!(output["interface_version"], WASM_INTERFACE_VERSION);
        assert_eq!(output["terrain_version"], TERRAIN_VERSION);
        assert_eq!(output["report"], serde_json::to_value(rust_report).unwrap());
    }
}
