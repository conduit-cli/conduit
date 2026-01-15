//! Core module containing shared infrastructure for Conduit.
//!
//! This module provides the foundational components used by both the TUI and web interfaces:
//! - Database access and DAO stores
//! - Agent runners (Claude, Codex, Gemini)
//! - Configuration and tool availability
//! - Worktree management

mod conduit_core;

pub use conduit_core::ConduitCore;
