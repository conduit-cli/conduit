//! Web server module for Conduit.
//!
//! This module provides an HTTP/WebSocket server interface to Conduit's functionality,
//! enabling browser-based access to AI coding agent capabilities.
//!
//! Enable with the `web` feature flag: `cargo build --features web`

mod error;
pub mod handlers;
pub mod routes;
mod server;
mod state;
mod status_manager;
mod status_types;
pub mod ws;

pub use error::WebError;
pub use server::{run_server, ServerConfig};
pub use state::WebAppState;
pub use status_manager::{StatusManager, StatusManagerConfig};
pub use status_types::{GitDiffStatsResponse, PrStatusResponse, WorkspaceStatusResponse};
pub use ws::{ClientMessage, ServerMessage, SessionManager};
