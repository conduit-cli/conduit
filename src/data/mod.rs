//! Data persistence layer for Conduit
//!
//! This module provides SQLite-based storage for repositories and workspaces.

mod app_state;
mod database;
mod models;
mod repository;
mod session_tab;
mod workspace;

pub use app_state::AppStateStore;
pub use database::Database;
pub use models::{
    QueuedImageAttachment, QueuedMessage, QueuedMessageMode, Repository, SessionTab, Workspace,
};
pub use repository::RepositoryStore;
pub use session_tab::SessionTabStore;
pub use workspace::WorkspaceStore;
