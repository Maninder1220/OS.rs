// =============================================================================
// File: src/collector/mod.rs
// Purpose:
//   Collector module wiring and public exports for scanner, models, and port collection.
//
// Where this fits in OSAI:
//   Lets the rest of the Rust app import collector APIs through a stable module boundary.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Keep exports minimal so model ownership stays clear.
// =============================================================================
pub // -----------------------------------------------------------------------------
// Module wiring
// -----------------------------------------------------------------------------

mod models;
pub mod ports;
pub mod scanner;

pub // -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use models::Snapshot;
pub use scanner::collect_snapshot;
