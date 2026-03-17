use wasm_bindgen::prelude::*;
use crate::construct::{Database, PersistenceMode};
use crate::traqula::Engine;
use std::sync::Arc;

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
        Ok(WasmEngine {
            db: Arc::new(db),
        })
    }

    pub fn execute(&self, script: &str) -> Result<String, JsValue> {
        let engine = Engine::new(&self.db);
        match engine.execute_collect(script) {
            Ok(output) => {
                // For now, join rows by newline for a simple text output
                let mut result = String::new();
                if !output.columns.is_empty() {
                    result.push_str(&output.columns.join("\t"));
                    result.push('\n');
                    result.push_str(&"-".repeat(result.len()));
                    result.push('\n');
                }
                for row in output.rows {
                    result.push_str(&row.join("\t"));
                    result.push('\n');
                }
                if output.limited {
                    result.push_str("... (limited)\n");
                }
                Ok(result)
            }
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
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
        let script = "add role person; add posit [{(+a, person)}, \"Alice\", @NOW]; search person -> *;";
        let output = engine.execute(script).expect("Execution failed");
        
        // Check if output contains our expected value
        assert!(output.contains("Alice"));
        assert!(output.contains("person"));
    }

    #[wasm_bindgen_test]
    fn test_wasm_syntax_error() {
        let engine = WasmEngine::new().expect("Failed to create engine");
        let script = "invalid syntax;";
        let result = engine.execute(script);
        assert!(result.is_err());
    }
}
