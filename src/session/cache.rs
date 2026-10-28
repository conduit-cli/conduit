//! Session discovery cache
//!
//! Provides persistent caching for discovered sessions to enable
//! instant display while background refresh validates/updates.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::session::ExternalSession;

/// Cached session entry with file metadata for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSessionEntry {
    pub session: ExternalSession,
    /// Unix timestamp of file modification time (for cache invalidation)
    pub file_mtime: u64,
}

/// Session discovery cache stored on disk
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionCache {
    /// Map of file path → cached session entry
    pub entries: HashMap<PathBuf, CachedSessionEntry>,
    /// Unix timestamp when cache was last fully refreshed
    pub last_refresh: Option<u64>,
}

impl SessionCache {
    /// Get the cache file path
    pub fn cache_path() -> PathBuf {
        crate::util::data_dir().join("sessions_cache.json")
    }

    /// Load cache from disk, returning empty cache if missing/corrupt
    pub fn load() -> Self {
        let path = Self::cache_path();
        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save cache to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::cache_path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    /// Get all cached sessions sorted by timestamp (most recent first)
    pub fn get_cached_sessions(&self) -> Vec<ExternalSession> {
        let mut sessions: Vec<_> = self
            .entries
            .values()
            .map(|entry| entry.session.clone())
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.timestamp));
        sessions
    }

    /// Check if a file needs to be re-read (new or modified)
    pub fn needs_refresh(&self, path: &Path, current_mtime: u64) -> bool {
        match self.entries.get(path) {
            Some(entry) => entry.file_mtime < current_mtime,
            None => true, // New file, not in cache
        }
    }

    /// Add or update a cache entry
    pub fn update(&mut self, path: PathBuf, session: ExternalSession, mtime: u64) {
        self.entries.insert(
            path,
            CachedSessionEntry {
                session,
                file_mtime: mtime,
            },
        );
    }

    /// Remove entries for files that no longer exist
    /// Returns the paths that were removed
    pub fn remove_missing(&mut self, existing_paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
        let to_remove: Vec<PathBuf> = self
            .entries
            .keys()
            .filter(|path| !existing_paths.contains(*path))
            .cloned()
            .collect();

        for path in &to_remove {
            self.entries.remove(path);
        }

        to_remove
    }

    /// Update the last refresh timestamp
    pub fn mark_refreshed(&mut self) {
        self.last_refresh = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
    }
}

/// Get file modification time as unix timestamp
pub fn get_file_mtime(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}
