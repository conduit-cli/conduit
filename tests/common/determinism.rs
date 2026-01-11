//! Deterministic test environment setup
//!
//! Provides utilities for creating reproducible tests by controlling
//! normally non-deterministic values like UUIDs and timestamps.

use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// Setup environment variables for deterministic test execution
pub fn setup_deterministic_env() {
    std::env::set_var("TZ", "UTC");
    std::env::set_var("NO_COLOR", "1");
    std::env::set_var("TERM", "dumb");
    std::env::set_var("COLUMNS", "80");
    std::env::set_var("LINES", "24");
    // Disable real agent discovery
    std::env::set_var("CONDUIT_TEST_MODE", "1");
}

/// Generates deterministic UUIDs for testing
///
/// Produces sequential UUIDs starting from a known seed,
/// ensuring tests are reproducible.
///
/// # Example
/// ```
/// use tests::common::DeterministicUuidGenerator;
///
/// let gen = DeterministicUuidGenerator::new();
/// let id1 = gen.next();
/// let id2 = gen.next();
/// assert_ne!(id1, id2);
/// ```
pub struct DeterministicUuidGenerator {
    counter: AtomicU64,
}

impl DeterministicUuidGenerator {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
        }
    }

    /// Generate the next deterministic UUID
    pub fn next(&self) -> Uuid {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        // Create a deterministic UUID from the counter
        // Uses a fixed namespace to ensure reproducibility
        Uuid::from_u128(n as u128)
    }

    /// Reset the generator to its initial state
    pub fn reset(&self) {
        self.counter.store(1, Ordering::SeqCst);
    }

    /// Get the current counter value without incrementing
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }
}

impl Default for DeterministicUuidGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Fixed timestamp for testing (2024-01-01 00:00:00 UTC)
pub const TEST_TIMESTAMP: &str = "2024-01-01T00:00:00Z";

/// Get a fixed chrono DateTime for testing
pub fn test_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(TEST_TIMESTAMP)
        .expect("Invalid test timestamp")
        .with_timezone(&chrono::Utc)
}

/// Format a timestamp in ISO 8601 format for SQLite compatibility
pub fn test_timestamp_string() -> String {
    TEST_TIMESTAMP.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_deterministic_uuid_generator() {
        let gen = DeterministicUuidGenerator::new();
        let id1 = gen.next();
        let id2 = gen.next();

        assert_ne!(id1, id2);
        assert_eq!(gen.current(), 3); // Started at 1, incremented twice
    }

    #[test]
    fn test_uuid_generator_reset() {
        let gen = DeterministicUuidGenerator::new();
        let id1 = gen.next();

        gen.reset();

        let id2 = gen.next();
        assert_eq!(id1, id2); // Same ID after reset
    }

    #[test]
    fn test_timestamp() {
        let ts = test_now();
        assert_eq!(ts.year(), 2024);
        assert_eq!(ts.month(), 1);
        assert_eq!(ts.day(), 1);
    }

    #[test]
    fn test_timestamp_string_format() {
        let ts_str = test_timestamp_string();
        assert_eq!(ts_str, "2024-01-01T00:00:00Z");
        // Verify it's valid ISO 8601 / RFC 3339
        assert!(chrono::DateTime::parse_from_rfc3339(&ts_str).is_ok());
    }

    #[test]
    fn test_setup_deterministic_env() {
        // Capture original values to restore after test
        let original_tz = std::env::var("TZ").ok();
        let original_no_color = std::env::var("NO_COLOR").ok();
        let original_term = std::env::var("TERM").ok();
        let original_columns = std::env::var("COLUMNS").ok();
        let original_lines = std::env::var("LINES").ok();
        let original_test_mode = std::env::var("CONDUIT_TEST_MODE").ok();

        setup_deterministic_env();

        // Verify the values are set
        assert_eq!(std::env::var("TZ").unwrap(), "UTC");
        assert_eq!(std::env::var("NO_COLOR").unwrap(), "1");
        assert_eq!(std::env::var("TERM").unwrap(), "dumb");
        assert_eq!(std::env::var("COLUMNS").unwrap(), "80");
        assert_eq!(std::env::var("LINES").unwrap(), "24");
        assert_eq!(std::env::var("CONDUIT_TEST_MODE").unwrap(), "1");

        // Restore original values to avoid polluting other tests
        fn restore_or_remove(key: &str, original: Option<String>) {
            match original {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }
        restore_or_remove("TZ", original_tz);
        restore_or_remove("NO_COLOR", original_no_color);
        restore_or_remove("TERM", original_term);
        restore_or_remove("COLUMNS", original_columns);
        restore_or_remove("LINES", original_lines);
        restore_or_remove("CONDUIT_TEST_MODE", original_test_mode);
    }
}
