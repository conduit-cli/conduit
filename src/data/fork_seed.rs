//! Fork seed data access object

use super::models::ForkSeed;
use crate::agent::AgentType;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Data access object for fork seed operations
#[derive(Clone)]
pub struct ForkSeedStore {
    conn: Arc<Mutex<Connection>>,
}

impl ForkSeedStore {
    /// Create a new ForkSeedStore
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a new fork seed
    pub fn create(&self, seed: &ForkSeed) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO fork_seeds (id, agent_type, parent_session_id, parent_workspace_id, created_at, seed_prompt_text, token_estimate, context_window, seed_ack_filtered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                seed.id.to_string(),
                seed.agent_type.as_str(),
                seed.parent_session_id,
                seed.parent_workspace_id.map(|id| id.to_string()),
                seed.created_at.to_rfc3339(),
                seed.seed_prompt_text,
                seed.token_estimate,
                seed.context_window,
                if seed.seed_ack_filtered { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    /// Get a fork seed by ID
    pub fn get_by_id(&self, id: Uuid) -> SqliteResult<Option<ForkSeed>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, agent_type, parent_session_id, parent_workspace_id, created_at, seed_prompt_text, token_estimate, context_window, seed_ack_filtered
             FROM fork_seeds WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_fork_seed(row)?))
        } else {
            Ok(None)
        }
    }

    /// Delete a fork seed by ID
    pub fn delete(&self, id: Uuid) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM fork_seeds WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    /// Convert a database row to a ForkSeed
    fn row_to_fork_seed(row: &rusqlite::Row) -> SqliteResult<ForkSeed> {
        let id_str: String = row.get(0)?;
        let agent_type_str: String = row.get(1)?;
        let parent_workspace_id_str: Option<String> = row.get(3)?;
        let created_at_str: String = row.get(4)?;
        let seed_ack_filtered: i64 = row.get(8)?;

        Ok(ForkSeed {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            agent_type: AgentType::parse(&agent_type_str),
            parent_session_id: row.get(2)?,
            parent_workspace_id: parent_workspace_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            seed_prompt_text: row.get(5)?,
            token_estimate: row.get(6)?,
            context_window: row.get(7)?,
            seed_ack_filtered: seed_ack_filtered != 0,
        })
    }
}
