//! SQLite database management

use rusqlite::{params, Connection, Result as SqliteResult};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// SQL schema for creating tables
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_path TEXT,
    repository_url TEXT,
    workspace_mode TEXT,
    archive_delete_branch INTEGER,
    archive_remote_prompt INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL,
    name TEXT NOT NULL,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_accessed TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    archived_at TEXT,
    archived_commit_sha TEXT,
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workspaces_repository ON workspaces(repository_id);

CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session_tabs (
    id TEXT PRIMARY KEY,
    tab_index INTEGER NOT NULL,
    is_open INTEGER NOT NULL DEFAULT 1,
    workspace_id TEXT,
    agent_type TEXT NOT NULL,
    agent_mode TEXT DEFAULT 'build',
    agent_session_id TEXT,
    model TEXT,
    model_invalid INTEGER NOT NULL DEFAULT 0,
    pr_number INTEGER,
    created_at TEXT NOT NULL,
    pending_user_message TEXT,
    queued_messages TEXT NOT NULL DEFAULT '[]',
    input_history TEXT NOT NULL DEFAULT '[]',
    fork_seed_id TEXT,
    title TEXT,
    title_generated INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_session_tabs_order ON session_tabs(tab_index);
CREATE UNIQUE INDEX IF NOT EXISTS idx_session_tabs_open_workspace
    ON session_tabs(workspace_id)
    WHERE is_open = 1 AND workspace_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS fork_seeds (
    id TEXT PRIMARY KEY,
    agent_type TEXT NOT NULL,
    parent_session_id TEXT,
    parent_workspace_id TEXT,
    created_at TEXT NOT NULL,
    seed_prompt_hash TEXT NOT NULL,
    seed_prompt_path TEXT,
    token_estimate INTEGER NOT NULL,
    context_window INTEGER NOT NULL,
    seed_ack_filtered INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (parent_workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_fork_seeds_parent_session ON fork_seeds(parent_session_id);
"#;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Failed to determine data directory")]
    NoDataDir,
    #[error("Failed to create data directory: {0}")]
    CreateDir(std::io::Error),
    #[error("Lock poisoned")]
    LockPoisoned,
}

/// Database connection wrapper
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    /// Path to the database file
    pub path: PathBuf,
}

fn hash_seed_prompt(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

impl Database {
    /// Open or create a database at the specified path
    pub fn open(path: PathBuf) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(DatabaseError::CreateDir)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        };

        db.initialize()?;
        Ok(db)
    }

    /// Open database in the default location (~/.conduit/conduit.db)
    pub fn open_default() -> Result<Self, DatabaseError> {
        Self::open(crate::util::database_path())
    }

    /// Initialize the database schema
    fn initialize(&self) -> Result<(), DatabaseError> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        conn.execute_batch(SCHEMA)?;
        drop(conn);

        // Apply migrations for existing databases
        self.apply_migrations()?;
        Ok(())
    }

    /// Apply database migrations for existing databases
    fn apply_migrations(&self) -> Result<(), DatabaseError> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;

        // Migration 1: Add archived_at column to workspaces table
        let has_archived_at: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name='archived_at'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archived_at {
            conn.execute("ALTER TABLE workspaces ADD COLUMN archived_at TEXT", [])?;
        }

        // Migration 2: Add pr_number column to session_tabs table
        let has_pr_number: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='pr_number'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_pr_number {
            conn.execute("ALTER TABLE session_tabs ADD COLUMN pr_number INTEGER", [])?;
        }

        // Migration 2b: Add model_invalid column to session_tabs table
        let has_model_invalid: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='model_invalid'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_model_invalid {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN model_invalid INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute(
                "UPDATE session_tabs SET model_invalid = 0 WHERE model_invalid IS NULL",
                [],
            )?;
        }

        // Migration 3: Add pending_user_message column to session_tabs table
        let has_pending_user_message: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='pending_user_message'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_pending_user_message {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN pending_user_message TEXT",
                [],
            )?;
        }

        // Migration 4: Add agent_mode column to session_tabs table
        let has_agent_mode: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='agent_mode'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_agent_mode {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN agent_mode TEXT DEFAULT 'build'",
                [],
            )?;
        }

        // Migration 5: Add archived_commit_sha column to workspaces table
        let has_archived_commit_sha: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name='archived_commit_sha'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archived_commit_sha {
            conn.execute(
                "ALTER TABLE workspaces ADD COLUMN archived_commit_sha TEXT",
                [],
            )?;
        }

        // Migration 6: Add queued_messages column to session_tabs table
        let has_queued_messages: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='queued_messages'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_queued_messages {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN queued_messages TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }

        conn.execute(
            "UPDATE session_tabs SET queued_messages = '[]' WHERE queued_messages IS NULL",
            [],
        )?;

        // Migration 7: Add fork_seed_id column to session_tabs table
        let has_fork_seed_id: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='fork_seed_id'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_fork_seed_id {
            conn.execute("ALTER TABLE session_tabs ADD COLUMN fork_seed_id TEXT", [])?;
        }

        // Migration 7: Replace fork_seeds seed_prompt_text with seed_prompt_hash/path
        let fork_seeds_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fork_seeds'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if fork_seeds_exists {
            let has_seed_prompt_hash: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('fork_seeds') WHERE name='seed_prompt_hash'",
                    [],
                    |row| row.get::<_, i64>(0).map(|c| c > 0),
                )
                .unwrap_or(false);

            if !has_seed_prompt_hash {
                // Verify old schema has seed_prompt_text column before attempting migration
                let has_seed_prompt_text: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pragma_table_info('fork_seeds') WHERE name='seed_prompt_text'",
                        [],
                        |row| row.get::<_, i64>(0).map(|c| c > 0),
                    )
                    .unwrap_or(false);

                if !has_seed_prompt_text {
                    // Old schema doesn't match expectations, skip migration
                    tracing::warn!(
                        "fork_seeds table exists but lacks seed_prompt_text column; skipping migration"
                    );
                    return Ok(());
                }

                conn.execute_batch(
                    r#"
CREATE TABLE IF NOT EXISTS fork_seeds_new (
    id TEXT PRIMARY KEY,
    agent_type TEXT NOT NULL,
    parent_session_id TEXT,
    parent_workspace_id TEXT,
    created_at TEXT NOT NULL,
    seed_prompt_hash TEXT NOT NULL,
    seed_prompt_path TEXT,
    token_estimate INTEGER NOT NULL,
    context_window INTEGER NOT NULL,
    seed_ack_filtered INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (parent_workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);
"#,
                )?;

                let mut stmt = conn.prepare(
                    "SELECT id, agent_type, parent_session_id, parent_workspace_id, created_at, seed_prompt_text, token_estimate, context_window, seed_ack_filtered FROM fork_seeds",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                    ))
                })?;

                for row in rows {
                    let (
                        id,
                        agent_type,
                        parent_session_id,
                        parent_workspace_id,
                        created_at,
                        seed_prompt_text,
                        token_estimate,
                        context_window,
                        seed_ack_filtered,
                    ) = row?;

                    let seed_prompt_hash = hash_seed_prompt(&seed_prompt_text);
                    conn.execute(
                        "INSERT INTO fork_seeds_new (id, agent_type, parent_session_id, parent_workspace_id, created_at, seed_prompt_hash, seed_prompt_path, token_estimate, context_window, seed_ack_filtered)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            id,
                            agent_type,
                            parent_session_id,
                            parent_workspace_id,
                            created_at,
                            seed_prompt_hash,
                            Option::<String>::None,
                            token_estimate,
                            context_window,
                            seed_ack_filtered,
                        ],
                    )?;
                }

                conn.execute("DROP TABLE fork_seeds", [])?;
                conn.execute("ALTER TABLE fork_seeds_new RENAME TO fork_seeds", [])?;
                conn.execute(
                    "CREATE INDEX IF NOT EXISTS idx_fork_seeds_parent_session ON fork_seeds(parent_session_id)",
                    [],
                )?;
            }
        }

        // Migration 8: Add title column to session_tabs table
        let has_title: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='title'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_title {
            conn.execute("ALTER TABLE session_tabs ADD COLUMN title TEXT", [])?;
        }

        // Migration 9: Add input_history column to session_tabs table
        let has_input_history: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='input_history'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_input_history {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN input_history TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }

        conn.execute(
            "UPDATE session_tabs SET input_history = '[]' WHERE input_history IS NULL",
            [],
        )?;

        // Migration 10: Add is_open column to session_tabs table
        let has_is_open: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='is_open'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_is_open {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN is_open INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
        }

        // Migration 11: Add title_generated column to session_tabs table
        let has_title_generated: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='title_generated'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_title_generated {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN title_generated INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }

        conn.execute(
            "UPDATE session_tabs SET title_generated = 1 WHERE title IS NOT NULL",
            [],
        )?;

        // Migration 12: Add workspace mode + archive settings to repositories table
        let has_workspace_mode: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('repositories') WHERE name='workspace_mode'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_workspace_mode {
            conn.execute(
                "ALTER TABLE repositories ADD COLUMN workspace_mode TEXT",
                [],
            )?;
        }

        let has_archive_delete_branch: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('repositories') WHERE name='archive_delete_branch'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archive_delete_branch {
            conn.execute(
                "ALTER TABLE repositories ADD COLUMN archive_delete_branch INTEGER",
                [],
            )?;
        }

        let has_archive_remote_prompt: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('repositories') WHERE name='archive_remote_prompt'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archive_remote_prompt {
            conn.execute(
                "ALTER TABLE repositories ADD COLUMN archive_remote_prompt INTEGER",
                [],
            )?;
        }

        conn.execute(
            "UPDATE repositories SET workspace_mode = 'worktree' WHERE workspace_mode IS NULL",
            [],
        )?;

        // Migration 12: Enforce at most one open session per workspace
        let has_open_workspace_index: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_session_tabs_open_workspace'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_open_workspace_index {
            // Close duplicate open sessions, keeping the newest per workspace.
            conn.execute_batch(
                r#"
WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY workspace_id
               ORDER BY datetime(created_at) DESC, id DESC
           ) AS rn
    FROM session_tabs
    WHERE is_open = 1 AND workspace_id IS NOT NULL
)
UPDATE session_tabs
SET is_open = 0
WHERE id IN (SELECT id FROM ranked WHERE rn > 1);
"#,
            )?;

            conn.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_session_tabs_open_workspace
                 ON session_tabs(workspace_id)
                 WHERE is_open = 1 AND workspace_id IS NOT NULL",
                [],
            )?;
        }

        Ok(())
    }

    /// Get a reference to the connection (for DAOs)
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// Execute a closure with the connection
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&Connection) -> SqliteResult<T>,
    {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        f(&conn).map_err(DatabaseError::Sqlite)
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let _db = Database::open(db_path.clone()).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn test_schema_initialization() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(db_path).unwrap();

        // Verify tables exist
        db.with_connection(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
            let tables: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            assert!(tables.contains(&"repositories".to_string()));
            assert!(tables.contains(&"workspaces".to_string()));
            Ok(())
        })
        .unwrap();
    }
}
