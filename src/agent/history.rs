//! Agent session history reader
//!
//! Reads chat history from agent files for session restoration.
//! - Claude Code: ~/.claude/projects/{project-path}/{session-id}.jsonl
//! - Codex CLI: ~/.codex/sessions/YYYY/MM/DD/rollout-*-{session-id}.jsonl
//! - Gemini CLI: not supported yet

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::display::MessageDisplay;
#[cfg(test)]
use crate::ui::components::MessageRole;
use crate::ui::components::{ChatMessage, TurnSummary};

/// Info extracted from a function_call entry for later lookup
struct FunctionCallInfo {
    name: String,
    command: String,
    session_id: Option<i64>,
}

/// Info extracted from a Claude tool_use block for later lookup
#[derive(Clone)]
struct ClaudeToolUseInfo {
    name: String,
    input: serde_json::Value,
}

struct ClaudeTurnTracker {
    started_at: Option<DateTime<Utc>>,
    last_assistant_at: Option<DateTime<Utc>>,
    usage_by_request: HashMap<String, (u64, u64)>,
    fallback_usage: (u64, u64),
    has_turn: bool,
}

struct CodexTurnTracker {
    started_at: Option<DateTime<Utc>>,
    last_assistant_at: Option<DateTime<Utc>>,
    last_usage: Option<(u64, u64)>,
    last_usage_at: Option<DateTime<Utc>>,
    has_turn: bool,
}

struct PendingExecOutput {
    message_index: usize,
}

impl CodexTurnTracker {
    fn new() -> Self {
        Self {
            started_at: None,
            last_assistant_at: None,
            last_usage: None,
            last_usage_at: None,
            has_turn: false,
        }
    }

    fn start_turn(&mut self, started_at: Option<DateTime<Utc>>) {
        self.started_at = started_at;
        self.last_assistant_at = None;
        self.last_usage = None;
        self.last_usage_at = None;
        self.has_turn = true;
    }

    fn update_usage(&mut self, usage: (u64, u64), timestamp: Option<DateTime<Utc>>) {
        self.last_usage = Some(usage);
        if timestamp.is_some() {
            self.last_usage_at = timestamp;
        }
    }

    fn update_assistant(&mut self, timestamp: Option<DateTime<Utc>>) {
        if timestamp.is_some() {
            self.last_assistant_at = timestamp;
        }
    }

    fn finish_turn(&mut self) -> Option<TurnSummary> {
        if !self.has_turn {
            return None;
        }
        let end_at = self.last_assistant_at.or(self.last_usage_at);
        let summary = build_turn_summary(self.started_at, end_at, self.last_usage);
        self.has_turn = false;
        summary
    }
}
impl ClaudeTurnTracker {
    fn new() -> Self {
        Self {
            started_at: None,
            last_assistant_at: None,
            usage_by_request: HashMap::new(),
            fallback_usage: (0, 0),
            has_turn: false,
        }
    }

    fn start_turn(&mut self, started_at: Option<DateTime<Utc>>) {
        self.started_at = started_at;
        self.last_assistant_at = None;
        self.usage_by_request.clear();
        self.fallback_usage = (0, 0);
        self.has_turn = true;
    }

    fn update_assistant(
        &mut self,
        request_id: Option<&str>,
        usage: (u64, u64),
        timestamp: Option<DateTime<Utc>>,
    ) {
        if let Some(ts) = timestamp {
            self.last_assistant_at = Some(ts);
        }
        if let Some(request_id) = request_id {
            // Same request_id may appear multiple times with cumulative counts;
            // keep the maximum (most complete) value for each request
            let entry = self
                .usage_by_request
                .entry(request_id.to_string())
                .or_insert((0, 0));
            entry.0 = entry.0.max(usage.0);
            entry.1 = entry.1.max(usage.1);
        } else {
            // Fallback entries are distinct unidentified requests; sum them
            self.fallback_usage.0 = self.fallback_usage.0.saturating_add(usage.0);
            self.fallback_usage.1 = self.fallback_usage.1.saturating_add(usage.1);
        }
    }

    fn finish_turn(&mut self) -> Option<TurnSummary> {
        if !self.has_turn {
            return None;
        }

        let mut input_tokens = self.fallback_usage.0;
        let mut output_tokens = self.fallback_usage.1;
        for usage in self.usage_by_request.values() {
            input_tokens = input_tokens.saturating_add(usage.0);
            output_tokens = output_tokens.saturating_add(usage.1);
        }

        let mut has_data = false;
        let mut summary = TurnSummary::new();

        if input_tokens > 0 || output_tokens > 0 {
            summary = summary.with_tokens(input_tokens, output_tokens);
            has_data = true;
        }

        if let (Some(start), Some(end)) = (self.started_at, self.last_assistant_at) {
            let duration = (end - start).num_seconds().max(0) as u64;
            if duration > 0 {
                summary = summary.with_duration(duration);
                has_data = true;
            }
        }

        self.has_turn = false;

        if has_data {
            Some(summary)
        } else {
            None
        }
    }
}

fn parse_timestamp(entry: &Value) -> Option<DateTime<Utc>> {
    entry
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn extract_claude_usage(entry: &Value) -> Option<(u64, u64)> {
    let usage = entry
        .get("message")
        .and_then(|m| m.get("usage"))
        .or_else(|| entry.get("usage"))?;
    let input = usage.get("input_tokens").and_then(|v| v.as_u64())?;
    let output = usage.get("output_tokens").and_then(|v| v.as_u64())?;
    Some((input, output))
}

fn extract_codex_usage(entry: &Value) -> Option<(u64, u64)> {
    let usage = entry
        .get("usage")
        .or_else(|| {
            entry
                .get("info")
                .and_then(|info| info.get("last_token_usage"))
        })
        .or_else(|| {
            entry
                .get("info")
                .and_then(|info| info.get("total_token_usage"))
        })?;
    let input = usage.get("input_tokens").and_then(|v| v.as_u64())?;
    let output = usage.get("output_tokens").and_then(|v| v.as_u64())?;
    Some((input, output))
}

fn parse_running_session_id(raw_output: &str) -> Option<i64> {
    let marker = "Process running with session ID ";
    let start = raw_output.find(marker)?;
    let after = &raw_output[start + marker.len()..];
    let end = after.find('\n').unwrap_or(after.len());
    after[..end].trim().parse::<i64>().ok()
}

fn append_output(target: &mut String, addition: &str) {
    if addition.is_empty() {
        return;
    }
    if !target.is_empty() && !target.ends_with('\n') && !addition.starts_with('\n') {
        target.push('\n');
    }
    target.push_str(addition);
}

fn is_claude_user_prompt(entry: &Value) -> bool {
    if entry.get("type").and_then(|t| t.as_str()) != Some("user") {
        return false;
    }
    let message = match entry.get("message") {
        Some(message) => message,
        None => return false,
    };
    let content = match message.get("content") {
        Some(content) => content,
        None => return false,
    };

    if let Some(text) = content.as_str() {
        return !text.trim().is_empty();
    }

    let Some(blocks) = content.as_array() else {
        return false;
    };

    let mut has_text = false;
    let mut has_tool_result = false;
    for block in blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|t| !t.trim().is_empty())
                    .unwrap_or(false)
                {
                    has_text = true;
                }
            }
            Some("tool_result") => {
                has_tool_result = true;
            }
            _ => {}
        }
    }

    has_text || (!has_tool_result && !blocks.is_empty())
}

fn build_turn_summary(
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    usage: Option<(u64, u64)>,
) -> Option<TurnSummary> {
    let mut has_data = false;
    let mut summary = TurnSummary::new();

    if let Some((input, output)) = usage {
        if input > 0 || output > 0 {
            summary = summary.with_tokens(input, output);
            has_data = true;
        }
    }

    if let (Some(start), Some(end)) = (started_at, ended_at) {
        let duration = (end - start).num_seconds().max(0) as u64;
        if duration > 0 {
            summary = summary.with_duration(duration);
            has_data = true;
        }
    }

    if has_data {
        Some(summary)
    } else {
        None
    }
}

/// Debug entry for history loading - shows what happened to each JSONL line
#[derive(Debug, Clone)]
pub struct HistoryDebugEntry {
    /// Line number in the file (0-indexed)
    pub line_number: usize,
    /// Entry type from JSON (e.g., "response_item", "session_meta")
    pub entry_type: String,
    /// Status of this entry (INCLUDE, SKIP, ERROR)
    pub status: String,
    /// Reason for status (e.g., "role=user", "filtered: environment_context")
    pub reason: String,
    /// Raw JSON value
    pub raw_json: Value,
}

/// Error type for history loading
#[derive(Debug)]
pub enum HistoryError {
    HomeNotFound,
    StorageNotFound,
    SessionNotFound(String),
    IoError(std::io::Error),
    ParseError(String),
}

impl From<std::io::Error> for HistoryError {
    fn from(e: std::io::Error) -> Self {
        HistoryError::IoError(e)
    }
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryError::HomeNotFound => write!(f, "Home directory not found"),
            HistoryError::StorageNotFound => write!(f, "OpenCode storage directory not found"),
            HistoryError::SessionNotFound(id) => write!(f, "Session not found: {}", id),
            HistoryError::IoError(e) => write!(f, "IO error: {}", e),
            HistoryError::ParseError(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl std::error::Error for HistoryError {}

#[derive(Debug, Deserialize)]
struct OpencodeMessageInfo {
    id: String,
    role: String,
    #[serde(default)]
    time: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpencodeSessionInfo {
    id: String,
    #[serde(rename = "directory")]
    directory: Option<String>,
    #[serde(default)]
    time: Option<OpencodeSessionTime>,
}

#[derive(Debug, Deserialize)]
struct OpencodeSessionTime {
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    updated: Option<i64>,
}

impl OpencodeSessionTime {
    fn latest_timestamp(&self) -> i64 {
        self.updated.or(self.created).unwrap_or(0)
    }
}

fn opencode_storage_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        candidates.push(dir.join("opencode").join("storage"));
    }
    if let Some(dir) = dirs::data_dir() {
        candidates.push(dir.join("opencode").join("storage"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/share/opencode/storage"));
    }
    candidates
}

fn io_error_with_context(error: std::io::Error, context: String) -> std::io::Error {
    std::io::Error::new(error.kind(), format!("{context}: {error}"))
}

fn find_opencode_session_file(
    storage_dir: &Path,
    session_id: &str,
) -> Result<Option<PathBuf>, HistoryError> {
    let sessions_dir = storage_dir.join("session");
    let entries = fs::read_dir(&sessions_dir).map_err(|error| {
        HistoryError::IoError(io_error_with_context(
            error,
            format!(
                "Failed to read OpenCode sessions directory {}",
                sessions_dir.display()
            ),
        ))
    })?;
    for project_entry in entries {
        let project_entry = project_entry.map_err(|error| {
            HistoryError::IoError(io_error_with_context(
                error,
                format!(
                    "Failed to read OpenCode project entry in {}",
                    sessions_dir.display()
                ),
            ))
        })?;
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let candidate = project_path.join(format!("{session_id}.json"));
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn find_opencode_storage_for_session(session_id: &str) -> Result<(PathBuf, PathBuf), HistoryError> {
    let mut has_storage = false;
    for storage_dir in opencode_storage_dir_candidates() {
        if !storage_dir.exists() {
            continue;
        }
        has_storage = true;
        if let Some(session_file) = find_opencode_session_file(&storage_dir, session_id)? {
            return Ok((storage_dir, session_file));
        }
    }
    if !has_storage {
        Err(HistoryError::StorageNotFound)
    } else {
        Err(HistoryError::SessionNotFound(session_id.to_string()))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "Failed to canonicalize OpenCode history path"
            );
            path.to_path_buf()
        }
    }
}

fn opencode_paths_match(session_dir: &str, working_dir: &Path) -> bool {
    let session_path = PathBuf::from(session_dir);
    let session_norm = normalize_path(&session_path);
    let working_norm = normalize_path(working_dir);
    session_norm == working_norm
}

fn find_opencode_session_for_dir(
    storage_dir: &Path,
    working_dir: &Path,
) -> Result<Option<(String, PathBuf, i64)>, HistoryError> {
    let sessions_dir = storage_dir.join("session");
    let entries = fs::read_dir(&sessions_dir).map_err(|error| {
        HistoryError::IoError(io_error_with_context(
            error,
            format!(
                "Failed to read OpenCode sessions directory {}",
                sessions_dir.display()
            ),
        ))
    })?;
    let mut best: Option<(String, PathBuf, i64)> = None;

    for project_entry in entries {
        let project_entry = project_entry.map_err(|error| {
            HistoryError::IoError(io_error_with_context(
                error,
                format!(
                    "Failed to read OpenCode project entry in {}",
                    sessions_dir.display()
                ),
            ))
        })?;
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        for session_path in list_sorted_json(&project_path)? {
            let raw = fs::read_to_string(&session_path).map_err(|error| {
                HistoryError::IoError(io_error_with_context(
                    error,
                    format!(
                        "Failed to read OpenCode session file {}",
                        session_path.display()
                    ),
                ))
            })?;
            let info: OpencodeSessionInfo = serde_json::from_str(&raw).map_err(|error| {
                HistoryError::ParseError(format!(
                    "Failed to parse OpenCode session file {}: {}",
                    session_path.display(),
                    error
                ))
            })?;
            let directory = match info.directory.as_deref() {
                Some(directory) => directory,
                None => continue,
            };
            if !opencode_paths_match(directory, working_dir) {
                continue;
            }
            let updated = info
                .time
                .as_ref()
                .map(OpencodeSessionTime::latest_timestamp)
                .unwrap_or(0);
            let candidate = (info.id, session_path, updated);
            let should_replace = match best.as_ref() {
                Some((_, _, best_updated)) => updated > *best_updated,
                None => true,
            };
            if should_replace {
                best = Some(candidate);
            }
        }
    }

    Ok(best)
}

fn list_sorted_json(dir: &Path) -> Result<Vec<PathBuf>, HistoryError> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).map_err(|error| {
        HistoryError::IoError(io_error_with_context(
            error,
            format!("Failed to read directory {}", dir.display()),
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            HistoryError::IoError(io_error_with_context(
                error,
                format!("Failed to read directory entry in {}", dir.display()),
            ))
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        a_name.cmp(b_name)
    });
    Ok(files)
}

fn opencode_parts_for_message(
    storage_dir: &Path,
    message_id: &str,
) -> Result<Vec<Value>, HistoryError> {
    let parts_dir = storage_dir.join("part").join(message_id);
    if !parts_dir.exists() || !parts_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for path in list_sorted_json(&parts_dir)? {
        let raw = fs::read_to_string(&path).map_err(|error| {
            HistoryError::IoError(io_error_with_context(
                error,
                format!("Failed to read OpenCode part file {}", path.display()),
            ))
        })?;
        let value: Value = serde_json::from_str(&raw).map_err(|error| {
            HistoryError::ParseError(format!(
                "Failed to parse OpenCode part file {}: {}",
                path.display(),
                error
            ))
        })?;
        parts.push(value);
    }
    Ok(parts)
}

fn opencode_text_from_parts(parts: &[Value], include_reasoning: bool) -> (String, String) {
    let mut text = String::new();
    let mut reasoning = String::new();

    for part in parts {
        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if part_type == "reasoning" && !include_reasoning {
            continue;
        }
        if part_type != "text" && part_type != "reasoning" {
            continue;
        }
        if part.get("ignored").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        if part.get("synthetic").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let chunk = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if chunk.is_empty() {
            continue;
        }
        if part_type == "reasoning" {
            reasoning.push_str(chunk);
            reasoning.push('\n');
        } else {
            text.push_str(chunk);
            text.push('\n');
        }
    }

    if text.ends_with('\n') {
        text.pop();
    }
    if reasoning.ends_with('\n') {
        reasoning.pop();
    }

    (text, reasoning)
}

fn opencode_tool_output_from_state(state: &Value) -> Option<String> {
    let output = state.get("output").and_then(|v| v.as_str());
    if let Some(output) = output {
        if !output.trim().is_empty() {
            return Some(output.to_string());
        }
    }
    let meta_output = state
        .get("metadata")
        .and_then(|v| v.get("output"))
        .and_then(|v| v.as_str());
    if let Some(output) = meta_output {
        if !output.trim().is_empty() {
            return Some(output.to_string());
        }
    }
    let preview = state
        .get("metadata")
        .and_then(|v| v.get("preview"))
        .and_then(|v| v.as_str());
    if let Some(preview) = preview {
        if !preview.trim().is_empty() {
            return Some(preview.to_string());
        }
    }
    None
}

fn opencode_tool_args_from_state(state: &Value) -> String {
    let input = match state.get("input") {
        Some(input) => input,
        None => return String::new(),
    };

    if input.is_null() {
        return String::new();
    }
    if input.as_object().map(|obj| obj.is_empty()).unwrap_or(false) {
        return String::new();
    }

    if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
        return command.to_string();
    }
    if let Some(file_path) = input.get("filePath").and_then(|v| v.as_str()) {
        let mut args = file_path.to_string();
        let offset = input.get("offset").and_then(|v| v.as_i64());
        let limit = input.get("limit").and_then(|v| v.as_i64());
        if offset.is_some() || limit.is_some() {
            let offset = offset.unwrap_or(0);
            if let Some(limit) = limit {
                args.push_str(&format!(" (offset {}, limit {})", offset, limit));
            } else {
                args.push_str(&format!(" (offset {})", offset));
            }
        }
        return args;
    }

    serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string())
}

fn opencode_tool_message_from_part(part: &Value) -> Option<ChatMessage> {
    let tool = part.get("tool").and_then(|v| v.as_str())?;
    let state = part.get("state").unwrap_or(&Value::Null);
    let status = state.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let mut output = opencode_tool_output_from_state(state).unwrap_or_default();
    if output.trim().is_empty() {
        if let Some(error) = state.get("error").and_then(|v| v.as_str()) {
            output = error.to_string();
        }
    }
    if output.trim().is_empty() && !status.is_empty() {
        output = format!("status: {}", status);
    }

    let args = opencode_tool_args_from_state(state);
    let exit_code = state
        .get("metadata")
        .and_then(|v| v.get("exit"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .or_else(|| if status == "error" { Some(1) } else { None });

    Some(
        MessageDisplay::Tool {
            name: MessageDisplay::tool_display_name_owned(tool),
            args,
            output,
            exit_code,
            file_size: None,
        }
        .to_chat_message(),
    )
}

fn opencode_push_assistant_message(messages: &mut Vec<ChatMessage>, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    messages.push(
        MessageDisplay::Assistant {
            content: text.to_string(),
            is_streaming: false,
        }
        .to_chat_message(),
    );
    true
}

/// Load Claude Code history for a session
///
/// Claude stores sessions as `~/.claude/projects/{project-path}/{session-id}.jsonl`
pub fn load_claude_history(session_id: &str) -> Result<Vec<ChatMessage>, HistoryError> {
    let home = dirs::home_dir().ok_or(HistoryError::HomeNotFound)?;
    let projects_dir = home.join(".claude/projects");

    if !projects_dir.exists() {
        return Err(HistoryError::SessionNotFound(session_id.to_string()));
    }

    // Search all project directories for this session
    let session_file = find_claude_session_file(&projects_dir, session_id)?;
    parse_claude_history_file(&session_file)
}

/// Load Claude Code history with debug information
pub fn load_claude_history_with_debug(
    session_id: &str,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>, PathBuf), HistoryError> {
    let home = dirs::home_dir().ok_or(HistoryError::HomeNotFound)?;
    let projects_dir = home.join(".claude/projects");

    if !projects_dir.exists() {
        return Err(HistoryError::SessionNotFound(session_id.to_string()));
    }

    let session_file = find_claude_session_file(&projects_dir, session_id)?;
    let (messages, debug_entries) = parse_claude_history_file_with_debug(&session_file)?;
    Ok((messages, debug_entries, session_file))
}

/// Find Claude session file by searching project directories
fn find_claude_session_file(
    projects_dir: &PathBuf,
    session_id: &str,
) -> Result<PathBuf, HistoryError> {
    let filename = format!("{}.jsonl", session_id);

    // Iterate through project directories
    if let Ok(entries) = fs::read_dir(projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let session_path = path.join(&filename);
                if session_path.exists() {
                    return Ok(session_path);
                }
            }
        }
    }

    Err(HistoryError::SessionNotFound(session_id.to_string()))
}

/// Parse a Claude history JSONL file
fn parse_claude_history_file(path: &PathBuf) -> Result<Vec<ChatMessage>, HistoryError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // First pass: collect all entries and build tool_use lookup
    let mut entries = Vec::new();
    let mut tool_uses: HashMap<String, ClaudeToolUseInfo> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<Value>(&line) {
            // Extract tool_use blocks from assistant messages
            if entry.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(message) = entry.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                if let (Some(id), Some(name)) = (
                                    block.get("id").and_then(|i| i.as_str()),
                                    block.get("name").and_then(|n| n.as_str()),
                                ) {
                                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                                    tool_uses.insert(
                                        id.to_string(),
                                        ClaudeToolUseInfo {
                                            name: name.to_string(),
                                            input,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            entries.push(entry);
        }
    }

    Ok(build_claude_messages(&entries, &tool_uses))
}

/// Parse a Claude history JSONL file with debug information
fn parse_claude_history_file_with_debug(
    path: &PathBuf,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>), HistoryError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    // First pass: collect all entries and build tool_use lookup
    let mut raw_entries: Vec<(usize, Value)> = Vec::new();
    let mut tool_uses: HashMap<String, ClaudeToolUseInfo> = HashMap::new();
    let mut debug_entries = Vec::new();

    for (line_num, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(line) {
            Ok(entry) => {
                // Extract tool_use blocks from assistant messages
                if entry.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                    if let Some(message) = entry.get("message") {
                        if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                            for block in content {
                                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                    if let (Some(id), Some(name)) = (
                                        block.get("id").and_then(|i| i.as_str()),
                                        block.get("name").and_then(|n| n.as_str()),
                                    ) {
                                        let input =
                                            block.get("input").cloned().unwrap_or(Value::Null);
                                        tool_uses.insert(
                                            id.to_string(),
                                            ClaudeToolUseInfo {
                                                name: name.to_string(),
                                                input,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                raw_entries.push((line_num, entry));
            }
            Err(e) => {
                debug_entries.push(HistoryDebugEntry {
                    line_number: line_num,
                    entry_type: "parse_error".to_string(),
                    status: "ERROR".to_string(),
                    reason: e.to_string(),
                    raw_json: Value::String(line.clone()),
                });
            }
        }
    }

    // Second pass: convert entries to messages with debug info
    for (line_num, entry) in &raw_entries {
        let entry_type = entry
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string();

        let converted = convert_claude_entry_with_tools(entry, &tool_uses);
        let (status, reason) = convert_claude_entry_debug_info(entry, converted.len());

        debug_entries.push(HistoryDebugEntry {
            line_number: *line_num,
            entry_type,
            status: status.to_string(),
            reason,
            raw_json: entry.clone(),
        });
    }

    let entries: Vec<Value> = raw_entries.iter().map(|(_, entry)| entry.clone()).collect();
    let messages = build_claude_messages(&entries, &tool_uses);

    // Sort debug entries by line number
    debug_entries.sort_by_key(|e| e.line_number);

    Ok((messages, debug_entries))
}

fn build_claude_messages(
    entries: &[Value],
    tool_uses: &HashMap<String, ClaudeToolUseInfo>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut tracker = ClaudeTurnTracker::new();

    for entry in entries {
        let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if is_claude_user_prompt(entry) {
            if let Some(summary) = tracker.finish_turn() {
                messages.push(ChatMessage::turn_summary(summary));
            }
            tracker.start_turn(parse_timestamp(entry));
        }

        let converted = convert_claude_entry_with_tools(entry, tool_uses);
        messages.extend(converted);

        if matches!(entry_type, "assistant" | "result") {
            if let Some(usage) = extract_claude_usage(entry) {
                let request_id = entry.get("requestId").and_then(|id| id.as_str());
                tracker.update_assistant(request_id, usage, parse_timestamp(entry));
            }
        }
    }

    if let Some(summary) = tracker.finish_turn() {
        messages.push(ChatMessage::turn_summary(summary));
    }

    messages
}

/// Get debug info for a Claude entry conversion
fn convert_claude_entry_debug_info(
    entry: &Value,
    converted_count: usize,
) -> (&'static str, String) {
    let entry_type = match entry.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return ("SKIP", "missing type field".to_string()),
    };

    match entry_type {
        "user" => {
            if converted_count > 0 {
                if let Some(message) = entry.get("message") {
                    if let Some(content) = message.get("content") {
                        if content.is_string() {
                            let preview = content
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(50)
                                .collect::<String>();
                            return (
                                "INCLUDE",
                                format!("user message: {}...", preview.replace('\n', " ")),
                            );
                        } else if let Some(blocks) = content.as_array() {
                            let block_types: Vec<_> = blocks
                                .iter()
                                .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                                .collect();
                            return ("INCLUDE", format!("user blocks: {:?}", block_types));
                        }
                    }
                }
                ("INCLUDE", "user message".to_string())
            } else {
                ("SKIP", "user message produced no output".to_string())
            }
        }
        "assistant" => {
            if converted_count > 0 {
                if let Some(message) = entry.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                        let block_types: Vec<_> = content
                            .iter()
                            .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                            .collect();
                        return ("INCLUDE", format!("assistant blocks: {:?}", block_types));
                    }
                }
                ("INCLUDE", "assistant message".to_string())
            } else {
                ("SKIP", "assistant message with no text content".to_string())
            }
        }
        "result" => ("SKIP", "result entry (metadata)".to_string()),
        "summary" => ("SKIP", "summary entry (metadata)".to_string()),
        _ => ("SKIP", format!("unhandled type: {}", entry_type)),
    }
}

/// Convert a Claude JSONL entry to ChatMessage (legacy, used by tests)
#[cfg(test)]
fn convert_claude_entry(entry: &Value) -> Option<ChatMessage> {
    let entry_type = entry.get("type")?.as_str()?;

    match entry_type {
        "user" => {
            // User message: {"type":"user","message":{"role":"user","content":"..."}}
            let message = entry.get("message")?;
            let content = message.get("content")?.as_str()?;
            let display = MessageDisplay::User {
                content: content.to_string(),
            };
            Some(display.to_chat_message())
        }
        "assistant" => {
            // Assistant message: {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"..."}]}}
            let message = entry.get("message")?;
            let content = message.get("content")?;

            // Content can be a string or array of content blocks
            let text = if let Some(text) = content.as_str() {
                text.to_string()
            } else if let Some(blocks) = content.as_array() {
                // Extract text from content blocks
                blocks
                    .iter()
                    .filter_map(|block| {
                        let block_type = block.get("type")?.as_str()?;
                        match block_type {
                            "text" => block.get("text")?.as_str().map(|s| s.to_string()),
                            "tool_use" => {
                                // Summarize tool use
                                let name = block.get("name")?.as_str()?;
                                Some(format!("[Tool: {}]", name))
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                return None;
            };

            if text.is_empty() {
                return None;
            }

            let display = MessageDisplay::Assistant {
                content: text,
                is_streaming: false,
            };
            Some(display.to_chat_message())
        }
        _ => None, // Skip queue-operation and other types
    }
}

/// Convert a Claude JSONL entry to ChatMessages with proper tool handling
/// Returns a Vec because assistant messages with tool_use blocks may produce multiple messages
fn convert_claude_entry_with_tools(
    entry: &Value,
    tool_uses: &HashMap<String, ClaudeToolUseInfo>,
) -> Vec<ChatMessage> {
    let entry_type = match entry.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    match entry_type {
        "user" => {
            // User message can have string content OR array of content blocks (including tool_result)
            if let Some(message) = entry.get("message") {
                if let Some(content) = message.get("content") {
                    // String content - regular user message
                    if let Some(text) = content.as_str() {
                        let display = MessageDisplay::User {
                            content: text.to_string(),
                        };
                        return vec![display.to_chat_message()];
                    }

                    // Array content - may contain tool_result blocks
                    if let Some(blocks) = content.as_array() {
                        let mut messages = Vec::new();

                        for block in blocks {
                            let block_type = block.get("type").and_then(|t| t.as_str());

                            match block_type {
                                Some("tool_result") => {
                                    // Tool result inside user message
                                    if let Some(tool_use_id) =
                                        block.get("tool_use_id").and_then(|id| id.as_str())
                                    {
                                        if let Some(tool_info) = tool_uses.get(tool_use_id) {
                                            let result_content = block
                                                .get("content")
                                                .map(extract_tool_result_content)
                                                .unwrap_or_default();
                                            let is_error = block
                                                .get("is_error")
                                                .and_then(|e| e.as_bool())
                                                .unwrap_or(false);

                                            let args =
                                                format_tool_args(&tool_info.name, &tool_info.input);
                                            let output = if is_error {
                                                format!("Error: {}", result_content)
                                            } else {
                                                result_content
                                            };

                                            // Extract file size from toolUseResult.file.originalSize if present
                                            let file_size = entry
                                                .get("toolUseResult")
                                                .and_then(|r| r.get("file"))
                                                .and_then(|f| f.get("originalSize"))
                                                .and_then(|s| s.as_u64());

                                            let display = MessageDisplay::Tool {
                                                name: MessageDisplay::tool_display_name_owned(
                                                    &tool_info.name,
                                                ),
                                                args,
                                                output,
                                                exit_code: None,
                                                file_size,
                                            };
                                            messages.push(display.to_chat_message());
                                        }
                                    }
                                }
                                Some("text") => {
                                    // Text block in user message (rare but possible)
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                        let display = MessageDisplay::User {
                                            content: text.to_string(),
                                        };
                                        messages.push(display.to_chat_message());
                                    }
                                }
                                _ => {}
                            }
                        }

                        return messages;
                    }
                }
            }
            vec![]
        }
        "assistant" => {
            // Assistant message with content blocks
            let mut messages = Vec::new();

            if let Some(message) = entry.get("message") {
                if let Some(content) = message.get("content") {
                    if let Some(text) = content.as_str() {
                        // Simple string content
                        let display = MessageDisplay::Assistant {
                            content: text.to_string(),
                            is_streaming: false,
                        };
                        messages.push(display.to_chat_message());
                    } else if let Some(blocks) = content.as_array() {
                        // Extract only text blocks as assistant message
                        let texts: Vec<String> = blocks
                            .iter()
                            .filter_map(|block| {
                                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    block
                                        .get("text")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !texts.is_empty() {
                            let display = MessageDisplay::Assistant {
                                content: texts.join("\n"),
                                is_streaming: false,
                            };
                            messages.push(display.to_chat_message());
                        }

                        // Note: tool_use blocks are NOT added here
                        // They will be matched with tool_result in user messages
                    }
                }
            }
            messages
        }
        "tool_result" => {
            // Tool result: {"type":"tool_result","tool_use_id":"...","content":"...", "is_error":false}
            if let Some(tool_use_id) = entry.get("tool_use_id").and_then(|id| id.as_str()) {
                if let Some(tool_info) = tool_uses.get(tool_use_id) {
                    let content = entry
                        .get("content")
                        .map(extract_tool_result_content)
                        .unwrap_or_default();
                    let is_error = entry
                        .get("is_error")
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);

                    // Format arguments for display
                    let args = format_tool_args(&tool_info.name, &tool_info.input);

                    let output = if is_error {
                        format!("Error: {}", content)
                    } else {
                        content
                    };

                    let display = MessageDisplay::Tool {
                        name: MessageDisplay::tool_display_name_owned(&tool_info.name),
                        args,
                        output,
                        exit_code: None, // Claude doesn't provide exit codes
                        file_size: None, // Not available in this code path
                    };
                    return vec![display.to_chat_message()];
                }
            }
            vec![]
        }
        _ => vec![], // Skip queue-operation and other types
    }
}

/// Format tool arguments for display based on tool type
fn format_tool_args(tool_name: &str, input: &Value) -> String {
    let fallback = || serde_json::to_string(input).unwrap_or_default();

    match tool_name {
        "Bash" | "exec_command" | "shell" | "local_shell_call" | "command_execution" => {
            // Extract command from input
            input
                .get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(fallback)
        }
        "Read" | "read_file" => {
            // Extract file path
            input
                .get("file_path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(fallback)
        }
        "Write" | "write_file" => {
            // Extract file path
            input
                .get("file_path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(fallback)
        }
        "Edit" => {
            // Extract file path
            input
                .get("file_path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(fallback)
        }
        "Glob" => {
            // Extract pattern
            input
                .get("pattern")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(fallback)
        }
        "Grep" => {
            // Extract pattern and path
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = input.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            format!("{} in {}", pattern, path)
        }
        _ => {
            // Default: serialize the whole input
            fallback()
        }
    }
}

fn extract_tool_result_content(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                    continue;
                }
                if let Some(text) = item.get("content").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                    continue;
                }
                if let Some(text) = item.as_str() {
                    parts.push(text.to_string());
                    continue;
                }
                parts.push(item.to_string());
            }
            parts.join("\n")
        }
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Find Codex session file by searching recursively
fn find_codex_session_file(
    sessions_dir: &PathBuf,
    session_id: &str,
) -> Result<PathBuf, HistoryError> {
    // Walk through year/month/day directories
    for year_entry in fs::read_dir(sessions_dir)?.flatten() {
        let year_path = year_entry.path();
        if !year_path.is_dir() {
            continue;
        }

        for month_entry in fs::read_dir(&year_path).into_iter().flatten().flatten() {
            let month_path = month_entry.path();
            if !month_path.is_dir() {
                continue;
            }

            for day_entry in fs::read_dir(&month_path).into_iter().flatten().flatten() {
                let day_path = day_entry.path();
                if !day_path.is_dir() {
                    continue;
                }

                // Look for files containing the session ID
                for file_entry in fs::read_dir(&day_path).into_iter().flatten().flatten() {
                    let file_path = file_entry.path();
                    if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                        if name.contains(session_id) && name.ends_with(".jsonl") {
                            return Ok(file_path);
                        }
                    }
                }
            }
        }
    }

    Err(HistoryError::SessionNotFound(session_id.to_string()))
}

/// Parse a Codex history JSONL file with debug information
pub fn parse_codex_history_file_with_debug(
    path: &PathBuf,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>), HistoryError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    // First pass: collect function_call entries by call_id
    let mut function_calls: HashMap<String, FunctionCallInfo> = HashMap::new();
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            if let Some((call_id, info)) = extract_function_call_info(&entry) {
                function_calls.insert(call_id, info);
            }
        }
    }

    // Second pass: process all entries with function_call lookup
    let mut messages = Vec::new();
    let mut debug_entries = Vec::new();
    let mut tracker = CodexTurnTracker::new();
    let mut pending_exec_output: HashMap<i64, PendingExecOutput> = HashMap::new();

    for (line_num, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(line) {
            Ok(entry) => {
                let entry_type = entry
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                if entry_type == "turn.started" {
                    tracker.start_turn(parse_timestamp(&entry));
                    debug_entries.push(HistoryDebugEntry {
                        line_number: line_num,
                        entry_type,
                        status: "SKIP".to_string(),
                        reason: "turn started".to_string(),
                        raw_json: entry,
                    });
                    continue;
                }

                if entry_type == "turn.failed" {
                    tracker.finish_turn();
                    debug_entries.push(HistoryDebugEntry {
                        line_number: line_num,
                        entry_type,
                        status: "SKIP".to_string(),
                        reason: "turn failed".to_string(),
                        raw_json: entry,
                    });
                    continue;
                }

                if entry_type == "turn.completed" {
                    tracker.update_usage(
                        extract_codex_usage(&entry).unwrap_or((0, 0)),
                        parse_timestamp(&entry),
                    );
                    let summary = tracker.finish_turn();
                    let (status, reason) = if summary.is_some() {
                        ("INCLUDE", "turn summary".to_string())
                    } else {
                        ("SKIP", "turn summary missing data".to_string())
                    };
                    if let Some(summary) = summary {
                        messages.push(ChatMessage::turn_summary(summary));
                    }
                    debug_entries.push(HistoryDebugEntry {
                        line_number: line_num,
                        entry_type,
                        status: status.to_string(),
                        reason,
                        raw_json: entry,
                    });
                    continue;
                }

                if entry_type == "event_msg" {
                    let payload_type = entry
                        .get("payload")
                        .and_then(|p| p.get("type"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if payload_type == "token_count" {
                        if let Some(payload) = entry.get("payload") {
                            if let Some(usage) = extract_codex_usage(payload) {
                                tracker.update_usage(usage, parse_timestamp(&entry));
                            }
                        }
                        debug_entries.push(HistoryDebugEntry {
                            line_number: line_num,
                            entry_type,
                            status: "SKIP".to_string(),
                            reason: "token_count".to_string(),
                            raw_json: entry,
                        });
                        continue;
                    }
                }

                if entry_type == "response_item" {
                    if let Some(payload) = entry.get("payload") {
                        if payload.get("type").and_then(|t| t.as_str())
                            == Some("function_call_output")
                        {
                            let call_id = payload
                                .get("call_id")
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            let raw_output =
                                payload.get("output").and_then(|o| o.as_str()).unwrap_or("");
                            let call_info = function_calls.get(call_id);
                            let raw_name =
                                call_info.map(|info| info.name.as_str()).unwrap_or("shell");

                            if raw_name == "exec_command" {
                                if let Some(session_id) = parse_running_session_id(raw_output) {
                                    let (output, exit_code) =
                                        MessageDisplay::parse_codex_tool_output(raw_output);
                                    let command = call_info
                                        .map(|info| info.command.clone())
                                        .unwrap_or_default();
                                    let display = MessageDisplay::Tool {
                                        name: MessageDisplay::tool_display_name_owned(raw_name),
                                        args: command,
                                        output,
                                        exit_code,
                                        file_size: None,
                                    };
                                    messages.push(display.to_chat_message());
                                    pending_exec_output.insert(
                                        session_id,
                                        PendingExecOutput {
                                            message_index: messages.len().saturating_sub(1),
                                        },
                                    );
                                    debug_entries.push(HistoryDebugEntry {
                                        line_number: line_num,
                                        entry_type,
                                        status: "INCLUDE".to_string(),
                                        reason: format!(
                                            "exec_command output pending session {}",
                                            session_id
                                        ),
                                        raw_json: entry,
                                    });
                                    continue;
                                }
                            }

                            if raw_name == "write_stdin" {
                                if let Some(session_id) = call_info.and_then(|info| info.session_id)
                                {
                                    if let Some(pending) = pending_exec_output.get(&session_id) {
                                        let (output, exit_code) =
                                            MessageDisplay::parse_codex_tool_output(raw_output);
                                        if let Some(message) =
                                            messages.get_mut(pending.message_index)
                                        {
                                            append_output(&mut message.content, &output);
                                            if message.exit_code.is_none() {
                                                message.exit_code = exit_code;
                                            }
                                        }
                                        if exit_code.is_some() {
                                            pending_exec_output.remove(&session_id);
                                        }
                                        debug_entries.push(HistoryDebugEntry {
                                            line_number: line_num,
                                            entry_type,
                                            status: "SKIP".to_string(),
                                            reason: format!(
                                                "coalesced write_stdin for session {}",
                                                session_id
                                            ),
                                            raw_json: entry,
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }

                let (msg, status, reason) = convert_codex_entry_with_debug(&entry, &function_calls);
                if let Some(payload) = entry.get("payload") {
                    if payload.get("type").and_then(|t| t.as_str()) == Some("message") {
                        if payload.get("role").and_then(|r| r.as_str()) == Some("user") {
                            if let Some(summary) = tracker.finish_turn() {
                                messages.push(ChatMessage::turn_summary(summary));
                            }
                            tracker.start_turn(parse_timestamp(&entry));
                        }
                        if payload.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                            tracker.update_assistant(parse_timestamp(&entry));
                        }
                    }
                }

                debug_entries.push(HistoryDebugEntry {
                    line_number: line_num,
                    entry_type,
                    status: status.to_string(),
                    reason,
                    raw_json: entry,
                });

                if let Some(m) = msg {
                    messages.push(m);
                }
            }
            Err(e) => {
                debug_entries.push(HistoryDebugEntry {
                    line_number: line_num,
                    entry_type: "parse_error".to_string(),
                    status: "ERROR".to_string(),
                    reason: e.to_string(),
                    raw_json: Value::String(line.clone()),
                });
            }
        }
    }

    if let Some(summary) = tracker.finish_turn() {
        messages.push(ChatMessage::turn_summary(summary));
    }

    for pending in pending_exec_output.values() {
        if let Some(message) = messages.get_mut(pending.message_index) {
            if message.content.is_empty() {
                message.content = "Process still running.".to_string();
            }
        }
    }

    Ok((messages, debug_entries))
}

/// Extract function_call info from a response_item entry
fn extract_function_call_info(entry: &Value) -> Option<(String, FunctionCallInfo)> {
    let entry_type = entry.get("type")?.as_str()?;
    if entry_type != "response_item" {
        return None;
    }

    let payload = entry.get("payload")?;
    let payload_type = payload.get("type")?.as_str()?;
    if payload_type != "function_call" {
        return None;
    }

    let call_id = payload.get("call_id")?.as_str()?.to_string();
    let name = payload.get("name")?.as_str()?.to_string();

    // Parse arguments JSON to get command
    let args_str = payload.get("arguments")?.as_str()?;
    let args: Value = serde_json::from_str(args_str).ok()?;
    let command = args
        .get("command")
        .or_else(|| args.get("cmd"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let session_id = args.get("session_id").and_then(|v| v.as_i64());

    Some((
        call_id,
        FunctionCallInfo {
            name,
            command,
            session_id,
        },
    ))
}

/// Load Codex CLI history with debug information
pub fn load_codex_history_with_debug(
    session_id: &str,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>, PathBuf), HistoryError> {
    let home = dirs::home_dir().ok_or(HistoryError::HomeNotFound)?;
    let sessions_dir = home.join(".codex/sessions");

    if !sessions_dir.exists() {
        return Err(HistoryError::SessionNotFound(session_id.to_string()));
    }

    let session_file = find_codex_session_file(&sessions_dir, session_id)?;
    let (messages, debug_entries) = parse_codex_history_file_with_debug(&session_file)?;
    Ok((messages, debug_entries, session_file))
}

/// Load OpenCode history with debug information from storage.
pub fn load_opencode_history_with_debug(
    session_id: &str,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>, PathBuf), HistoryError> {
    let (storage_dir, _session_file) = find_opencode_storage_for_session(session_id)?;

    load_opencode_history_from_storage(&storage_dir, session_id)
}

fn load_opencode_history_from_storage(
    storage_dir: &Path,
    session_id: &str,
) -> Result<(Vec<ChatMessage>, Vec<HistoryDebugEntry>, PathBuf), HistoryError> {
    let session_file = find_opencode_session_file(storage_dir, session_id)?
        .ok_or_else(|| HistoryError::SessionNotFound(session_id.to_string()))?;

    let message_dir = storage_dir.join("message").join(session_id);
    if !message_dir.exists() {
        return Err(HistoryError::SessionNotFound(session_id.to_string()));
    }

    let mut records = Vec::new();
    for message_path in list_sorted_json(&message_dir)? {
        let raw = fs::read_to_string(&message_path).map_err(|error| {
            HistoryError::IoError(io_error_with_context(
                error,
                format!(
                    "Failed to read OpenCode message file {}",
                    message_path.display()
                ),
            ))
        })?;
        let raw_value: Value = serde_json::from_str(&raw).map_err(|error| {
            HistoryError::ParseError(format!(
                "Failed to parse OpenCode message file {}: {}",
                message_path.display(),
                error
            ))
        })?;
        let info: OpencodeMessageInfo = serde_json::from_value(raw_value.clone())
            .map_err(|e| HistoryError::ParseError(e.to_string()))?;
        let parts = opencode_parts_for_message(storage_dir, &info.id)?;
        let created = info
            .time
            .as_ref()
            .and_then(|t| t.get("created"))
            .and_then(|v| v.as_i64());
        records.push((created, info, parts, raw_value));
    }

    records.sort_by(|a, b| {
        let a_time = a.0.unwrap_or(0);
        let b_time = b.0.unwrap_or(0);
        if a_time == b_time {
            a.1.id.cmp(&b.1.id)
        } else {
            a_time.cmp(&b_time)
        }
    });

    let mut messages = Vec::new();
    let mut debug_entries = Vec::new();

    for (idx, (_created, info, parts, raw_info)) in records.into_iter().enumerate() {
        let mut status = "INCLUDE".to_string();
        let mut reason = format!("role={}", info.role);
        let raw_json = serde_json::json!({ "info": raw_info, "parts": parts });

        match raw_json
            .get("info")
            .and_then(|v| v.get("role"))
            .and_then(|v| v.as_str())
        {
            Some("user") => {
                let parts_val = raw_json
                    .get("parts")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let (text, _) = opencode_text_from_parts(&parts_val, false);
                if text.trim().is_empty() {
                    status = "SKIP".to_string();
                    reason = "user message empty".to_string();
                } else {
                    messages.push(MessageDisplay::User { content: text }.to_chat_message());
                }
            }
            Some("assistant") => {
                let info_val = raw_json.get("info");
                let summary = info_val
                    .and_then(|v| v.get("summary"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if summary {
                    status = "SKIP".to_string();
                    reason = "assistant summary".to_string();
                } else {
                    let parts_val = raw_json
                        .get("parts")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let mut pending_text = String::new();
                    let mut pending_reasoning = String::new();
                    let mut has_tool = false;
                    let mut has_text = false;

                    for part in &parts_val {
                        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match part_type {
                            "reasoning" => {
                                if part.get("ignored").and_then(|v| v.as_bool()) == Some(true) {
                                    continue;
                                }
                                if part.get("synthetic").and_then(|v| v.as_bool()) == Some(true) {
                                    continue;
                                }
                                let chunk = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                if chunk.is_empty() {
                                    continue;
                                }
                                append_output(&mut pending_reasoning, chunk);
                            }
                            "text" => {
                                if !pending_reasoning.trim().is_empty() {
                                    messages.push(
                                        MessageDisplay::Reasoning {
                                            content: pending_reasoning.clone(),
                                        }
                                        .to_chat_message(),
                                    );
                                    has_text = true;
                                    pending_reasoning.clear();
                                }
                                if part.get("ignored").and_then(|v| v.as_bool()) == Some(true) {
                                    continue;
                                }
                                if part.get("synthetic").and_then(|v| v.as_bool()) == Some(true) {
                                    continue;
                                }
                                let chunk = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                if chunk.is_empty() {
                                    continue;
                                }
                                append_output(&mut pending_text, chunk);
                            }
                            "tool" => {
                                if !pending_reasoning.trim().is_empty() {
                                    messages.push(
                                        MessageDisplay::Reasoning {
                                            content: pending_reasoning.clone(),
                                        }
                                        .to_chat_message(),
                                    );
                                    has_text = true;
                                    pending_reasoning.clear();
                                }
                                if opencode_push_assistant_message(&mut messages, &pending_text) {
                                    has_text = true;
                                }
                                pending_text.clear();
                                if let Some(tool_message) = opencode_tool_message_from_part(part) {
                                    messages.push(tool_message);
                                    has_tool = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    if !pending_reasoning.trim().is_empty() {
                        messages.push(
                            MessageDisplay::Reasoning {
                                content: pending_reasoning.clone(),
                            }
                            .to_chat_message(),
                        );
                        has_text = true;
                        pending_reasoning.clear();
                    }

                    if opencode_push_assistant_message(&mut messages, &pending_text) {
                        has_text = true;
                    }
                    if !has_text && !has_tool {
                        let error_message = info_val
                            .and_then(|v| v.get("error"))
                            .and_then(|v| v.get("message"))
                            .and_then(|v| v.as_str());
                        if let Some(error_message) = error_message {
                            messages.push(
                                MessageDisplay::Error {
                                    content: error_message.to_string(),
                                }
                                .to_chat_message(),
                            );
                            reason = "assistant error".to_string();
                        } else {
                            status = "SKIP".to_string();
                            reason = "assistant message empty".to_string();
                        }
                    }
                }
            }
            _ => {
                status = "SKIP".to_string();
                reason = "unsupported role".to_string();
            }
        }

        debug_entries.push(HistoryDebugEntry {
            line_number: idx,
            entry_type: "opencode_message".to_string(),
            status,
            reason,
            raw_json,
        });
    }

    Ok((messages, debug_entries, session_file))
}

/// Load OpenCode history for the latest session in the given working directory.
pub fn load_opencode_history_for_dir_with_debug(
    working_dir: &Path,
) -> Result<(String, Vec<ChatMessage>, Vec<HistoryDebugEntry>, PathBuf), HistoryError> {
    let mut has_storage = false;
    let mut best: Option<(PathBuf, String, PathBuf, i64)> = None;

    for storage_dir in opencode_storage_dir_candidates() {
        if !storage_dir.exists() {
            continue;
        }
        has_storage = true;
        if let Some((session_id, session_file, updated)) =
            find_opencode_session_for_dir(&storage_dir, working_dir)?
        {
            let should_replace = match best.as_ref() {
                Some((_, _, _, best_updated)) => updated > *best_updated,
                None => true,
            };
            if should_replace {
                best = Some((storage_dir, session_id, session_file, updated));
            }
        }
    }

    if !has_storage {
        return Err(HistoryError::StorageNotFound);
    }

    let (storage_dir, session_id, _session_file, _) = best
        .ok_or_else(|| HistoryError::SessionNotFound(format!("dir:{}", working_dir.display())))?;

    let (messages, debug_entries, session_file) =
        load_opencode_history_from_storage(&storage_dir, &session_id)?;

    Ok((session_id, messages, debug_entries, session_file))
}

/// Create a truncated preview of text for debug output
fn truncate_preview(text: &str, max_len: usize) -> String {
    let preview: String = text.chars().take(max_len).collect();
    if text.chars().count() > max_len {
        format!("{}...", preview.replace('\n', " "))
    } else {
        preview.replace('\n', " ")
    }
}

fn codex_text_skip_reason(text: &str) -> Option<&'static str> {
    if text.contains("<environment_context>") {
        return Some("filtered: environment_context");
    }
    if text.starts_with("# AGENTS.md instructions") {
        return Some("filtered: AGENTS.md instructions");
    }
    if text.contains("<INSTRUCTIONS>") {
        return Some("filtered: INSTRUCTIONS tags");
    }
    None
}

/// Extract text content from a Codex payload's content blocks
fn extract_text_content(payload: &Value) -> String {
    let content = match payload.get("content") {
        Some(c) => c,
        None => return String::new(),
    };

    if let Some(blocks) = content.as_array() {
        blocks
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type")?.as_str()?;
                match block_type {
                    "input_text" | "output_text" | "text" => {
                        block.get("text")?.as_str().map(|s| s.to_string())
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    }
}

/// Convert a Codex JSONL entry to ChatMessage with debug information
/// Returns (Option<ChatMessage>, status, reason)
fn convert_codex_entry_with_debug(
    entry: &Value,
    function_calls: &HashMap<String, FunctionCallInfo>,
) -> (Option<ChatMessage>, &'static str, String) {
    let entry_type = match entry.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return (None, "SKIP", "missing type field".to_string()),
    };

    let payload = match entry.get("payload") {
        Some(p) => p,
        None => return (None, "SKIP", "missing payload".to_string()),
    };

    match entry_type {
        "event_msg" => {
            let payload_type = payload.get("type").and_then(|t| t.as_str());
            if payload_type == Some("user_message") {
                let text = payload
                    .get("message")
                    .and_then(|message| message.as_str())
                    .unwrap_or("");
                if text.is_empty() {
                    return (None, "SKIP", "empty user_message text".to_string());
                }
                if let Some(reason) = codex_text_skip_reason(text) {
                    return (None, "SKIP", reason.to_string());
                }
                let preview = truncate_preview(text, 60);
                let display = MessageDisplay::User {
                    content: text.to_string(),
                };
                return (
                    Some(display.to_chat_message()),
                    "INCLUDE",
                    format!("event_msg user_message: \"{}\"", preview),
                );
            }
            (None, "SKIP", format!("event_msg type={:?}", payload_type))
        }

        "response_item" => {
            let payload_type = payload.get("type").and_then(|t| t.as_str());

            // Handle function_call_output (tool results)
            if payload_type == Some("function_call_output") {
                if let Some(raw_output) = payload.get("output").and_then(|o| o.as_str()) {
                    let call_id = payload
                        .get("call_id")
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown");

                    // Look up the function call to get the command
                    let (raw_name, command) = if let Some(info) = function_calls.get(call_id) {
                        (info.name.as_str(), info.command.clone())
                    } else {
                        ("shell", call_id.to_string())
                    };

                    // Parse Codex metadata wrapper to get clean output and exit code
                    let (output, exit_code) = MessageDisplay::parse_codex_tool_output(raw_output);

                    let display = MessageDisplay::Tool {
                        name: MessageDisplay::tool_display_name_owned(raw_name),
                        args: command.clone(),
                        output,
                        exit_code,
                        file_size: None, // Codex doesn't provide file size
                    };

                    let preview = truncate_preview(raw_output, 60);
                    return (
                        Some(display.to_chat_message()),
                        "INCLUDE",
                        format!(
                            "{}({}): \"{}\"",
                            MessageDisplay::tool_display_name(raw_name),
                            truncate_preview(&command, 30),
                            preview
                        ),
                    );
                }
            }

            // Handle regular messages with role
            let role = match payload.get("role").and_then(|r| r.as_str()) {
                Some(r) => r,
                None => {
                    return (
                        None,
                        "SKIP",
                        format!("role is null, type={:?}", payload_type),
                    )
                }
            };

            // Extract text content
            let text = extract_text_content(payload);
            if text.is_empty() {
                return (None, "SKIP", "empty text content".to_string());
            }

            // Filter system content
            if let Some(reason) = codex_text_skip_reason(&text) {
                return (None, "SKIP", reason.to_string());
            }

            let preview = truncate_preview(&text, 60);
            let display = match role {
                "user" => MessageDisplay::User { content: text },
                "assistant" => MessageDisplay::Assistant {
                    content: text,
                    is_streaming: false,
                },
                _ => return (None, "SKIP", format!("unknown role: {}", role)),
            };

            (
                Some(display.to_chat_message()),
                "INCLUDE",
                format!("role={}: \"{}\"", role, preview),
            )
        }

        _ => (None, "SKIP", format!("type={}", entry_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_convert_claude_user_entry() {
        let entry = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "Hello, Claude!"
            }
        });

        let msg = convert_claude_entry(&entry).unwrap();
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "Hello, Claude!");
    }

    #[test]
    fn test_convert_claude_assistant_entry() {
        let entry = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Hello!"},
                    {"type": "text", "text": "How can I help?"}
                ]
            }
        });

        let msg = convert_claude_entry(&entry).unwrap();
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.content, "Hello!\nHow can I help?");
    }

    #[test]
    fn test_skip_queue_operation() {
        let entry = serde_json::json!({
            "type": "queue-operation",
            "operation": "dequeue"
        });

        assert!(convert_claude_entry(&entry).is_none());
    }

    #[test]
    fn test_convert_codex_user_entry() {
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "What is the answer?"}
                ]
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "INCLUDE");
        let msg = msg.unwrap();
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "What is the answer?");
    }

    #[test]
    fn test_skip_environment_context() {
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "<environment_context>\n<cwd>/tmp</cwd>\n</environment_context>"}
                ]
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "SKIP");
        assert!(msg.is_none());
    }

    #[test]
    fn test_event_msg_user_message_included() {
        let entry = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": "Can you inspect the git log?"
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "INCLUDE");
        let msg = msg.unwrap();
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "Can you inspect the git log?");
    }

    #[test]
    fn test_event_msg_non_user_message_skipped() {
        let entry = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "input_tokens": 12,
                "output_tokens": 34
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "SKIP");
        assert!(msg.is_none());
    }

    #[test]
    fn test_function_call_output() {
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": "call_123",
                "output": "commit abc123\nAuthor: Test"
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "INCLUDE");
        let msg = msg.unwrap();
        assert_eq!(msg.role, MessageRole::Tool);
        assert!(msg.content.contains("commit abc123"));
    }

    #[test]
    fn test_function_call_output_with_lookup() {
        // Test that function_call_output looks up the command from function_calls
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": "call_123",
                "output": "commit abc123\nAuthor: Test"
            }
        });

        let mut function_calls = HashMap::new();
        function_calls.insert(
            "call_123".to_string(),
            FunctionCallInfo {
                name: "exec_command".to_string(),
                command: "git log -1 --stat".to_string(),
                session_id: None,
            },
        );

        let (msg, status, reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "INCLUDE");
        let msg = msg.unwrap();
        assert_eq!(msg.role, MessageRole::Tool);
        // Tool name is now mapped: exec_command -> Bash
        assert_eq!(msg.tool_name, Some("Bash".to_string()));
        assert_eq!(msg.tool_args, Some("git log -1 --stat".to_string()));
        assert!(reason.contains("Bash"));
        assert!(reason.contains("git log"));
    }

    #[test]
    fn test_skip_agents_md() {
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "# AGENTS.md instructions\n\nThis is system content..."}
                ]
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, reason) = convert_codex_entry_with_debug(&entry, &function_calls);
        assert_eq!(status, "SKIP");
        assert!(msg.is_none());
        assert!(reason.contains("AGENTS.md"));
    }

    /// Test that function_call entries are skipped in history parsing.
    /// This is expected behavior because function_call_output already creates
    /// the complete Tool message with both the command and output.
    /// For live events, item.started handles tool invocation display.
    #[test]
    fn test_function_call_entry_is_skipped_in_history() {
        // function_call entries don't have a "role" field, so they are skipped
        let entry = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"ls\"}",
                "call_id": "call_aeinHIp3JWOoInq6T7yjlGKx"
            }
        });
        let function_calls = HashMap::new();

        let (msg, status, _reason) = convert_codex_entry_with_debug(&entry, &function_calls);

        // function_call is skipped - the tool info is captured via extract_function_call_info
        // and used when function_call_output arrives
        assert_eq!(status, "SKIP");
        assert!(msg.is_none());
    }

    /// Test a full Codex tool call cycle: function_call followed by function_call_output
    /// The function_call is skipped but its info is used to enrich function_call_output
    #[test]
    fn test_codex_tool_call_cycle() {
        let entries = vec![
            // User message
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "List the files"}]
                }
            }),
            // Tool invocation (function_call) - will be skipped but info is captured
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"ls -la\"}",
                    "call_id": "call_test123"
                }
            }),
            // Tool output (function_call_output) - creates the Tool message
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call_test123",
                    "output": "Process exited with code 0\nOutput:\nfile1.txt\nfile2.txt"
                }
            }),
            // Assistant response
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Found 2 files."}]
                }
            }),
        ];

        // Build function_calls map (first pass) - captures function_call info
        let mut function_calls = HashMap::new();
        for entry in &entries {
            if let Some((call_id, info)) = extract_function_call_info(entry) {
                function_calls.insert(call_id, info);
            }
        }

        // Verify function_call info was captured
        assert!(function_calls.contains_key("call_test123"));

        // Process all entries (second pass)
        let mut messages = Vec::new();
        for entry in &entries {
            let (msg, status, _reason) = convert_codex_entry_with_debug(entry, &function_calls);
            if status == "INCLUDE" {
                if let Some(m) = msg {
                    messages.push(m);
                }
            }
        }

        // Should have 3 messages: User, Tool (from function_call_output), Assistant
        assert_eq!(
            messages.len(),
            3,
            "Expected 3 messages (user, tool, assistant), got {}",
            messages.len()
        );

        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[0].content, "List the files");

        // Tool message from function_call_output has the command looked up from function_calls
        assert_eq!(messages[1].role, MessageRole::Tool);
        assert_eq!(messages[1].tool_name, Some("Bash".to_string()));
        assert!(
            messages[1]
                .tool_args
                .as_ref()
                .is_some_and(|args| args.contains("ls")),
            "Tool args should contain the command from function_call lookup"
        );

        assert_eq!(messages[2].role, MessageRole::Assistant);
    }

    #[test]
    fn test_codex_coalesces_write_stdin_output() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let entries = vec![
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "Run check"}]
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"cargo check --all\"}",
                    "call_id": "call_exec"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call_exec",
                    "output": "Chunk ID: abc123\nProcess running with session ID 42\nOutput:\nfirst line\n"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "write_stdin",
                    "arguments": "{\"session_id\":42,\"chars\":\"\"}",
                    "call_id": "call_write"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call_write",
                    "output": "Chunk ID: def456\nProcess exited with code 0\nOutput:\nsecond line\n"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Done"}]
                }
            }),
        ];

        let mut file = NamedTempFile::new().expect("create temp file");
        for entry in &entries {
            let line = serde_json::to_string(entry).expect("serialize entry");
            writeln!(file, "{}", line).expect("write line");
        }

        let (messages, _) =
            parse_codex_history_file_with_debug(&file.path().to_path_buf()).expect("parse file");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, MessageRole::Tool);
        assert!(messages[1].content.contains("first line"));
        assert!(messages[1].content.contains("second line"));
        assert_eq!(messages[1].exit_code, Some(0));
    }

    /// Test parsing the fixture file - verifies function_call_output creates Tool messages
    #[test]
    fn test_codex_session_with_tool_calls_fixture() {
        use std::fs;
        use std::path::PathBuf;

        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("codex_tool_calls.jsonl");

        // Skip if fixture doesn't exist (for CI or fresh clones)
        if !fixture_path.exists() {
            eprintln!(
                "Skipping test: fixture file not found at {:?}",
                fixture_path
            );
            return;
        }

        let content = fs::read_to_string(&fixture_path).expect("Failed to read fixture");
        let lines: Vec<&str> = content.lines().collect();

        // Parse all entries
        let entries: Vec<serde_json::Value> = lines
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        // First pass: collect function_call info for later lookup
        let mut function_calls = HashMap::new();
        for entry in &entries {
            if let Some((call_id, info)) = extract_function_call_info(entry) {
                function_calls.insert(call_id, info);
            }
        }

        // Verify we captured function_call info
        assert!(
            function_calls.contains_key("call_aeinHIp3JWOoInq6T7yjlGKx"),
            "Should have extracted function_call info for call_id"
        );

        // Second pass: convert entries
        let mut messages = Vec::new();
        for entry in &entries {
            let (msg, status, _reason) = convert_codex_entry_with_debug(entry, &function_calls);
            if status == "INCLUDE" {
                if let Some(m) = msg {
                    messages.push(m);
                }
            }
        }

        // Verify we have Tool messages from function_call_output
        let tool_messages: Vec<_> = messages
            .iter()
            .filter(|m| m.role == MessageRole::Tool)
            .collect();

        assert!(
            !tool_messages.is_empty(),
            "Should have Tool messages from function_call_output"
        );

        // The tool message should show the ls command (looked up from function_calls)
        let has_ls_tool = tool_messages
            .iter()
            .any(|m| m.tool_args.as_ref().is_some_and(|args| args.contains("ls")));
        assert!(
            has_ls_tool,
            "Tool message should have 'ls' command from function_call lookup"
        );
    }

    #[test]
    fn test_opencode_history_includes_reasoning_parts() {
        let temp = TempDir::new().unwrap();
        let storage_dir = temp.path();
        let session_id = "ses_reasoning";

        let session_dir = storage_dir.join("session").join("project");
        fs::create_dir_all(&session_dir).unwrap();
        let session_file = session_dir.join(format!("{session_id}.json"));
        fs::write(
            &session_file,
            serde_json::json!({
                "id": session_id,
                "directory": "/tmp/project",
                "time": {"created": 1}
            })
            .to_string(),
        )
        .unwrap();

        let message_dir = storage_dir.join("message").join(session_id);
        fs::create_dir_all(&message_dir).unwrap();
        let message_file = message_dir.join("0001.json");
        fs::write(
            &message_file,
            serde_json::json!({
                "id": "msg_1",
                "role": "assistant",
                "time": {"created": 1}
            })
            .to_string(),
        )
        .unwrap();

        let parts_dir = storage_dir.join("part").join("msg_1");
        fs::create_dir_all(&parts_dir).unwrap();
        let part_file = parts_dir.join("0001.json");
        fs::write(
            &part_file,
            serde_json::json!({
                "id": "prt_1",
                "sessionID": session_id,
                "messageID": "msg_1",
                "type": "reasoning",
                "text": "Thinking about tests..."
            })
            .to_string(),
        )
        .unwrap();

        let (messages, _debug_entries, _session_path) =
            load_opencode_history_from_storage(storage_dir, session_id).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::Reasoning);
        assert!(messages[0].content.contains("Thinking about tests"));
    }

    #[test]
    fn test_opencode_tool_args_empty_input() {
        let state = serde_json::json!({"input": {}});
        assert!(opencode_tool_args_from_state(&state).is_empty());
    }

    #[test]
    fn test_opencode_history_missing_parts_dir() {
        let temp = TempDir::new().unwrap();
        let storage_dir = temp.path();
        let session_id = "ses_test";

        let session_dir = storage_dir.join("session").join("project");
        fs::create_dir_all(&session_dir).unwrap();
        let session_file = session_dir.join(format!("{session_id}.json"));
        fs::write(
            &session_file,
            serde_json::json!({
                "id": session_id,
                "directory": "/tmp/project",
                "time": {"created": 1}
            })
            .to_string(),
        )
        .unwrap();

        let message_dir = storage_dir.join("message").join(session_id);
        fs::create_dir_all(&message_dir).unwrap();
        let message_file = message_dir.join("0001.json");
        fs::write(
            &message_file,
            serde_json::json!({
                "id": "msg_1",
                "role": "assistant",
                "time": {"created": 1},
                "error": {"message": "Model missing"}
            })
            .to_string(),
        )
        .unwrap();

        let (messages, _debug_entries, _session_path) =
            load_opencode_history_from_storage(storage_dir, session_id).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::Error);
        assert!(messages[0].content.contains("Model missing"));
    }
}
