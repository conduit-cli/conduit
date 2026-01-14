//! Tab abstraction for supporting different tab types
//!
//! This module defines the Tab enum which allows the TabManager to store
//! both agent sessions and file viewers in the same collection.

use uuid::Uuid;

use crate::ui::file_viewer::FileViewerSession;
use crate::ui::session::AgentSession;

/// Represents any type of tab in the application
///
/// Note: The large size difference between variants (AgentSession ~2KB vs FileViewerSession ~112B)
/// is acceptable since tabs are stored in a Vec and accessed by reference.
#[allow(clippy::large_enum_variant)]
pub enum Tab {
    /// An agent chat session tab
    Agent(AgentSession),
    /// A file viewer tab
    File(FileViewerSession),
}

impl Tab {
    /// Get the unique ID for this tab
    pub fn id(&self) -> Uuid {
        match self {
            Tab::Agent(session) => session.id,
            Tab::File(viewer) => viewer.id,
        }
    }

    /// Get display name for the tab bar
    pub fn tab_name(&self) -> String {
        match self {
            Tab::Agent(session) => session.tab_name(),
            Tab::File(viewer) => viewer.tab_name(),
        }
    }

    /// Check if this tab needs attention (unread content)
    pub fn needs_attention(&self) -> bool {
        match self {
            Tab::Agent(session) => session.needs_attention,
            Tab::File(_) => false, // Files don't have notifications
        }
    }

    /// Check if this tab is processing
    pub fn is_processing(&self) -> bool {
        match self {
            Tab::Agent(session) => session.is_processing,
            Tab::File(_) => false,
        }
    }

    /// Check if this tab is awaiting user response
    pub fn is_awaiting_response(&self) -> bool {
        match self {
            Tab::Agent(session) => session.inline_prompt.is_some(),
            Tab::File(_) => false,
        }
    }

    /// Get the agent session if this is an agent tab
    pub fn as_agent(&self) -> Option<&AgentSession> {
        match self {
            Tab::Agent(session) => Some(session),
            Tab::File(_) => None,
        }
    }

    /// Get the agent session mutably if this is an agent tab
    pub fn as_agent_mut(&mut self) -> Option<&mut AgentSession> {
        match self {
            Tab::Agent(session) => Some(session),
            Tab::File(_) => None,
        }
    }

    /// Get the file viewer if this is a file tab
    pub fn as_file(&self) -> Option<&FileViewerSession> {
        match self {
            Tab::Agent(_) => None,
            Tab::File(viewer) => Some(viewer),
        }
    }

    /// Get the file viewer mutably if this is a file tab
    pub fn as_file_mut(&mut self) -> Option<&mut FileViewerSession> {
        match self {
            Tab::Agent(_) => None,
            Tab::File(viewer) => Some(viewer),
        }
    }
}
