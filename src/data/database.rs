//! SQLite database management

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use super::migrations;

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

impl Database {
    /// Open or create a database at the specified path
    pub fn open(path: PathBuf) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(DatabaseError::CreateDir)?;
        }

        let mut conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Run migrations
        migrations::run_migrations(&mut conn)?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        };

        Ok(db)
    }

    /// Open database in the default location (~/.conduit/conduit.db)
    pub fn open_default() -> Result<Self, DatabaseError> {
        Self::open(crate::util::database_path())
    }

    /// Get a reference to the connection (for DAOs)
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// Execute a closure with the connection
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T>,
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
            assert!(tables.contains(&"schema_migrations".to_string()));
            Ok(())
        })
        .unwrap();
    }
}
