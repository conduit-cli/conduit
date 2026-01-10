//! Shared test utilities for Conduit
//!
//! This module provides common helpers for integration and E2E tests:
//! - Deterministic UUID/timestamp generation
//! - Git repository fixtures
//! - TUI terminal testing helpers

pub mod determinism;
pub mod git_fixtures;
pub mod terminal;
