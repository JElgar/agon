//! wasm-bindgen bindings for `agon_core`, consumed by `agon_ui`.
//!
//! The types crossing the boundary are `agon_core`'s own: its `wasm`
//! feature derives a TypeScript type for each (via `tsify`) into this
//! package's generated `.d.ts`, and passes them as plain JS objects (via
//! `serde-wasm-bindgen`). So `agon_ui` imports the generated types and never
//! hand-copies a Rust struct, and a mismatch is a type error at build time.

use agon_core::team_builder::{PlanInput, PlanOutput};
use wasm_bindgen::prelude::*;

/// Preview an auto-assigned split. Throws a JS error with a readable
/// message for a pool that doesn't fit the sides (or a bad pin).
#[wasm_bindgen(js_name = autoAssign)]
pub fn auto_assign(input: PlanInput) -> Result<PlanOutput, JsError> {
    agon_core::team_builder::plan(&input).map_err(|e| JsError::new(&e.to_string()))
}
