//! Main entry point for integration tests
//!
//! This file includes all integration test modules.
//! Run with: `cargo test --test integration_tests`

mod common;
mod integration;

// Re-export the test modules so tests are discovered
pub use integration::*;
