//! Database migrations using a proper versioned migration pattern.
//!
//! Each migration runs exactly once and is tracked in the `schema_migrations` table.
//! Migrations are applied in order by version number.

use rusqlite::{params, Connection};

/// A database migration with a version number, name, and SQL to execute.
pub struct Migration {
    /// Unique version number (migrations run in order)
    pub version: i64,
    /// Human-readable name for the migration
    pub name: &'static str,
    /// SQL to execute (can be multiple statements)
    pub sql: &'static str,
}

/// All migrations in order. New migrations should be added at the end.
pub const MIGRATIONS: &[Migration] = &[
    // ============================================================
    // Initial schema (v1-v4)
    // ============================================================
    Migration {
        version: 1,
        name: "create_repositories_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS repositories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                base_path TEXT,
                repository_url TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#,
    },
    Migration {
        version: 2,
        name: "create_workspaces_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                name TEXT NOT NULL,
                branch TEXT NOT NULL,
                path TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_workspaces_repository ON workspaces(repository_id);
        "#,
    },
    Migration {
        version: 3,
        name: "create_app_state_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS app_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#,
    },
    Migration {
        version: 4,
        name: "create_session_tabs_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS session_tabs (
                id TEXT PRIMARY KEY,
                tab_index INTEGER NOT NULL,
                workspace_id TEXT,
                agent_type TEXT NOT NULL,
                agent_session_id TEXT,
                model TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_tabs_order ON session_tabs(tab_index);
        "#,
    },
    // ============================================================
    // Incremental migrations (v5+)
    // ============================================================
    Migration {
        version: 5,
        name: "add_workspaces_archived_at",
        sql: "ALTER TABLE workspaces ADD COLUMN archived_at TEXT;",
    },
    Migration {
        version: 6,
        name: "add_session_tabs_pr_number",
        sql: "ALTER TABLE session_tabs ADD COLUMN pr_number INTEGER;",
    },
    Migration {
        version: 7,
        name: "add_session_tabs_pending_user_message",
        sql: "ALTER TABLE session_tabs ADD COLUMN pending_user_message TEXT;",
    },
    Migration {
        version: 8,
        name: "add_session_tabs_agent_mode",
        sql: "ALTER TABLE session_tabs ADD COLUMN agent_mode TEXT DEFAULT 'build';",
    },
    Migration {
        version: 9,
        name: "add_workspaces_archived_commit_sha",
        sql: "ALTER TABLE workspaces ADD COLUMN archived_commit_sha TEXT;",
    },
    Migration {
        version: 10,
        name: "add_session_tabs_queued_messages",
        sql: r#"
            ALTER TABLE session_tabs ADD COLUMN queued_messages TEXT NOT NULL DEFAULT '[]';
        "#,
    },
    Migration {
        version: 11,
        name: "add_session_tabs_fork_seed_id",
        sql: "ALTER TABLE session_tabs ADD COLUMN fork_seed_id TEXT;",
    },
    Migration {
        version: 12,
        name: "create_fork_seeds_table",
        sql: r#"
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
        "#,
    },
    Migration {
        version: 13,
        name: "add_session_tabs_title",
        sql: "ALTER TABLE session_tabs ADD COLUMN title TEXT;",
    },
    Migration {
        version: 14,
        name: "add_session_tabs_input_history",
        sql: r#"
            ALTER TABLE session_tabs ADD COLUMN input_history TEXT NOT NULL DEFAULT '[]';
        "#,
    },
    Migration {
        version: 15,
        name: "add_session_tabs_is_open",
        sql: "ALTER TABLE session_tabs ADD COLUMN is_open INTEGER NOT NULL DEFAULT 1;",
    },
    Migration {
        version: 16,
        name: "add_session_tabs_title_generated",
        sql: "ALTER TABLE session_tabs ADD COLUMN title_generated INTEGER NOT NULL DEFAULT 0;",
    },
    Migration {
        version: 17,
        name: "add_repositories_workspace_settings",
        sql: r#"
            ALTER TABLE repositories ADD COLUMN workspace_mode TEXT;
            ALTER TABLE repositories ADD COLUMN archive_delete_branch INTEGER;
            ALTER TABLE repositories ADD COLUMN archive_remote_prompt INTEGER;
        "#,
    },
    Migration {
        version: 18,
        name: "create_session_tabs_open_workspace_index",
        sql: r#"
            -- Close duplicate open sessions, keeping the newest per workspace
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

            -- Create unique index to enforce one open session per workspace
            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_tabs_open_workspace
                ON session_tabs(workspace_id)
                WHERE is_open = 1 AND workspace_id IS NOT NULL;
        "#,
    },
    Migration {
        version: 19,
        name: "add_session_tabs_model_invalid",
        sql: "ALTER TABLE session_tabs ADD COLUMN model_invalid INTEGER NOT NULL DEFAULT 0;",
    },
];

/// Create the schema_migrations table if it doesn't exist.
fn ensure_migrations_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// Get the set of already-applied migration versions.
fn get_applied_versions(conn: &Connection) -> rusqlite::Result<std::collections::HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations")?;
    let versions = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<std::collections::HashSet<i64>>>()?;
    Ok(versions)
}

/// Check if a column exists in a table.
fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name='{}'",
            table, column
        ),
        [],
        |row| row.get::<_, i64>(0).map(|c| c > 0),
    )
}

/// Check if a table exists.
fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get::<_, i64>(0).map(|c| c > 0),
    )
}

/// Check if an index exists.
fn index_exists(conn: &Connection, index: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
        [index],
        |row| row.get::<_, i64>(0).map(|c| c > 0),
    )
}

/// Bootstrap existing databases that predate the migration system.
///
/// This detects what schema already exists and marks those migrations as applied
/// without re-running them.
fn bootstrap_existing_database(conn: &Connection) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // If repositories table exists, this is an existing database
    if !table_exists(conn, "repositories")? {
        return Ok(()); // Fresh database, nothing to bootstrap
    }

    tracing::info!("Bootstrapping existing database into migration system");

    // Collect migrations to mark as applied
    let mut to_mark: Vec<&Migration> = Vec::new();

    // Check each migration and collect if its changes already exist
    for migration in MIGRATIONS {
        let already_applied = match migration.version {
            1 => table_exists(conn, "repositories")?,
            2 => table_exists(conn, "workspaces")?,
            3 => table_exists(conn, "app_state")?,
            4 => table_exists(conn, "session_tabs")?,
            5 => column_exists(conn, "workspaces", "archived_at")?,
            6 => column_exists(conn, "session_tabs", "pr_number")?,
            7 => column_exists(conn, "session_tabs", "pending_user_message")?,
            8 => column_exists(conn, "session_tabs", "agent_mode")?,
            9 => column_exists(conn, "workspaces", "archived_commit_sha")?,
            10 => column_exists(conn, "session_tabs", "queued_messages")?,
            11 => column_exists(conn, "session_tabs", "fork_seed_id")?,
            12 => table_exists(conn, "fork_seeds")?,
            13 => column_exists(conn, "session_tabs", "title")?,
            14 => column_exists(conn, "session_tabs", "input_history")?,
            15 => column_exists(conn, "session_tabs", "is_open")?,
            16 => column_exists(conn, "session_tabs", "title_generated")?,
            17 => column_exists(conn, "repositories", "workspace_mode")?,
            18 => index_exists(conn, "idx_session_tabs_open_workspace")?,
            19 => column_exists(conn, "session_tabs", "model_invalid")?,
            _ => false,
        };

        if already_applied {
            to_mark.push(migration);
        }
    }

    // Insert all bootstrap records in a single transaction
    if !to_mark.is_empty() {
        let inserts: Vec<String> = to_mark
            .iter()
            .map(|m| {
                format!(
                    "INSERT OR IGNORE INTO schema_migrations (version, name, applied_at) VALUES ({}, '{}', '{}');",
                    m.version, m.name, now
                )
            })
            .collect();
        let sql = format!("BEGIN;\n{}\nCOMMIT;", inserts.join("\n"));
        conn.execute_batch(&sql)?;
    }

    Ok(())
}

/// Run all pending migrations.
///
/// This is the main entry point for the migration system.
pub fn run_migrations(conn: &mut Connection) -> rusqlite::Result<()> {
    // Ensure the migrations table exists
    ensure_migrations_table(conn)?;

    // Bootstrap existing databases that predate the migration system
    bootstrap_existing_database(conn)?;

    // Get already-applied migrations
    let applied = get_applied_versions(conn)?;

    // Apply pending migrations in order
    for migration in MIGRATIONS {
        if applied.contains(&migration.version) {
            continue;
        }

        tracing::info!(
            version = migration.version,
            name = migration.name,
            "Applying migration"
        );

        // Execute the migration SQL and record it within a single transaction for atomicity
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction()?;
        if let Err(e) = tx.execute_batch(migration.sql) {
            tracing::error!(
                version = migration.version,
                name = migration.name,
                error = %e,
                "Migration failed"
            );
            return Err(e);
        }
        if let Err(e) = tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, now],
        ) {
            tracing::error!(
                version = migration.version,
                name = migration.name,
                error = %e,
                "Migration failed"
            );
            return Err(e);
        }
        if let Err(e) = tx.commit() {
            tracing::error!(
                version = migration.version,
                name = migration.name,
                error = %e,
                "Migration failed"
            );
            return Err(e);
        }

        tracing::info!(
            version = migration.version,
            name = migration.name,
            "Migration applied successfully"
        );
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_migrations_have_unique_versions() {
        let mut versions = std::collections::HashSet::new();
        for migration in MIGRATIONS {
            assert!(
                versions.insert(migration.version),
                "Duplicate migration version: {}",
                migration.version
            );
        }
    }

    #[test]
    fn test_migrations_are_ordered() {
        let mut last_version = 0;
        for migration in MIGRATIONS {
            assert!(
                migration.version > last_version,
                "Migrations must be in ascending order: {} should come after {}",
                migration.version,
                last_version
            );
            last_version = migration.version;
        }
    }

    #[test]
    fn test_fresh_database_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        // Verify all migrations were recorded
        let applied = get_applied_versions(&conn).unwrap();
        assert_eq!(applied.len(), MIGRATIONS.len());

        // Verify tables exist
        assert!(table_exists(&conn, "repositories").unwrap());
        assert!(table_exists(&conn, "workspaces").unwrap());
        assert!(table_exists(&conn, "session_tabs").unwrap());
        assert!(table_exists(&conn, "fork_seeds").unwrap());
        assert!(table_exists(&conn, "schema_migrations").unwrap());
    }

    #[test]
    fn test_idempotent_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();

        // Run migrations twice
        run_migrations(&mut conn).unwrap();
        run_migrations(&mut conn).unwrap();

        // Should still have same number of recorded migrations
        let applied = get_applied_versions(&conn).unwrap();
        assert_eq!(applied.len(), MIGRATIONS.len());
    }
}
