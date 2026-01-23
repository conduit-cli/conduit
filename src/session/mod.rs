//! Session management module
//!
//! This module provides utilities for discovering and importing
//! sessions from external agents (Claude Code, Codex CLI, Gemini CLI, and OpenCode).

pub mod cache;
pub mod import;

pub use cache::{get_file_mtime, SessionCache};
pub use import::{
    discover_all_sessions, discover_claude_sessions, discover_codex_sessions,
    discover_opencode_sessions, discover_sessions_incremental, ExternalSession,
    SessionDiscoveryUpdate,
};
