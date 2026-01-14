//! Tab manager for handling multiple tabs of different types
//!
//! This module manages all tabs (agent sessions and file viewers) in the application.

use std::path::PathBuf;

use uuid::Uuid;

use crate::agent::AgentType;
use crate::ui::file_viewer::FileViewerSession;
use crate::ui::session::AgentSession;
use crate::ui::tab::Tab;

/// Manages multiple tabs (agent sessions and file viewers)
pub struct TabManager {
    /// All active tabs
    tabs: Vec<Tab>,
    /// Index of the currently active tab
    active_tab: usize,
    /// Maximum number of tabs allowed
    max_tabs: usize,
}

impl TabManager {
    pub fn new(max_tabs: usize) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            max_tabs,
        }
    }

    /// Create a new agent tab with the given agent type
    pub fn new_tab(&mut self, agent_type: AgentType) -> Option<usize> {
        if self.tabs.len() >= self.max_tabs {
            return None;
        }

        let session = AgentSession::new(agent_type);
        self.tabs.push(Tab::Agent(session));
        let new_index = self.tabs.len() - 1;
        self.active_tab = new_index;
        Some(new_index)
    }

    /// Create a new agent tab with the given agent type and working directory
    pub fn new_tab_with_working_dir(
        &mut self,
        agent_type: AgentType,
        working_dir: PathBuf,
    ) -> Option<usize> {
        if self.tabs.len() >= self.max_tabs {
            return None;
        }

        let session = AgentSession::with_working_dir(agent_type, working_dir);
        self.tabs.push(Tab::Agent(session));
        let new_index = self.tabs.len() - 1;
        self.active_tab = new_index;
        Some(new_index)
    }

    /// Open a file in a new tab
    pub fn open_file(&mut self, path: PathBuf) -> Result<usize, std::io::Error> {
        if self.tabs.len() >= self.max_tabs {
            return Err(std::io::Error::other("Maximum tabs reached"));
        }

        let viewer = FileViewerSession::new(path)?;
        self.tabs.push(Tab::File(viewer));
        let new_index = self.tabs.len() - 1;
        self.active_tab = new_index;
        Ok(new_index)
    }

    /// Close a tab by index
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        self.tabs.remove(index);

        // Adjust active tab if needed
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        } else if self.active_tab > index {
            self.active_tab = self.active_tab.saturating_sub(1);
        }

        true
    }

    /// Switch to a specific tab
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab = index;
            // Clear needs_attention flag when switching to an agent tab
            if let Some(Tab::Agent(session)) = self.tabs.get_mut(index) {
                session.needs_attention = false;
            }
            true
        } else {
            false
        }
    }

    /// Switch to the next tab
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
            // Clear needs_attention flag when switching to an agent tab
            if let Some(Tab::Agent(session)) = self.tabs.get_mut(self.active_tab) {
                session.needs_attention = false;
            }
        }
    }

    /// Switch to the previous tab
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
            // Clear needs_attention flag when switching to an agent tab
            if let Some(Tab::Agent(session)) = self.tabs.get_mut(self.active_tab) {
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
        self.tabs.len()
    }

    /// Check if there are no tabs
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get a reference to the active tab
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    /// Get a mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_tab)
    }

    /// Get a reference to the active session (for backward compatibility)
    /// Returns None if active tab is not an agent session
    pub fn active_session(&self) -> Option<&AgentSession> {
        match self.tabs.get(self.active_tab) {
            Some(Tab::Agent(session)) => Some(session),
            _ => None,
        }
    }

    /// Get a mutable reference to the active session
    /// Returns None if active tab is not an agent session
    pub fn active_session_mut(&mut self) -> Option<&mut AgentSession> {
        match self.tabs.get_mut(self.active_tab) {
            Some(Tab::Agent(session)) => Some(session),
            _ => None,
        }
    }

    /// Get a reference to the active file viewer
    /// Returns None if active tab is not a file viewer
    pub fn active_file_viewer(&self) -> Option<&FileViewerSession> {
        match self.tabs.get(self.active_tab) {
            Some(Tab::File(viewer)) => Some(viewer),
            _ => None,
        }
    }

    /// Get a mutable reference to the active file viewer
    /// Returns None if active tab is not a file viewer
    pub fn active_file_viewer_mut(&mut self) -> Option<&mut FileViewerSession> {
        match self.tabs.get_mut(self.active_tab) {
            Some(Tab::File(viewer)) => Some(viewer),
            _ => None,
        }
    }

    /// Get a reference to an agent session by index
    pub fn session(&self, index: usize) -> Option<&AgentSession> {
        match self.tabs.get(index) {
            Some(Tab::Agent(session)) => Some(session),
            _ => None,
        }
    }

    /// Get a mutable reference to an agent session by index
    pub fn session_mut(&mut self, index: usize) -> Option<&mut AgentSession> {
        match self.tabs.get_mut(index) {
            Some(Tab::Agent(session)) => Some(session),
            _ => None,
        }
    }

    /// Get a reference to a tab by index
    pub fn tab(&self, index: usize) -> Option<&Tab> {
        self.tabs.get(index)
    }

    /// Get a mutable reference to a tab by index
    pub fn tab_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// Get all agent sessions as a Vec (for backward compatibility)
    pub fn sessions(&self) -> Vec<&AgentSession> {
        self.tabs
            .iter()
            .filter_map(|t| match t {
                Tab::Agent(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// Iterate over all agent sessions mutably
    pub fn sessions_mut(&mut self) -> impl Iterator<Item = &mut AgentSession> {
        self.tabs.iter_mut().filter_map(|t| match t {
            Tab::Agent(s) => Some(s),
            _ => None,
        })
    }

    /// Get all tabs for iteration
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get tab names for display
    pub fn tab_names(&self) -> Vec<String> {
        self.tabs.iter().map(|t| t.tab_name()).collect()
    }

    /// Check if we can add more tabs
    pub fn can_add_tab(&self) -> bool {
        self.tabs.len() < self.max_tabs
    }

    /// Find a tab index by its UUID
    pub fn tab_index_by_id(&self, id: Uuid) -> Option<usize> {
        self.tabs.iter().position(|t| t.id() == id)
    }

    /// Find a session index by its UUID (for backward compatibility)
    pub fn session_index_by_id(&self, id: Uuid) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| matches!(t, Tab::Agent(s) if s.id == id))
    }

    /// Add an existing session (used for session restoration)
    pub fn add_session(&mut self, session: AgentSession) -> Option<usize> {
        if self.tabs.len() >= self.max_tabs {
            return None;
        }

        self.tabs.push(Tab::Agent(session));
        let new_index = self.tabs.len() - 1;
        Some(new_index)
    }

    /// Find a session by its UUID and return a mutable reference
    pub fn session_by_id_mut(&mut self, id: Uuid) -> Option<&mut AgentSession> {
        self.tabs.iter_mut().find_map(|t| match t {
            Tab::Agent(s) if s.id == id => Some(s),
            _ => None,
        })
    }

    /// Check if the active tab is a file viewer
    pub fn active_is_file(&self) -> bool {
        matches!(self.tabs.get(self.active_tab), Some(Tab::File(_)))
    }

    /// Check if the active tab is an agent session
    pub fn active_is_agent(&self) -> bool {
        matches!(self.tabs.get(self.active_tab), Some(Tab::Agent(_)))
    }
}
