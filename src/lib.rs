//! **Stateless** Streamable HTTP (JSON + SSE) MCP server over the tableski engine.
//!
//! Everything about tables and tools lives in [`tableski_core`] and is re-exported here, so
//! `tableski::AppState` and `tableski_core::AppState` are the same type. This crate adds the
//! HTTP wiring ([`app_router`]) and the CLI (`src/main.rs`). No `Mcp-Session-Id` is issued or
//! required — every POST is independent.

use axum::Router;
use std::sync::Arc;

pub use tableski_core::*;

/// Build the stateless Streamable HTTP router over the registered tables.
pub fn app_router(state: AppState) -> Router {
    emperor_mcp::http_router(Arc::new(state))
}
