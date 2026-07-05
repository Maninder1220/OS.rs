// =============================================================================
// File: src/plugins/mod.rs
// Purpose:
//   Plugin module wiring for Kubernetes and GitLab collectors.
//
// Where this fits in OSAI:
//   Keeps optional environment-specific collectors behind a small module boundary.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Add new plugin modules here only when their scanner integration is stable.
// =============================================================================
pub // -----------------------------------------------------------------------------
// Module wiring
// -----------------------------------------------------------------------------

mod gitlab;
pub mod kubernetes;

pub // -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use gitlab::collect_gitlab_hints;
pub use kubernetes::collect_kubernetes_hints;
