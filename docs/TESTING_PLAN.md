# Conduit E2E Testing Implementation Plan

## Overview

This document outlines the implementation plan for comprehensive end-to-end, integration, and unit testing for Conduit. The goal is to ensure deterministic, reproducible tests that prevent regressions in core user flows.

**Priority Order** (per stakeholder input):
1. Agent Interaction
2. PR Creation
3. Workspace Creation

---

## Phase 1: Foundation & Infrastructure

### 1.1 Add Test Dependencies

**File**: `Cargo.toml`

```toml
[dev-dependencies]
# Snapshot testing for TUI output
insta = { version = "1.40", features = ["yaml", "json"] }

# Property-based testing for JSONL parsing
proptest = "1.4"

# CLI binary testing
assert_cmd = "2.0"
predicates = "3.1"

# Async test utilities
tokio-test = "0.4"

# Lazy static for shared fixtures
once_cell = "1.19"

# Coverage (run with: cargo llvm-cov)
# Install: rustup component add llvm-tools-preview && cargo install cargo-llvm-cov
```

### 1.2 Create Test Directory Structure

```
tests/
├── common/
│   ├── mod.rs              # Re-exports all helpers
│   ├── determinism.rs      # UUID/timestamp fixtures
│   ├── git_fixtures.rs     # TestRepo helper
│   └── terminal.rs         # TestBackend helpers
├── fixtures/
│   ├── jsonl/
│   │   ├── claude_hello.jsonl
│   │   ├── claude_tool_bash.jsonl
│   │   ├── claude_tool_edit.jsonl
│   │   ├── claude_auth_error.jsonl
│   │   ├── claude_pr_creation.jsonl
│   │   └── codex_basic.jsonl
│   └── git/
│       └── (scripts if needed)
├── integration/
│   ├── mod.rs
│   ├── agent_session.rs    # Mock agent → session flow
│   ├── pr_workflow.rs      # PR preflight + mock agent
│   ├── workspace_flow.rs   # Workspace creation
│   └── ui_snapshots.rs     # TUI rendering snapshots
└── e2e/
    ├── mod.rs
    ├── cli_args.rs         # Binary argument parsing
    └── full_workflow.rs    # End-to-end with mock agents
```

### 1.3 Create Mock Agent Runner

**File**: `src/agent/mock.rs`

```rust
//! Mock agent runner for deterministic testing
//!
//! Implements AgentRunner trait to emit pre-configured events
//! without spawning real CLI processes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;
use crate::agent::events::AgentEvent;
use crate::agent::runner::{AgentHandle, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;

/// Configuration for mock behavior
#[derive(Clone)]
pub struct MockConfig {
    /// Events to emit when started
    pub events: Vec<AgentEvent>,
    /// Delay between events (simulates streaming)
    pub event_delay: Duration,
    /// Whether start() should fail
    pub fail_on_start: bool,
    /// Error message if failing
    pub error_message: Option<String>,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            event_delay: Duration::from_millis(1),
            fail_on_start: false,
            error_message: None,
        }
    }
}

impl MockConfig {
    pub fn with_events(mut self, events: Vec<AgentEvent>) -> Self {
        self.events = events;
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.event_delay = delay;
        self
    }

    pub fn failing(mut self, message: impl Into<String>) -> Self {
        self.fail_on_start = true;
        self.error_message = Some(message.into());
        self
    }
}

/// Mock agent runner for testing
pub struct MockAgentRunner {
    agent_type: AgentType,
    config: MockConfig,
    /// Captured start configs for verification
    captured_configs: Arc<Mutex<Vec<AgentStartConfig>>>,
    /// Captured inputs sent via send_input
    captured_inputs: Arc<Mutex<Vec<String>>>,
}

impl MockAgentRunner {
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            agent_type,
            config: MockConfig::default(),
            captured_configs: Arc::new(Mutex::new(Vec::new())),
            captured_inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_config(mut self, config: MockConfig) -> Self {
        self.config = config;
        self
    }

    /// Load events from a JSONL fixture file
    pub fn with_fixture(mut self, fixture_path: &str) -> Self {
        let content = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|_| panic!("Failed to load fixture: {}", fixture_path));

        let events: Vec<AgentEvent> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                // Parse JSONL and convert to AgentEvent
                // This depends on your event parsing logic
                parse_jsonl_to_agent_event(line).ok()
            })
            .collect();

        self.config.events = events;
        self
    }

    /// Get captured start configurations for assertions
    pub fn captured_configs(&self) -> Vec<AgentStartConfig> {
        self.captured_configs.lock().clone()
    }

    /// Get captured inputs for assertions
    pub fn captured_inputs(&self) -> Vec<String> {
        self.captured_inputs.lock().clone()
    }
}

#[async_trait]
impl AgentRunner for MockAgentRunner {
    fn agent_type(&self) -> AgentType {
        self.agent_type
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        // Capture the config for later assertions
        self.captured_configs.lock().push(config.clone());

        if self.config.fail_on_start {
            return Err(AgentError::ProcessSpawnFailed(
                self.config.error_message.clone().unwrap_or_default(),
            ));
        }

        let (tx, rx) = mpsc::channel(32);
        let events = self.config.events.clone();
        let delay = self.config.event_delay;

        // Spawn task to emit pre-configured events
        tokio::spawn(async move {
            for event in events {
                if tx.send(event).await.is_err() {
                    break; // Receiver dropped
                }
                tokio::time::sleep(delay).await;
            }
        });

        Ok(AgentHandle::new(rx, 99999)) // Fake PID
    }

    async fn send_input(&self, _handle: &AgentHandle, input: &str) -> Result<(), AgentError> {
        self.captured_inputs.lock().push(input.to_string());
        Ok(())
    }

    async fn stop(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        Ok(())
    }

    async fn kill(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn binary_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/mock/agent"))
    }
}

/// Parse a JSONL line into an AgentEvent
/// This bridges between raw Claude/Codex output and unified events
fn parse_jsonl_to_agent_event(line: &str) -> Result<AgentEvent, serde_json::Error> {
    // Use existing stream parsing logic
    use crate::agent::stream::ClaudeRawEvent;

    let raw: ClaudeRawEvent = serde_json::from_str(line)?;
    // Convert to AgentEvent based on type
    // This may need adjustment based on your actual conversion logic
    Ok(raw.into_agent_event())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_emits_configured_events() {
        let events = vec![
            AgentEvent::SessionInit(crate::agent::events::SessionInitEvent {
                session_id: SessionId::from_string("test-session".into()),
                model: Some("mock-model".into()),
            }),
        ];

        let runner = MockAgentRunner::new(AgentType::Claude)
            .with_config(MockConfig::default().with_events(events));

        let config = AgentStartConfig::new("test prompt", PathBuf::from("/tmp"));
        let mut handle = runner.start(config).await.unwrap();

        let event = handle.events.recv().await;
        assert!(event.is_some());
        assert!(matches!(event.unwrap(), AgentEvent::SessionInit(_)));
    }

    #[tokio::test]
    async fn test_mock_captures_configs() {
        let runner = MockAgentRunner::new(AgentType::Claude);

        let config = AgentStartConfig::new("hello world", PathBuf::from("/test"));
        let _ = runner.start(config).await.unwrap();

        let captured = runner.captured_configs();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].prompt, "hello world");
    }

    #[tokio::test]
    async fn test_mock_fails_when_configured() {
        let runner = MockAgentRunner::new(AgentType::Claude)
            .with_config(MockConfig::default().failing("auth error"));

        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let result = runner.start(config).await;

        assert!(result.is_err());
    }
}
```

### 1.4 Create Test Helpers

**File**: `tests/common/mod.rs`

```rust
//! Shared test utilities

pub mod determinism;
pub mod git_fixtures;
pub mod terminal;

pub use determinism::*;
pub use git_fixtures::*;
pub use terminal::*;
```

**File**: `tests/common/determinism.rs`

```rust
//! Deterministic test environment setup

use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// Setup environment for deterministic tests
pub fn setup_deterministic_env() {
    std::env::set_var("TZ", "UTC");
    std::env::set_var("NO_COLOR", "1");
    std::env::set_var("TERM", "dumb");
    std::env::set_var("COLUMNS", "80");
    std::env::set_var("LINES", "24");
}

/// Generates deterministic UUIDs for testing
pub struct DeterministicUuidGenerator {
    counter: AtomicU64,
}

impl DeterministicUuidGenerator {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
        }
    }

    pub fn next(&self) -> Uuid {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        // Create a deterministic UUID from the counter
        Uuid::from_u128(n as u128)
    }

    pub fn reset(&self) {
        self.counter.store(1, Ordering::SeqCst);
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
        .unwrap()
        .with_timezone(&chrono::Utc)
}
```

**File**: `tests/common/git_fixtures.rs`

```rust
//! Git repository test fixtures

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// A temporary git repository for testing
/// Automatically cleans up worktrees on drop
pub struct TestRepo {
    /// TempDir handle (keeps directory alive)
    _dir: TempDir,
    /// Path to the repository
    pub path: PathBuf,
    /// Tracks worktree paths for cleanup on drop
    worktrees: Arc<Mutex<Vec<PathBuf>>>,
}

impl TestRepo {
    /// Create a new test repository with initial commit
    pub fn new() -> Self {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = dir.path().to_path_buf();

        Self::git(&path, &["init"]);
        Self::git(&path, &["config", "user.email", "test@example.com"]);
        Self::git(&path, &["config", "user.name", "Test User"]);
        // Disable GPG signing for CI compatibility
        Self::git(&path, &["config", "commit.gpgsign", "false"]);

        // Create initial commit
        std::fs::write(path.join("README.md"), "# Test Repository\n").unwrap();
        Self::git(&path, &["add", "."]);
        Self::git(&path, &["commit", "-m", "Initial commit"]);

        Self {
            _dir: dir,
            path,
            worktrees: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a repo with multiple branches
    pub fn with_branches(branch_names: &[&str]) -> Self {
        let repo = Self::new();
        for branch in branch_names {
            Self::git(&repo.path, &["branch", branch]);
        }
        repo
    }

    /// Create a repo with uncommitted changes
    pub fn with_uncommitted_changes() -> Self {
        let repo = Self::new();
        std::fs::write(repo.path.join("dirty.txt"), "uncommitted content").unwrap();
        repo
    }

    /// Create a repo with staged changes
    pub fn with_staged_changes() -> Self {
        let repo = Self::new();
        std::fs::write(repo.path.join("staged.txt"), "staged content").unwrap();
        Self::git(&repo.path, &["add", "staged.txt"]);
        repo
    }

    /// Create a repo with a worktree already set up
    /// Returns both the repo and the worktree path for cleanup tracking
    pub fn with_worktree(worktree_name: &str, branch_name: &str) -> (Self, PathBuf) {
        let repo = Self::new();

        // Create the branch first
        Self::git(&repo.path, &["branch", branch_name]);

        // Create worktree directory path (sibling to main repo, inside the temp dir)
        let worktree_path = repo.path.parent().unwrap().join(worktree_name);

        Self::git(
            &repo.path,
            &["worktree", "add", worktree_path.to_str().unwrap(), branch_name],
        );

        // Track for cleanup on drop
        repo.worktrees.lock().unwrap().push(worktree_path.clone());

        (repo, worktree_path)
    }

    /// Create a worktree and track it for automatic cleanup on drop
    pub fn create_tracked_worktree(&self, worktree_path: &Path, branch_name: &str) {
        // Auto-creates branch if it doesn't exist
        Self::git(
            &self.path,
            &["worktree", "add", "-b", branch_name, worktree_path.to_str().unwrap()],
        );
        self.worktrees.lock().unwrap().push(worktree_path.to_path_buf());
    }

    /// Add a file and commit it
    pub fn commit_file(&self, filename: &str, content: &str, message: &str) {
        std::fs::write(self.path.join(filename), content).unwrap();
        Self::git(&self.path, &["add", filename]);
        Self::git(&self.path, &["commit", "-m", message]);
    }

    /// Checkout a branch
    pub fn checkout(&self, branch: &str) {
        Self::git(&self.path, &["checkout", branch]);
    }

    /// Create and checkout a new branch
    pub fn checkout_new_branch(&self, branch: &str) {
        Self::git(&self.path, &["checkout", "-b", branch]);
    }

    /// Get current branch name
    pub fn current_branch(&self) -> String {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get branch");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Check if repo is dirty
    pub fn is_dirty(&self) -> bool {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get status");
        !output.stdout.is_empty()
    }

    /// Execute git command
    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("Git command failed to execute");

        if !output.status.success() {
            panic!(
                "Git command failed: git {}\nstderr: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_creation() {
        let repo = TestRepo::new();
        assert!(repo.path.join(".git").exists());
        assert!(repo.path.join("README.md").exists());
    }

    #[test]
    fn test_repo_with_branches() {
        let repo = TestRepo::with_branches(&["feature-1", "feature-2"]);

        // Verify branches exist
        let output = Command::new("git")
            .args(["branch", "--list"])
            .current_dir(&repo.path)
            .output()
            .unwrap();

        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(branches.contains("feature-1"));
        assert!(branches.contains("feature-2"));
    }

    #[test]
    fn test_repo_with_uncommitted() {
        let repo = TestRepo::with_uncommitted_changes();
        assert!(repo.is_dirty());
    }
}
```

**File**: `tests/common/terminal.rs`

```rust
//! TUI testing utilities using Ratatui's TestBackend

use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    Terminal,
};

/// Create a test terminal with standard dimensions
pub fn create_test_terminal() -> Terminal<TestBackend> {
    create_test_terminal_sized(80, 24)
}

/// Create a test terminal with custom dimensions
pub fn create_test_terminal_sized(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to create test terminal")
}

/// Convert a buffer to a string for snapshot testing
pub fn buffer_to_string(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut output = String::new();

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                output.push_str(cell.symbol());
            }
        }
        output.push('\n');
    }

    output
}

/// Convert buffer to string, trimming trailing whitespace per line
pub fn buffer_to_trimmed_string(buffer: &Buffer) -> String {
    buffer_to_string(buffer)
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert that a specific region of the buffer contains expected text
pub fn assert_buffer_contains(buffer: &Buffer, area: Rect, expected: &str) {
    let mut actual = String::new();

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                actual.push_str(cell.symbol());
            }
        }
        if y < area.y + area.height - 1 {
            actual.push('\n');
        }
    }

    assert!(
        actual.contains(expected),
        "Buffer region does not contain expected text.\nExpected: {}\nActual:\n{}",
        expected,
        actual
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::{Block, Borders, Paragraph};

    #[test]
    fn test_create_terminal() {
        let terminal = create_test_terminal();
        let size = terminal.size().unwrap();
        assert_eq!(size.width, 80);
        assert_eq!(size.height, 24);
    }

    #[test]
    fn test_buffer_to_string() {
        let mut terminal = create_test_terminal_sized(10, 3);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Hello");
                f.render_widget(para, f.area());
            })
            .unwrap();

        let output = buffer_to_trimmed_string(terminal.backend().buffer());
        assert!(output.starts_with("Hello"));
    }
}
```

---

## Phase 2: Agent Interaction Tests

### 2.1 JSONL Fixtures

**File**: `tests/fixtures/jsonl/claude_hello.jsonl`

```json
{"type":"system","subtype":"init","session_id":"test-session-001","model":"claude-sonnet-4-5-20250929","tools":["Bash","Read","Write"]}
{"type":"assistant","message":{"content":[{"type":"text","text":"Hello! How can I help you today?"}]},"session_id":"test-session-001"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":1234,"session_id":"test-session-001","usage":{"input_tokens":100,"output_tokens":50}}
```

**File**: `tests/fixtures/jsonl/claude_tool_bash.jsonl`

```json
{"type":"system","subtype":"init","session_id":"test-session-002","model":"claude-sonnet-4-5-20250929","tools":["Bash","Read","Write"]}
{"type":"assistant","message":{"content":[{"type":"text","text":"I'll list the files for you."},{"type":"tool_use","id":"tool_001","name":"Bash","input":{"command":"ls -la"}}]},"session_id":"test-session-002"}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool_001","content":"total 8\ndrwxr-xr-x  3 user user 4096 Jan  1 00:00 .\ndrwxr-xr-x 10 user user 4096 Jan  1 00:00 ..\n-rw-r--r--  1 user user   42 Jan  1 00:00 README.md"}]},"session_id":"test-session-002"}
{"type":"assistant","message":{"content":[{"type":"text","text":"Here are the files in the current directory:\n- README.md"}]},"session_id":"test-session-002"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":2345,"session_id":"test-session-002","usage":{"input_tokens":200,"output_tokens":100}}
```

**File**: `tests/fixtures/jsonl/claude_auth_error.jsonl`

```json
{"type":"system","subtype":"init","session_id":"test-session-err","model":"claude-sonnet-4-5-20250929"}
{"type":"assistant","message":{"content":[{"type":"text","text":"Invalid API key \u00b7 Please run /login"}]},"error":true,"session_id":"test-session-err"}
{"type":"result","subtype":"success","is_error":true,"duration_ms":262,"result":"Invalid API key \u00b7 Please run /login","session_id":"test-session-err","usage":{"input_tokens":0,"output_tokens":0}}
```

### 2.2 Agent Session Integration Tests

**File**: `tests/integration/agent_session.rs`

```rust
//! Integration tests for agent session flow
//!
//! Tests the flow: MockAgent → AgentEvents → Session state

mod common;

use std::path::PathBuf;
use std::time::Duration;

use conduit::agent::events::{AgentEvent, SessionInitEvent, AssistantMessageEvent, TurnCompletedEvent};
use conduit::agent::mock::{MockAgentRunner, MockConfig};
use conduit::agent::runner::{AgentRunner, AgentStartConfig, AgentType};
use conduit::agent::session::SessionId;

#[tokio::test]
async fn test_mock_agent_emits_session_init() {
    let events = vec![AgentEvent::SessionInit(SessionInitEvent {
        session_id: SessionId::from_string("test-001".into()),
        model: Some("claude-sonnet".into()),
    })];

    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().with_events(events));

    let config = AgentStartConfig::new("Hello", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let event = handle.events.recv().await.unwrap();

    match event {
        AgentEvent::SessionInit(init) => {
            assert_eq!(init.session_id.as_str(), "test-001");
            assert_eq!(init.model, Some("claude-sonnet".into()));
        }
        _ => panic!("Expected SessionInit event"),
    }
}

#[tokio::test]
async fn test_mock_agent_emits_message_sequence() {
    let events = vec![
        AgentEvent::SessionInit(SessionInitEvent {
            session_id: SessionId::from_string("test-002".into()),
            model: Some("claude-sonnet".into()),
        }),
        AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "Hello! How can I help?".into(),
            is_final: true,
        }),
        AgentEvent::TurnCompleted(TurnCompletedEvent {
            usage: Default::default(),
        }),
    ];

    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().with_events(events));

    let config = AgentStartConfig::new("Hi", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    // Collect all events
    let mut received = Vec::new();
    while let Some(event) = handle.events.recv().await {
        received.push(event);
    }

    assert_eq!(received.len(), 3);
    assert!(matches!(received[0], AgentEvent::SessionInit(_)));
    assert!(matches!(received[1], AgentEvent::AssistantMessage(_)));
    assert!(matches!(received[2], AgentEvent::TurnCompleted(_)));
}

#[tokio::test]
async fn test_mock_agent_captures_config() {
    let runner = MockAgentRunner::new(AgentType::Claude);

    let config = AgentStartConfig::new("Test prompt", PathBuf::from("/workspace"))
        .with_model("opus")
        .with_tools(vec!["Bash".into(), "Read".into()]);

    let _ = runner.start(config).await.unwrap();

    let captured = runner.captured_configs();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].prompt, "Test prompt");
    assert_eq!(captured[0].model, Some("opus".into()));
    assert_eq!(captured[0].allowed_tools, vec!["Bash", "Read"]);
}

#[tokio::test]
async fn test_mock_agent_failure() {
    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().failing("Authentication failed"));

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let result = runner.start(config).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Authentication failed"));
}

#[tokio::test]
async fn test_mock_agent_with_delay() {
    let events = vec![
        AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "First".into(),
            is_final: false,
        }),
        AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "Second".into(),
            is_final: true,
        }),
    ];

    let runner = MockAgentRunner::new(AgentType::Claude).with_config(
        MockConfig::default()
            .with_events(events)
            .with_delay(Duration::from_millis(50)),
    );

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let start = std::time::Instant::now();

    let _ = handle.events.recv().await;
    let _ = handle.events.recv().await;

    let elapsed = start.elapsed();

    // Should have taken at least 50ms (one delay between events)
    assert!(elapsed >= Duration::from_millis(50));
}

// Property-based test for JSONL parsing resilience
#[cfg(feature = "proptest")]
mod proptest_tests {
    use proptest::prelude::*;
    use conduit::agent::stream::ClaudeRawEvent;

    proptest! {
        #[test]
        fn test_jsonl_parsing_never_panics(input in ".*") {
            // Should never panic on any input
            let _: Result<ClaudeRawEvent, _> = serde_json::from_str(&input);
        }
    }
}
```

### 2.3 Session State Tests

**File**: `tests/integration/session_state.rs`

```rust
//! Tests for session state management
//!
//! Verifies that agent events correctly update session state

use conduit::agent::events::*;
use conduit::ui::session::Session;

#[test]
fn test_session_tracks_messages() {
    let mut session = Session::new_for_testing();

    session.handle_event(AgentEvent::AssistantMessage(AssistantMessageEvent {
        text: "Hello!".into(),
        is_final: true,
    }));

    assert_eq!(session.messages().len(), 1);
    assert_eq!(session.messages()[0].content, "Hello!");
}

#[test]
fn test_session_tracks_tool_use() {
    let mut session = Session::new_for_testing();

    session.handle_event(AgentEvent::ToolStarted(ToolStartedEvent {
        tool_id: "tool-001".into(),
        tool_name: "Bash".into(),
        input: serde_json::json!({"command": "ls"}),
    }));

    session.handle_event(AgentEvent::ToolCompleted(ToolCompletedEvent {
        tool_id: "tool-001".into(),
        output: Some("file1.txt\nfile2.txt".into()),
        is_error: false,
    }));

    let tools = session.tool_uses();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "Bash");
    assert!(tools[0].completed);
}

#[test]
fn test_session_tracks_token_usage() {
    let mut session = Session::new_for_testing();

    session.handle_event(AgentEvent::TurnCompleted(TurnCompletedEvent {
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
        },
    }));

    let usage = session.total_usage();
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
}
```

---

## Phase 3: PR Workflow Tests

### 3.1 PR Preflight Unit Tests (extend existing)

**File**: `src/git/pr.rs` (add to existing tests module)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... existing tests ...

    #[test]
    fn test_parse_repo_name_https() {
        assert_eq!(
            PrManager::parse_repo_name_from_url("https://github.com/user/repo.git"),
            Some("repo".into())
        );
        assert_eq!(
            PrManager::parse_repo_name_from_url("https://github.com/user/repo"),
            Some("repo".into())
        );
    }

    #[test]
    fn test_parse_repo_name_ssh() {
        assert_eq!(
            PrManager::parse_repo_name_from_url("git@github.com:user/repo.git"),
            Some("repo".into())
        );
    }

    #[test]
    fn test_check_status_from_completed_checks() {
        let checks = vec![
            GhStatusCheck {
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
                state: "".into(),
            },
            GhStatusCheck {
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
                state: "".into(),
            },
        ];

        let status = CheckStatus::from_status_checks(&checks);
        assert_eq!(status.total, 2);
        assert_eq!(status.passed, 2);
        assert_eq!(status.state(), CheckState::Passing);
    }

    #[test]
    fn test_check_status_with_pending() {
        let checks = vec![
            GhStatusCheck {
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
                state: "".into(),
            },
            GhStatusCheck {
                status: "IN_PROGRESS".into(),
                conclusion: "".into(),
                state: "".into(),
            },
        ];

        let status = CheckStatus::from_status_checks(&checks);
        assert_eq!(status.state(), CheckState::Pending);
    }

    #[test]
    fn test_check_status_with_failure() {
        let checks = vec![
            GhStatusCheck {
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
                state: "".into(),
            },
            GhStatusCheck {
                status: "COMPLETED".into(),
                conclusion: "FAILURE".into(),
                state: "".into(),
            },
        ];

        let status = CheckStatus::from_status_checks(&checks);
        assert_eq!(status.state(), CheckState::Failing);
    }

    #[test]
    fn test_merge_readiness_ready() {
        let checks = CheckStatus {
            total: 2,
            passed: 2,
            failed: 0,
            pending: 0,
            skipped: 0,
        };

        let readiness = MergeReadiness::compute(
            &checks,
            MergeableStatus::Mergeable,
            ReviewDecision::Approved,
        );

        assert_eq!(readiness, MergeReadiness::Ready);
    }

    #[test]
    fn test_merge_readiness_conflicts_take_priority() {
        let checks = CheckStatus {
            total: 2,
            passed: 2,
            failed: 0,
            pending: 0,
            skipped: 0,
        };

        let readiness = MergeReadiness::compute(
            &checks,
            MergeableStatus::Conflicting,
            ReviewDecision::Approved,
        );

        assert_eq!(readiness, MergeReadiness::HasConflicts);
    }
}
```

### 3.2 PR Workflow Integration Tests

**File**: `tests/integration/pr_workflow.rs`

```rust
//! Integration tests for PR creation workflow

mod common;

use common::git_fixtures::TestRepo;
use conduit::git::pr::PrManager;

#[test]
fn test_preflight_detects_clean_repo() {
    let repo = TestRepo::new();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 0);
}

#[test]
fn test_preflight_detects_uncommitted_changes() {
    let repo = TestRepo::with_uncommitted_changes();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 1);
}

#[test]
fn test_preflight_detects_staged_changes() {
    let repo = TestRepo::with_staged_changes();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 1);
}

#[test]
fn test_get_current_branch() {
    let repo = TestRepo::new();

    // Default branch after init is usually 'main' or 'master'
    let branch = PrManager::get_current_branch(&repo.path);
    assert!(branch.is_some());

    // Create and checkout a feature branch
    repo.checkout_new_branch("feature/test");

    let branch = PrManager::get_current_branch(&repo.path).unwrap();
    assert_eq!(branch, "feature/test");
}

#[test]
fn test_detects_main_branch() {
    assert!(PrManager::is_main_branch("main"));
    assert!(PrManager::is_main_branch("master"));
    assert!(PrManager::is_main_branch("develop"));
    assert!(!PrManager::is_main_branch("feature/foo"));
    assert!(!PrManager::is_main_branch("fcoury/bold-fox"));
}

#[test]
fn test_has_no_upstream_initially() {
    let repo = TestRepo::new();
    repo.checkout_new_branch("feature/test");

    // No remote, so no upstream
    let has_upstream = PrManager::has_upstream(&repo.path);
    assert!(!has_upstream);
}

#[test]
fn test_pr_prompt_generation() {
    use conduit::git::pr::PrPreflightResult;

    let preflight = PrPreflightResult {
        gh_installed: true,
        gh_authenticated: true,
        on_main_branch: false,
        branch_name: "fcoury/bold-fox".into(),
        target_branch: "origin/main".into(),
        uncommitted_count: 3,
        has_upstream: false,
        existing_pr: None,
    };

    let prompt = PrManager::generate_pr_prompt(&preflight);

    assert!(prompt.contains("3 uncommitted changes"));
    assert!(prompt.contains("fcoury/bold-fox"));
    assert!(prompt.contains("no upstream branch"));
    assert!(prompt.contains("gh pr create --base main"));
}
```

---

## Phase 4: Workspace Creation Tests

### 4.1 Workspace Unit Tests (extend existing)

**File**: `src/util/names.rs` (existing tests are good, add more edge cases)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... existing tests ...

    #[test]
    fn test_generate_workspace_name_with_many_existing() {
        // Simulate having many existing workspaces
        let existing: HashSet<String> = (0..100)
            .map(|i| format!("workspace-{}", i))
            .collect();

        let name = generate_workspace_name(&existing);

        // Should still generate a unique name
        assert!(!existing.contains(&name));
        assert!(!name.is_empty());
    }

    #[test]
    fn test_branch_name_with_special_chars() {
        let name = generate_branch_name("user@domain.com", "test workspace");

        // Should sanitize special characters
        assert!(!name.contains('@'));
        assert!(!name.contains(' '));
    }

    #[test]
    fn test_branch_name_max_length() {
        let long_username = "a".repeat(100);
        let long_workspace = "b".repeat(100);

        let name = generate_branch_name(&long_username, &long_workspace);

        // Git has limits on ref name lengths
        assert!(name.len() <= 255);
    }
}
```

### 4.2 Workspace Integration Tests

**File**: `tests/integration/workspace_flow.rs`

```rust
//! Integration tests for workspace creation flow

mod common;

use common::git_fixtures::TestRepo;
use conduit::git::worktree::WorktreeManager;
use conduit::data::database::Database;
use conduit::data::workspace::Workspace;
use conduit::data::repository::Repository;
use tempfile::TempDir;

fn create_test_db() -> (Database, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Database::open(db_path).unwrap();
    (db, dir)
}

#[test]
fn test_worktree_creation_existing_branch() {
    let repo = TestRepo::with_branches(&["feature-1"]);

    // Use a unique path to avoid collisions
    let unique_id = Uuid::new_v4().as_simple().to_string();
    let worktree_path = repo.path.parent().unwrap()
        .join(format!("wt-feature-1-{}", &unique_id[..8]));

    // Use tracked worktree creation for automatic cleanup
    repo.create_tracked_worktree(&worktree_path, "feature-1");

    assert!(worktree_path.exists());
    assert!(worktree_path.join(".git").exists());
}

#[test]
fn test_worktree_creation_new_branch() {
    let repo = TestRepo::new();

    // Use a unique path to avoid collisions
    let unique_id = Uuid::new_v4().as_simple().to_string();
    let worktree_path = repo.path.parent().unwrap()
        .join(format!("wt-new-feature-{}", &unique_id[..8]));

    // create_tracked_worktree auto-creates the branch if it doesn't exist
    repo.create_tracked_worktree(&worktree_path, "new-feature");

    assert!(worktree_path.exists());

    // Verify branch was created
    let branches = repo.branches();
    assert!(branches.iter().any(|b| b == "new-feature"));
}

#[test]
fn test_workspace_persists_to_database() {
    let (db, _dir) = create_test_db();

    // Create repository first
    let repo = Repository::new("/test/path", "test-repo");
    repo.save(&db).unwrap();

    // Create workspace
    let workspace = Workspace::new(
        &repo.id,
        "bold-fox",
        "fcoury/bold-fox",
        "/test/path/worktrees/bold-fox",
    );
    workspace.save(&db).unwrap();

    // Verify it can be retrieved
    let loaded = Workspace::find_by_id(&db, &workspace.id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded.name, "bold-fox");
    assert_eq!(loaded.branch, "fcoury/bold-fox");
}

#[test]
fn test_workspace_unique_names_per_repo() {
    let (db, _dir) = create_test_db();

    let repo = Repository::new("/test/path", "test-repo");
    repo.save(&db).unwrap();

    // Get existing workspace names
    let existing = Workspace::names_for_repository(&db, &repo.id).unwrap();
    assert!(existing.is_empty());

    // Create workspace
    let ws1 = Workspace::new(&repo.id, "bold-fox", "branch1", "/path1");
    ws1.save(&db).unwrap();

    // Now existing should contain the new name
    let existing = Workspace::names_for_repository(&db, &repo.id).unwrap();
    assert!(existing.contains("bold-fox"));
}

#[test]
fn test_full_workspace_creation_flow() {
    let repo = TestRepo::new();
    let (db, _db_dir) = create_test_db();

    // 1. Register repository
    let db_repo = Repository::new(
        repo.path.to_str().unwrap(),
        "test-repo",
    );
    db_repo.save(&db).unwrap();

    // 2. Generate unique workspace name
    let existing = Workspace::names_for_repository(&db, &db_repo.id).unwrap();
    let workspace_name = conduit::util::names::generate_workspace_name(&existing);

    // 3. Generate branch name
    let branch_name = conduit::util::names::generate_branch_name("testuser", &workspace_name);

    // 4. Create worktree
    let worktree_path = repo.path.parent().unwrap().join(&workspace_name);
    let manager = WorktreeManager::new();
    manager
        .create_worktree_new_branch(
            &repo.path,
            &branch_name,
            worktree_path.to_str().unwrap(),
        )
        .unwrap();

    // 5. Persist to database
    let workspace = Workspace::new(
        &db_repo.id,
        &workspace_name,
        &branch_name,
        worktree_path.to_str().unwrap(),
    );
    workspace.save(&db).unwrap();

    // Verify everything
    assert!(worktree_path.exists());
    let loaded = Workspace::find_by_id(&db, &workspace.id).unwrap().unwrap();
    assert_eq!(loaded.name, workspace_name);
}
```

---

## Phase 5: TUI Snapshot Tests

### 5.1 UI Component Snapshots

**File**: `tests/integration/ui_snapshots.rs`

```rust
//! Snapshot tests for TUI components

mod common;

use common::terminal::{create_test_terminal, buffer_to_trimmed_string};
use insta::assert_snapshot;
use ratatui::layout::Rect;

// Import your UI components
use conduit::ui::components::chat_view::ChatView;
use conduit::ui::components::status_bar::StatusBar;
use conduit::ui::components::tab_bar::TabBar;

#[test]
fn test_chat_view_empty() {
    let mut terminal = create_test_terminal();

    terminal.draw(|f| {
        let view = ChatView::new(&[]);
        f.render_widget(view, f.area());
    }).unwrap();

    let output = buffer_to_trimmed_string(terminal.backend().buffer());
    assert_snapshot!("chat_view_empty", output);
}

#[test]
fn test_chat_view_with_messages() {
    let mut terminal = create_test_terminal();

    let messages = vec![
        // Create test messages
        // This depends on your ChatMessage type
    ];

    terminal.draw(|f| {
        let view = ChatView::new(&messages);
        f.render_widget(view, f.area());
    }).unwrap();

    let output = buffer_to_trimmed_string(terminal.backend().buffer());
    assert_snapshot!("chat_view_messages", output);
}

#[test]
fn test_status_bar_build_mode() {
    let mut terminal = create_test_terminal();

    terminal.draw(|f| {
        let area = Rect::new(0, 23, 80, 1); // Bottom row
        let bar = StatusBar::new()
            .model("claude-sonnet")
            .mode("Build")
            .tokens(1000, 500);
        f.render_widget(bar, area);
    }).unwrap();

    let output = buffer_to_trimmed_string(terminal.backend().buffer());
    assert_snapshot!("status_bar_build", output);
}

#[test]
fn test_tab_bar_single_tab() {
    let mut terminal = create_test_terminal();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 1); // Top row
        let tabs = TabBar::new(&["bold-fox"], 0);
        f.render_widget(tabs, area);
    }).unwrap();

    let output = buffer_to_trimmed_string(terminal.backend().buffer());
    assert_snapshot!("tab_bar_single", output);
}

#[test]
fn test_tab_bar_multiple_tabs() {
    let mut terminal = create_test_terminal();

    terminal.draw(|f| {
        let area = Rect::new(0, 0, 80, 1);
        let tabs = TabBar::new(
            &["bold-fox", "swift-eagle", "calm-river"],
            1, // Second tab selected
        );
        f.render_widget(tabs, area);
    }).unwrap();

    let output = buffer_to_trimmed_string(terminal.backend().buffer());
    assert_snapshot!("tab_bar_multiple", output);
}
```

---

## Phase 6: E2E Tests

### 6.1 CLI Binary Tests

**File**: `tests/e2e/cli_args.rs`

```rust
//! E2E tests for CLI argument parsing

use assert_cmd::Command;
use predicates::prelude::*;

fn conduit() -> Command {
    Command::cargo_bin("conduit").unwrap()
}

#[test]
fn test_version_flag() {
    conduit()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("conduit"));
}

#[test]
fn test_help_flag() {
    conduit()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Multi-agent TUI"))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("Codex CLI"));
}

#[test]
fn test_invalid_flag() {
    conduit()
        .arg("--invalid-flag-that-doesnt-exist")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_debug_keys_subcommand() {
    // This subcommand should exist
    conduit()
        .arg("debug-keys")
        .arg("--help")
        .assert()
        .success();
}
```

---

## Phase 7: CI/CD Configuration

### 7.1 GitHub Actions Workflow

**File**: `.github/workflows/test.yml`

```yaml
name: Tests

on:
  push:
    branches: [main, master]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  # Fast checks that run on every PR
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --all-targets

  # Format and lint
  fmt-clippy:
    name: Format & Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings

  # Unit tests (in-module tests)
  unit-tests:
    name: Unit Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --lib

  # Integration tests
  integration-tests:
    name: Integration Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --test '*'

  # Snapshot tests
  snapshot-tests:
    name: Snapshot Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo install cargo-insta
      - run: cargo insta test --check
        # Fail if snapshots don't match

  # Doc tests
  doc-tests:
    name: Doc Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --doc

  # Coverage report
  coverage:
    name: Code Coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - run: cargo install cargo-llvm-cov
      - run: cargo llvm-cov --all-features --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: true
        env:
          CODECOV_TOKEN: ${{ secrets.CODECOV_TOKEN }}

  # Cross-platform verification
  cross-platform:
    name: Cross Platform (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release
      - run: cargo test
```

### 7.2 Coverage Badge

Add to `README.md`:

```markdown
[![codecov](https://codecov.io/gh/fcoury/conduit/branch/main/graph/badge.svg)](https://codecov.io/gh/fcoury/conduit)
```

---

## Implementation Checklist

### Phase 1: Foundation (Week 1)
- [ ] Add dev-dependencies to Cargo.toml
- [ ] Create `tests/` directory structure
- [ ] Create `tests/common/mod.rs` with helper modules
- [ ] Create `tests/common/determinism.rs`
- [ ] Create `tests/common/git_fixtures.rs`
- [ ] Create `tests/common/terminal.rs`
- [ ] Create `src/agent/mock.rs` (MockAgentRunner)
- [ ] Add `pub mod mock;` to `src/agent/mod.rs`

### Phase 2: Agent Tests (Week 1-2)
- [ ] Create JSONL fixtures in `tests/fixtures/jsonl/`
- [ ] Create `tests/integration/agent_session.rs`
- [ ] Create `tests/integration/session_state.rs`
- [ ] Add property-based tests for JSONL parsing

### Phase 3: PR Tests (Week 2)
- [ ] Extend `src/git/pr.rs` tests
- [ ] Create `tests/integration/pr_workflow.rs`
- [ ] Test PR preflight with various repo states

### Phase 4: Workspace Tests (Week 2-3)
- [ ] Extend `src/util/names.rs` tests
- [ ] Create `tests/integration/workspace_flow.rs`
- [ ] Test full workspace creation flow

### Phase 5: TUI Snapshots (Week 3)
- [ ] Create `tests/integration/ui_snapshots.rs`
- [ ] Generate initial snapshots with `cargo insta test`
- [ ] Review and approve snapshots

### Phase 6: E2E & CI (Week 3-4)
- [ ] Create `tests/e2e/cli_args.rs`
- [ ] Create `.github/workflows/test.yml`
- [ ] Set up Codecov integration
- [ ] Add coverage badge to README

---

## Running Tests

```bash
# Run all tests
cargo test

# Run only unit tests (in-module)
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Run with coverage
cargo llvm-cov

# Review/update snapshots
cargo insta test        # Run tests
cargo insta review      # Interactive review
cargo insta accept      # Accept all pending

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_mock_agent_emits_session_init
```

---

## Summary

This plan provides:

1. **Deterministic testing** via MockAgentRunner and fixed fixtures
2. **Test isolation** via TempDir and hybrid fixture strategy
3. **Visual regression testing** via Ratatui TestBackend + insta snapshots
4. **CI/CD integration** with coverage reporting
5. **Prioritized implementation** (Agent → PR → Workspace)

The mock agent infrastructure is the key enabler - it allows testing the full agent interaction flow without real CLI processes or API calls, making tests fast, deterministic, and free.
