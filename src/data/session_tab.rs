//! Session tab data access object

use super::models::SessionTab;
use crate::agent::AgentType;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Data access object for session tab operations
#[derive(Clone)]
pub struct SessionTabStore {
    conn: Arc<Mutex<Connection>>,
}

impl SessionTabStore {
    /// Create a new SessionTabDao
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a new session tab
    pub fn create(&self, tab: &SessionTab) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_tabs (id, tab_index, workspace_id, agent_type, agent_mode, agent_session_id, model, pr_number, created_at, pending_user_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                tab.id.to_string(),
                tab.tab_index,
                tab.workspace_id.map(|id| id.to_string()),
                tab.agent_type.as_str(),
                tab.agent_mode,
                tab.agent_session_id,
                tab.model,
                tab.pr_number,
                tab.created_at.to_rfc3339(),
                tab.pending_user_message,
            ],
        )?;
        Ok(())
    }

    /// Get all session tabs ordered by tab_index
    pub fn get_all(&self) -> SqliteResult<Vec<SessionTab>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tab_index, workspace_id, agent_type, agent_mode, agent_session_id, model, pr_number, created_at, pending_user_message
             FROM session_tabs ORDER BY tab_index",
        )?;

        let tabs = stmt
            .query_map([], Self::row_to_session_tab)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tabs)
    }

    /// Get a session tab by ID
    pub fn get_by_id(&self, id: Uuid) -> SqliteResult<Option<SessionTab>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tab_index, workspace_id, agent_type, agent_mode, agent_session_id, model, pr_number, created_at, pending_user_message
             FROM session_tabs WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_session_tab(row)?))
        } else {
            Ok(None)
        }
    }

    /// Update a session tab
    pub fn update(&self, tab: &SessionTab) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE session_tabs SET tab_index = ?2, workspace_id = ?3, agent_type = ?4, agent_mode = ?5,
             agent_session_id = ?6, model = ?7, pr_number = ?8, pending_user_message = ?9 WHERE id = ?1",
            params![
                tab.id.to_string(),
                tab.tab_index,
                tab.workspace_id.map(|id| id.to_string()),
                tab.agent_type.as_str(),
                tab.agent_mode,
                tab.agent_session_id,
                tab.model,
                tab.pr_number,
                tab.pending_user_message,
            ],
        )?;
        Ok(())
    }

    /// Delete a session tab
    pub fn delete(&self, id: Uuid) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM session_tabs WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// Clear all session tabs
    pub fn clear_all(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM session_tabs", [])?;
        Ok(())
    }

    /// Get count of session tabs
    pub fn count(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM session_tabs", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Get a session tab by workspace_id
    pub fn get_by_workspace_id(&self, workspace_id: Uuid) -> SqliteResult<Option<SessionTab>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tab_index, workspace_id, agent_type, agent_mode, agent_session_id, model, pr_number, created_at, pending_user_message
             FROM session_tabs WHERE workspace_id = ?1",
        )?;

        let mut rows = stmt.query(params![workspace_id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_session_tab(row)?))
        } else {
            Ok(None)
        }
    }

    /// Convert a database row to a SessionTab
    /// Row order: id, tab_index, workspace_id, agent_type, agent_mode, agent_session_id, model, pr_number, created_at, pending_user_message
    fn row_to_session_tab(row: &rusqlite::Row) -> SqliteResult<SessionTab> {
        let id_str: String = row.get(0)?;
        let workspace_id_str: Option<String> = row.get(2)?;
        let agent_type_str: String = row.get(3)?;
        let created_at_str: String = row.get(8)?;

        Ok(SessionTab {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            tab_index: row.get(1)?,
            workspace_id: workspace_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
            agent_type: AgentType::parse(&agent_type_str),
            agent_mode: row.get(4)?,
            agent_session_id: row.get(5)?,
            model: row.get(6)?,
            pr_number: row.get(7)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            pending_user_message: row.get(9)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Database;
    use tempfile::tempdir;

    fn setup_db() -> (tempfile::TempDir, Database, SessionTabStore) {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();
        let dao = SessionTabStore::new(db.connection());
        (dir, db, dao)
    }

    #[test]
    fn test_create_and_get() {
        let (_dir, _db, dao) = setup_db();
        let tab = SessionTab::new(
            0,
            AgentType::Claude,
            None,
            Some("session-123".to_string()),
            None,
            None,
        );

        dao.create(&tab).unwrap();
        let retrieved = dao.get_by_id(tab.id).unwrap().unwrap();

        assert_eq!(retrieved.tab_index, 0);
        assert_eq!(retrieved.agent_type, AgentType::Claude);
        assert_eq!(retrieved.agent_session_id, Some("session-123".to_string()));
    }

    #[test]
    fn test_get_all_ordered() {
        let (_dir, _db, dao) = setup_db();

        let tab1 = SessionTab::new(1, AgentType::Codex, None, None, None, None);
        let tab0 = SessionTab::new(0, AgentType::Claude, None, None, None, None);

        dao.create(&tab1).unwrap();
        dao.create(&tab0).unwrap();

        let all = dao.get_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].tab_index, 0);
        assert_eq!(all[1].tab_index, 1);
    }

    #[test]
    fn test_clear_all() {
        let (_dir, _db, dao) = setup_db();

        let tab1 = SessionTab::new(0, AgentType::Claude, None, None, None, None);
        let tab2 = SessionTab::new(1, AgentType::Codex, None, None, None, None);

        dao.create(&tab1).unwrap();
        dao.create(&tab2).unwrap();
        assert_eq!(dao.count().unwrap(), 2);

        dao.clear_all().unwrap();
        assert_eq!(dao.count().unwrap(), 0);
    }

    #[test]
    fn test_update() {
        let (_dir, _db, dao) = setup_db();
        let mut tab = SessionTab::new(0, AgentType::Claude, None, None, None, None);

        dao.create(&tab).unwrap();

        tab.agent_session_id = Some("updated-session".to_string());
        tab.model = Some("claude-sonnet".to_string());
        tab.pr_number = Some(42);
        dao.update(&tab).unwrap();

        let retrieved = dao.get_by_id(tab.id).unwrap().unwrap();
        assert_eq!(
            retrieved.agent_session_id,
            Some("updated-session".to_string())
        );
        assert_eq!(retrieved.model, Some("claude-sonnet".to_string()));
        assert_eq!(retrieved.pr_number, Some(42));
    }
}
