//! Main entry point for integration tests
//!
//! This file includes all integration test modules.
//! Run with: `cargo test --test integration_tests`
//!
//! Note: The `common` module is loaded via `#[path]` in each integration test module
//! to avoid duplicate module loading issues.

mod integration;

// Re-export the test modules so tests are discovered
pub use integration::*;
