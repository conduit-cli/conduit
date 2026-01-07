use std::path::PathBuf;

use crate::agent::AgentType;
use crate::ui::session::AgentSession;

/// Manages multiple agent sessions as tabs
pub struct TabManager {
    /// All active sessions
    sessions: Vec<AgentSession>,
    /// Index of the currently active tab
    active_tab: usize,
    /// Maximum number of tabs allowed
    max_tabs: usize,
}

impl TabManager {
    pub fn new(max_tabs: usize) -> Self {
        Self {
            sessions: Vec::new(),
            active_tab: 0,
            max_tabs,
        }
    }

    /// Create a new tab with the given agent type
    pub fn new_tab(&mut self, agent_type: AgentType) -> Option<usize> {
        if self.sessions.len() >= self.max_tabs {
            return None;
        }

        let session = AgentSession::new(agent_type);
        self.sessions.push(session);
        let new_index = self.sessions.len() - 1;
        self.active_tab = new_index;
        Some(new_index)
    }

    /// Create a new tab with the given agent type and working directory
    pub fn new_tab_with_working_dir(
        &mut self,
        agent_type: AgentType,
        working_dir: PathBuf,
    ) -> Option<usize> {
        if self.sessions.len() >= self.max_tabs {
            return None;
        }

        let session = AgentSession::with_working_dir(agent_type, working_dir);
        self.sessions.push(session);
        let new_index = self.sessions.len() - 1;
        self.active_tab = new_index;
        Some(new_index)
    }

    /// Close a tab by index
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.sessions.len() {
            return false;
        }

        self.sessions.remove(index);

        // Adjust active tab if needed
        if self.active_tab >= self.sessions.len() {
            self.active_tab = self.sessions.len().saturating_sub(1);
        } else if self.active_tab > index {
            self.active_tab = self.active_tab.saturating_sub(1);
        }

        true
    }

    /// Switch to a specific tab
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.sessions.len() {
            self.active_tab = index;
            // Clear needs_attention flag when switching to a tab
            if let Some(session) = self.sessions.get_mut(index) {
                session.needs_attention = false;
            }
            true
        } else {
            false
        }
    }

    /// Switch to the next tab
    pub fn next_tab(&mut self) {
        if !self.sessions.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.sessions.len();
            // Clear needs_attention flag when switching to a tab
            if let Some(session) = self.sessions.get_mut(self.active_tab) {
                session.needs_attention = false;
            }
        }
    }

    /// Switch to the previous tab
    pub fn prev_tab(&mut self) {
        if !self.sessions.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.sessions.len() - 1
            } else {
                self.active_tab - 1
            };
            // Clear needs_attention flag when switching to a tab
            if let Some(session) = self.sessions.get_mut(self.active_tab) {
                session.needs_attention = false;
            }
        }
    }

    /// Get the current active tab index
    pub fn active_index(&self) -> usize {
        self.active_tab
    }

    /// Get the number of tabs
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Check if there are no tabs
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get a reference to the active session
    pub fn active_session(&self) -> Option<&AgentSession> {
        self.sessions.get(self.active_tab)
    }

    /// Get a mutable reference to the active session
    pub fn active_session_mut(&mut self) -> Option<&mut AgentSession> {
        self.sessions.get_mut(self.active_tab)
    }

    /// Get a reference to a session by index
    pub fn session(&self, index: usize) -> Option<&AgentSession> {
        self.sessions.get(index)
    }

    /// Get a mutable reference to a session by index
    pub fn session_mut(&mut self, index: usize) -> Option<&mut AgentSession> {
        self.sessions.get_mut(index)
    }

    /// Get all sessions for iteration
    pub fn sessions(&self) -> &[AgentSession] {
        &self.sessions
    }

    /// Get mutable references to all sessions for iteration
    pub fn sessions_mut(&mut self) -> &mut [AgentSession] {
        &mut self.sessions
    }

    /// Get tab names for display
    pub fn tab_names(&self) -> Vec<String> {
        self.sessions.iter().map(|s| s.tab_name()).collect()
    }

    /// Check if we can add more tabs
    pub fn can_add_tab(&self) -> bool {
        self.sessions.len() < self.max_tabs
    }

    /// Add an existing session (used for session restoration)
    pub fn add_session(&mut self, session: AgentSession) -> Option<usize> {
        if self.sessions.len() >= self.max_tabs {
            return None;
        }

        self.sessions.push(session);
        let new_index = self.sessions.len() - 1;
        Some(new_index)
    }
}
