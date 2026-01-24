//! Session discovery and import utilities
//!
//! Provides functions to discover sessions from Claude Code, Codex CLI, and OpenCode,
//! and parse them for display in the import picker. (Gemini CLI discovery
//! is not supported yet.)

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::agent::AgentType;
use crate::session::cache::{get_file_mtime, SessionCache};

/// A session discovered from an external agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSession {
    /// Unique identifier (session file UUID)
    pub id: String,
    /// Agent type (Claude, Codex, or Gemini)
    pub agent_type: AgentType,
    /// Display text (first message or summary)
    pub display: String,
    /// Project path (if available)
    pub project: Option<String>,
    /// Session timestamp
    pub timestamp: DateTime<Utc>,
    /// Number of messages in the session
    pub message_count: usize,
    /// Path to the session file
    pub file_path: PathBuf,
}

impl ExternalSession {
    /// Get a relative time string (e.g., "2 hours ago")
    pub fn relative_time(&self) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.timestamp);

        let minutes = duration.num_minutes();
        let hours = duration.num_hours();
        let days = duration.num_days();

        if minutes < 1 {
            "just now".to_string()
        } else if minutes < 60 {
            format!("{} min ago", minutes)
        } else if hours < 24 {
            if hours == 1 {
                "1 hour ago".to_string()
            } else {
                format!("{} hours ago", hours)
            }
        } else if days == 1 {
            "Yesterday".to_string()
        } else if days < 7 {
            format!("{} days ago", days)
        } else if days < 30 {
            let weeks = days / 7;
            if weeks == 1 {
                "1 week ago".to_string()
            } else {
                format!("{} weeks ago", weeks)
            }
        } else if days < 365 {
            let months = days / 30;
            if months == 1 {
                "1 month ago".to_string()
            } else {
                format!("{} months ago", months)
            }
        } else {
            let years = days / 365;
            if years == 1 {
                "1 year ago".to_string()
            } else {
                format!("{} years ago", years)
            }
        }
    }

    /// Get a truncated display string
    pub fn truncated_display(&self, max_len: usize) -> String {
        let cleaned: String = self
            .display
            .chars()
            .filter(|c| !c.is_control() || *c == ' ')
            .collect();

        let char_count = cleaned.chars().count();
        if char_count <= max_len {
            cleaned
        } else {
            let take_len = max_len.saturating_sub(3);
            let truncated: String = cleaned.chars().take(take_len).collect();
            format!("{}...", truncated)
        }
    }

    /// Get the project name (last component of path)
    pub fn project_name(&self) -> Option<String> {
        self.project.as_ref().and_then(|p| {
            PathBuf::from(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
    }
}

/// Entry from Claude's history.jsonl index file
#[derive(Debug, Deserialize)]
struct ClaudeHistoryEntry {
    display: String,
    timestamp: i64,
    project: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

/// Discover all sessions from both Claude Code and Codex CLI
pub fn discover_all_sessions() -> Vec<ExternalSession> {
    let mut sessions = Vec::new();
    sessions.extend(discover_claude_sessions());
    sessions.extend(discover_codex_sessions());
    sessions.extend(discover_opencode_sessions());

    // Sort by timestamp descending (most recent first)
    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    sessions
}

// ============ Incremental Discovery with Caching ============

/// Update type for incremental session discovery
#[derive(Debug, Clone)]
pub enum SessionDiscoveryUpdate {
    /// Cached sessions loaded (fast path)
    CachedLoaded(Vec<ExternalSession>),
    /// Single session updated (new or modified)
    SessionUpdated(ExternalSession),
    /// Session removed (file no longer exists)
    SessionRemoved(PathBuf),
    /// Discovery complete
    Complete,
}

/// Discover sessions with caching - returns cached immediately, then updates incrementally
///
/// This function provides a two-tier discovery:
/// 1. Load and return cached sessions immediately via callback
/// 2. Scan filesystem for new/modified files and update incrementally
pub fn discover_sessions_incremental<F>(mut on_update: F)
where
    F: FnMut(SessionDiscoveryUpdate),
{
    // 1. Load cache
    let mut cache = SessionCache::load();

    // 2. Return cached sessions immediately via callback
    let cached_sessions = cache.get_cached_sessions();
    on_update(SessionDiscoveryUpdate::CachedLoaded(cached_sessions));

    // 3. Scan directories for file list (fast - no file reading, just paths and mtimes)
    let file_list = scan_session_files();

    // 4. Determine which files need reading
    let stale_files: Vec<_> = file_list
        .iter()
        .filter(|(path, mtime)| cache.needs_refresh(path, *mtime))
        .cloned()
        .collect();

    // 5. Read stale files, emit updates as we go
    for (path, mtime) in stale_files {
        if let Some(session) = read_single_session(&path) {
            cache.update(path, session.clone(), mtime);
            on_update(SessionDiscoveryUpdate::SessionUpdated(session));
        }
    }

    // 6. Remove deleted files from cache
    let existing: HashSet<_> = file_list.iter().map(|(p, _)| p.clone()).collect();
    let removed = cache.remove_missing(&existing);
    for path in removed {
        on_update(SessionDiscoveryUpdate::SessionRemoved(path));
    }

    // 7. Save updated cache
    cache.mark_refreshed();
    if let Err(e) = cache.save() {
        warn!("Failed to save session cache: {}", e);
    }

    // 8. Signal completion
    on_update(SessionDiscoveryUpdate::Complete);
}

/// Quick scan for session files without reading content
/// Returns (path, mtime) pairs for all session files
fn scan_session_files() -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return files,
    };

    // Scan Claude sessions
    let claude_dir = home.join(".claude");
    if claude_dir.exists() {
        files.extend(scan_claude_session_files(&claude_dir));
    }

    // Scan Codex sessions
    let codex_sessions_dir = home.join(".codex").join("sessions");
    if codex_sessions_dir.exists() {
        files.extend(scan_codex_session_files(&codex_sessions_dir));
    }

    // Scan OpenCode sessions
    if let Some(storage_dir) = opencode_storage_dir() {
        let opencode_sessions_dir = storage_dir.join("session");
        if opencode_sessions_dir.exists() {
            files.extend(scan_opencode_session_files(&opencode_sessions_dir));
        }
    }

    files
}

fn opencode_storage_dir() -> Option<PathBuf> {
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
    candidates.into_iter().find(|path| path.exists())
}

/// Scan Claude session files from projects directory
fn scan_claude_session_files(claude_dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return files;
    }

    // Walk project directories
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let project_path = entry.path();
            if !project_path.is_dir() {
                continue;
            }

            // Find .jsonl files in this project directory
            if let Ok(file_entries) = fs::read_dir(&project_path) {
                for file_entry in file_entries.flatten() {
                    let path = file_entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        if let Some(mtime) = get_file_mtime(&path) {
                            files.push((path, mtime));
                        }
                    }
                }
            }
        }
    }

    files
}

/// Walk Codex session files from YYYY/MM/DD directory structure.
fn walk_codex_session_files<F>(sessions_dir: &PathBuf, mut visit: F)
where
    F: FnMut(&PathBuf),
{
    if let Ok(year_entries) = fs::read_dir(sessions_dir) {
        for year_entry in year_entries.flatten() {
            let year_path = year_entry.path();
            if !year_path.is_dir() {
                continue;
            }

            if let Ok(month_entries) = fs::read_dir(&year_path) {
                for month_entry in month_entries.flatten() {
                    let month_path = month_entry.path();
                    if !month_path.is_dir() {
                        continue;
                    }

                    if let Ok(day_entries) = fs::read_dir(&month_path) {
                        for day_entry in day_entries.flatten() {
                            let day_path = day_entry.path();
                            if !day_path.is_dir() {
                                continue;
                            }

                            if let Ok(file_entries) = fs::read_dir(&day_path) {
                                for file_entry in file_entries.flatten() {
                                    let path = file_entry.path();
                                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                                        continue;
                                    }

                                    visit(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Scan Codex session files from YYYY/MM/DD directory structure
fn scan_codex_session_files(sessions_dir: &PathBuf) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    walk_codex_session_files(sessions_dir, |path| {
        if let Some(mtime) = get_file_mtime(path) {
            files.push((path.clone(), mtime));
        }
    });

    files
}

/// Scan OpenCode session files from storage/session/<project>/<session>.json
fn scan_opencode_session_files(sessions_dir: &PathBuf) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    if let Ok(project_entries) = fs::read_dir(sessions_dir) {
        for project_entry in project_entries.flatten() {
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }
            if let Ok(session_entries) = fs::read_dir(&project_path) {
                for session_entry in session_entries.flatten() {
                    let path = session_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Some(mtime) = get_file_mtime(&path) {
                        files.push((path, mtime));
                    }
                }
            }
        }
    }
    files
}

/// Read a single session file and return ExternalSession
fn read_single_session(path: &PathBuf) -> Option<ExternalSession> {
    let home = dirs::home_dir()?;

    // Determine session type based on path
    if path.starts_with(home.join(".claude")) {
        read_claude_session(path)
    } else if path.starts_with(home.join(".codex")) {
        parse_codex_session_file(path)
    } else if let Some(storage_dir) = opencode_storage_dir() {
        if path.starts_with(storage_dir.join("session")) {
            parse_opencode_session_file(path, &storage_dir)
        } else {
            None
        }
    } else {
        None
    }
}

/// Read a single Claude session file
fn read_claude_session(path: &PathBuf) -> Option<ExternalSession> {
    let home = dirs::home_dir()?;
    let claude_dir = home.join(".claude");
    let projects_dir = claude_dir.join("projects");

    // Extract project path from directory structure
    let relative = path.strip_prefix(&projects_dir).ok()?;
    let project_dir = relative.iter().next()?.to_str()?;
    let project_path = decode_project_path(project_dir);

    // Get session ID from filename
    let session_id = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Peek at file for message count and first message
    let (message_count, first_message) = peek_session_file(path);

    // Skip empty sessions
    if message_count == 0 {
        return None;
    }

    // Get timestamp from file modification time
    let timestamp = path
        .metadata()
        .and_then(|m| m.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());

    Some(ExternalSession {
        id: session_id,
        agent_type: AgentType::Claude,
        display: if first_message.is_empty() {
            "(No message)".to_string()
        } else {
            first_message
        },
        project: Some(project_path),
        timestamp,
        message_count,
        file_path: path.clone(),
    })
}

// ============ End Incremental Discovery ============

/// Discover Claude Code sessions from ~/.claude/
pub fn discover_claude_sessions() -> Vec<ExternalSession> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let claude_dir = home.join(".claude");
    if !claude_dir.exists() {
        return Vec::new();
    }

    // Try to read the history index first
    let history_file = claude_dir.join("history.jsonl");
    if history_file.exists() {
        if let Ok(sessions) = discover_claude_from_history(&history_file, &claude_dir) {
            return sessions;
        }
    }

    // Fallback: scan project directories directly
    discover_claude_from_projects(&claude_dir)
}

/// Discover Claude sessions using the history.jsonl index
fn discover_claude_from_history(
    history_file: &Path,
    claude_dir: &Path,
) -> Result<Vec<ExternalSession>, std::io::Error> {
    let file = File::open(history_file)?;
    let reader = BufReader::new(file);
    let mut sessions = Vec::new();
    let mut seen_sessions = std::collections::HashSet::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<ClaudeHistoryEntry>(&line) {
            // Skip if we've already seen this session
            if let Some(ref session_id) = entry.session_id {
                if seen_sessions.contains(session_id) {
                    continue;
                }
                seen_sessions.insert(session_id.clone());
            }

            // Try to find the session file
            if let Some(ref project) = entry.project {
                let encoded_path = encode_project_path(project);
                let mut project_dir = claude_dir.join("projects").join(&encoded_path);
                if !project_dir.exists() {
                    let legacy_encoded = encode_project_path_legacy(project);
                    if legacy_encoded != encoded_path {
                        let legacy_dir = claude_dir.join("projects").join(&legacy_encoded);
                        if legacy_dir.exists() {
                            project_dir = legacy_dir;
                        }
                    }
                }

                if project_dir.exists() {
                    // Find session files in this project directory
                    if let Ok(entries) = fs::read_dir(&project_dir) {
                        for file_entry in entries.flatten() {
                            let path = file_entry.path();
                            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                                let file_name =
                                    path.file_stem().and_then(|n| n.to_str()).unwrap_or("");

                                // Skip if already seen
                                if seen_sessions.contains(file_name) {
                                    continue;
                                }

                                // Check if this matches the session_id or is a recent file
                                let matches_session = entry
                                    .session_id
                                    .as_ref()
                                    .map(|id| file_name.contains(id))
                                    .unwrap_or(false);

                                // Use file metadata for timestamp if not matching
                                if matches_session || entry.session_id.is_none() {
                                    let (message_count, first_message) = peek_session_file(&path);

                                    let display = if !first_message.is_empty() {
                                        first_message
                                    } else {
                                        entry.display.clone()
                                    };

                                    let timestamp = Utc
                                        .timestamp_millis_opt(entry.timestamp)
                                        .single()
                                        .unwrap_or_else(Utc::now);

                                    seen_sessions.insert(file_name.to_string());

                                    sessions.push(ExternalSession {
                                        id: file_name.to_string(),
                                        agent_type: AgentType::Claude,
                                        display,
                                        project: Some(project.clone()),
                                        timestamp,
                                        message_count,
                                        file_path: path,
                                    });

                                    if matches_session {
                                        break; // Found the matching session
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(sessions)
}

/// Discover Claude sessions by scanning project directories
fn discover_claude_from_projects(claude_dir: &Path) -> Vec<ExternalSession> {
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    if let Ok(project_entries) = fs::read_dir(&projects_dir) {
        for project_entry in project_entries.flatten() {
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }

            let project_name = decode_project_path(
                project_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(""),
            );

            if let Ok(session_entries) = fs::read_dir(&project_path) {
                for session_entry in session_entries.flatten() {
                    let session_path = session_entry.path();
                    if session_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }

                    let session_id = session_path
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    // Get file modification time
                    let timestamp = session_path
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(DateTime::<Utc>::from)
                        .unwrap_or_else(|_| Utc::now());

                    let (message_count, first_message) = peek_session_file(&session_path);

                    sessions.push(ExternalSession {
                        id: session_id,
                        agent_type: AgentType::Claude,
                        display: first_message,
                        project: (!project_name.is_empty()).then(|| project_name.clone()),
                        timestamp,
                        message_count,
                        file_path: session_path,
                    });
                }
            }
        }
    }

    sessions
}

/// Discover Codex CLI sessions from ~/.codex/sessions/
pub fn discover_codex_sessions() -> Vec<ExternalSession> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let sessions_dir = home.join(".codex").join("sessions");
    if !sessions_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    walk_codex_session_files(&sessions_dir, |file_path| {
        if let Some(session) = parse_codex_session_file(file_path) {
            sessions.push(session);
        }
    });

    sessions
}

#[derive(Debug, Deserialize)]
struct OpencodeSessionTime {
    created: i64,
    updated: i64,
}

#[derive(Debug, Deserialize)]
struct OpencodeSessionInfo {
    id: String,
    title: String,
    directory: String,
    #[serde(default)]
    time: Option<OpencodeSessionTime>,
}

/// Discover OpenCode sessions from storage/session/<project>/<session>.json
pub fn discover_opencode_sessions() -> Vec<ExternalSession> {
    let storage_dir = match opencode_storage_dir() {
        Some(dir) => dir,
        None => return Vec::new(),
    };
    let sessions_dir = storage_dir.join("session");
    if !sessions_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    if let Ok(project_entries) = fs::read_dir(&sessions_dir) {
        for project_entry in project_entries.flatten() {
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }
            if let Ok(session_entries) = fs::read_dir(&project_path) {
                for session_entry in session_entries.flatten() {
                    let path = session_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Some(session) = parse_opencode_session_file(&path, &storage_dir) {
                        sessions.push(session);
                    }
                }
            }
        }
    }

    sessions
}

/// Parse a Codex session file and extract metadata
fn parse_codex_session_file(path: &PathBuf) -> Option<ExternalSession> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut message_count = 0;
    let mut first_user_message = String::new();
    let mut project: Option<String> = None;
    let mut timestamp: Option<DateTime<Utc>> = None;

    for line in reader.lines().take(100) {
        // Only read first 100 lines for efficiency
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Try to extract timestamp from thread.started event
        if timestamp.is_none() {
            if let Some(ts_millis) = entry.get("timestamp").and_then(|t| t.as_i64()) {
                timestamp = Utc.timestamp_millis_opt(ts_millis).single();
            }
        }

        // Extract project from thread.started event
        if project.is_none() && entry.get("type").and_then(|t| t.as_str()) == Some("event_msg") {
            if let Some(payload) = entry.get("payload") {
                if payload.get("type").and_then(|t| t.as_str()) == Some("thread.started") {
                    if let Some(cwd) = payload.get("cwd").and_then(|c| c.as_str()) {
                        project = Some(cwd.to_string());
                    }
                }
            }
        }

        // Fallback: extract project from session_meta payload when thread.started is missing
        if project.is_none() && entry.get("type").and_then(|t| t.as_str()) == Some("session_meta") {
            if let Some(payload) = entry.get("payload") {
                if let Some(meta) = payload.get("meta") {
                    if let Some(cwd) = meta.get("cwd").and_then(|c| c.as_str()) {
                        project = Some(cwd.to_string());
                    }
                }
            }
        }

        // Count messages and get first user message
        if entry.get("type").and_then(|t| t.as_str()) == Some("response_item") {
            if let Some(payload) = entry.get("payload") {
                let role = payload.get("role").and_then(|r| r.as_str());

                if role == Some("user") || role == Some("assistant") {
                    message_count += 1;

                    // Extract first user message for display
                    if first_user_message.is_empty() && role == Some("user") {
                        if let Some(content) = payload.get("content").and_then(|c| c.as_array()) {
                            for block in content {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    // Skip system content
                                    if !text.contains("<environment_context>")
                                        && !text.starts_with("# AGENTS.md")
                                        && !text.contains("<INSTRUCTIONS>")
                                    {
                                        first_user_message = text.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Skip sessions with no messages
    if message_count == 0 {
        return None;
    }

    // Extract session ID from filename (rollout-timestamp-uuid.jsonl)
    let filename = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let session_id = filename
        .strip_prefix("rollout-")
        .and_then(|s| s.split('-').next_back())
        .unwrap_or(filename)
        .to_string();

    // Use file modification time as fallback timestamp
    let timestamp = timestamp.unwrap_or_else(|| {
        path.metadata()
            .and_then(|m| m.modified())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now())
    });

    Some(ExternalSession {
        id: session_id,
        agent_type: AgentType::Codex,
        display: if first_user_message.is_empty() {
            "(No message)".to_string()
        } else {
            first_user_message
        },
        project,
        timestamp,
        message_count,
        file_path: path.clone(),
    })
}

/// Parse an OpenCode session file and extract metadata
fn parse_opencode_session_file(path: &Path, storage_dir: &Path) -> Option<ExternalSession> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "Failed to read OpenCode session file");
            return None;
        }
    };
    let info: OpencodeSessionInfo = match serde_json::from_str(&raw) {
        Ok(info) => info,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "Failed to parse OpenCode session file");
            return None;
        }
    };

    let message_dir = storage_dir.join("message").join(&info.id);
    let message_count = match fs::read_dir(&message_dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("json"))
            .count(),
        Err(err) => {
            warn!(dir = %message_dir.display(), error = %err, "Failed to read OpenCode message directory");
            return None;
        }
    };

    if message_count == 0 {
        return None;
    }

    let timestamp = info
        .time
        .as_ref()
        .and_then(|time| {
            let millis = time.updated.max(time.created);
            Utc.timestamp_millis_opt(millis).single()
        })
        .or_else(|| {
            path.metadata()
                .and_then(|m| m.modified())
                .map(DateTime::<Utc>::from)
                .ok()
        })
        .unwrap_or_else(Utc::now);

    Some(ExternalSession {
        id: info.id,
        agent_type: AgentType::Opencode,
        display: if info.title.trim().is_empty() {
            "(No title)".to_string()
        } else {
            info.title
        },
        project: (!info.directory.trim().is_empty()).then_some(info.directory),
        timestamp,
        message_count,
        file_path: path.to_path_buf(),
    })
}

/// Peek at a Claude session file to get message count and first user message
fn peek_session_file(path: &PathBuf) -> (usize, String) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (0, String::new()),
    };
    let reader = BufReader::new(file);

    let mut message_count = 0;
    let mut first_user_message = String::new();

    for line in reader.lines().take(50) {
        // Only peek first 50 lines
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let entry_type = entry.get("type").and_then(|t| t.as_str());

        if entry_type == Some("user") || entry_type == Some("assistant") {
            message_count += 1;

            // Get first user message for display
            if first_user_message.is_empty() && entry_type == Some("user") {
                if let Some(message) = entry.get("message") {
                    if let Some(content) = message.get("content") {
                        if let Some(text) = content.as_str() {
                            first_user_message = text.to_string();
                        } else if let Some(blocks) = content.as_array() {
                            for block in blocks {
                                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                        first_user_message = text.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (message_count, first_user_message)
}

/// Encode a project path for Claude's directory naming scheme.
///
/// This is a reversible encoding: we percent-encode '-' and '%' before
/// replacing '/' with '-'. That preserves hyphens in path segments.
/// e.g., /Users/john-doe/bar -> -Users-john%2Ddoe-bar
fn encode_project_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '%' => escaped.push_str("%25"),
            '-' => escaped.push_str("%2D"),
            _ => escaped.push(ch),
        }
    }
    escaped.replace('/', "-")
}

/// Legacy (lossy) encoding used by Claude: replace '/' with '-'.
fn encode_project_path_legacy(path: &str) -> String {
    path.replace('/', "-")
}

/// Decode a Claude project directory name back to a path.
///
/// If the name uses our reversible encoding, percent-decode after converting
/// '-' back to '/'. For legacy Claude names (which are lossy), this is
/// best-effort only.
fn decode_project_path(encoded: &str) -> String {
    let with_slashes = if encoded.starts_with('-') {
        encoded.replacen('-', "/", 1).replace('-', "/")
    } else {
        encoded.replace('-', "/")
    };
    percent_decode(&with_slashes)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_project_path() {
        assert_eq!(encode_project_path("/Users/foo/bar"), "-Users-foo-bar");
        assert_eq!(
            encode_project_path("/home/user/project"),
            "-home-user-project"
        );
        assert_eq!(
            encode_project_path("/Users/john-doe/projects/app"),
            "-Users-john%2Ddoe-projects-app"
        );
    }

    #[test]
    fn test_decode_project_path() {
        assert_eq!(decode_project_path("-Users-foo-bar"), "/Users/foo/bar");
        assert_eq!(
            decode_project_path("-home-user-project"),
            "/home/user/project"
        );
        assert_eq!(
            decode_project_path("-Users-john%2Ddoe-projects-app"),
            "/Users/john-doe/projects/app"
        );
    }

    #[test]
    fn test_encode_decode_round_trip_with_hyphen() {
        let path = "/Users/john-doe/projects/app";
        let encoded = encode_project_path(path);
        let decoded = decode_project_path(&encoded);
        assert_eq!(decoded, path);
    }

    #[test]
    fn test_relative_time() {
        let now = Utc::now();

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now,
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "just now");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::hours(2),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "2 hours ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(1),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "Yesterday");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(7),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "1 week ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(14),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "2 weeks ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(30),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "1 month ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(60),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "2 months ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(365),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "1 year ago");

        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: None,
            timestamp: now - chrono::Duration::days(800),
            message_count: 1,
            file_path: PathBuf::new(),
        };
        assert_eq!(session.relative_time(), "2 years ago");
    }

    #[test]
    fn test_truncated_display() {
        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "This is a very long message that should be truncated".to_string(),
            project: None,
            timestamp: Utc::now(),
            message_count: 1,
            file_path: PathBuf::new(),
        };

        let truncated = session.truncated_display(20);
        assert!(truncated.chars().count() <= 20);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_truncated_display_unicode() {
        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "こんにちは世界、これは長いメッセージです".to_string(),
            project: None,
            timestamp: Utc::now(),
            message_count: 1,
            file_path: PathBuf::new(),
        };

        let truncated = session.truncated_display(10);
        assert_eq!(truncated.chars().count(), 10);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated, "こんにちは世界...");
    }

    #[test]
    fn test_project_name() {
        let session = ExternalSession {
            id: "test".to_string(),
            agent_type: AgentType::Claude,
            display: "test".to_string(),
            project: Some("/Users/foo/my-project".to_string()),
            timestamp: Utc::now(),
            message_count: 1,
            file_path: PathBuf::new(),
        };

        assert_eq!(session.project_name(), Some("my-project".to_string()));
    }
}
