//! wasm-bindgen bindings for `agon_core`, consumed by `agon_ui`.
//!
//! Every function takes and returns JSON text in the serde shapes
//! `agon_core` defines (and tests), rather than generating a JS class per
//! Rust type: the UI keeps one small hand-written TS type file
//! (`agon_ui/src/lib/agonCore.ts`), and this shim stays trivially thin.

use wasm_bindgen::prelude::*;

/// Preview an auto-assigned split: `agon_core::team_builder::PlanInput`
/// JSON in, `PlanOutput` JSON out. Throws a JS error with a readable
/// message for malformed input or a pool that doesn't fit the sides.
#[wasm_bindgen(js_name = autoAssign)]
pub fn auto_assign(input: &str) -> Result<String, JsError> {
    let input: agon_core::team_builder::PlanInput =
        serde_json::from_str(input).map_err(|e| JsError::new(&format!("bad input: {e}")))?;
    let output = agon_core::team_builder::plan(&input).map_err(|e| JsError::new(&e.to_string()))?;
    serde_json::to_string(&output).map_err(|e| JsError::new(&e.to_string()))
}
