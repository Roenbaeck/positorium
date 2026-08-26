use crate::construct::{Database, PersistenceMode};
use crate::traqula::Engine;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

pub const WASM_INTERFACE_VERSION: &str = "1";

#[derive(serde::Serialize)]
struct WasmQueryResponse {
    interface_version: &'static str,
    traqula_version: u16,
    result_sets: Vec<crate::traqula::CollectedResultSet>,
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
        let engine = Engine::new(&self.db);
        let result_sets = engine
            .execute_collect_multi(script)
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
        let script = "add role person; add posit [{(+a, person)}, \"Alice\", @NOW]; search [{(*, person)}, +name, *] return name;";
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
}
