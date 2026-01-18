//! Web application state for the Conduit web server.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::core::ConduitCore;

use super::ws::SessionManager;
use super::{StatusManager, StatusManagerConfig};

/// Shared state for the web application.
///
/// This wraps `ConduitCore` with thread-safe access patterns suitable
/// for use with Axum's async handlers.
#[derive(Clone)]
pub struct WebAppState {
    /// The shared Conduit core containing all business logic.
    inner: Arc<RwLock<ConduitCore>>,
    /// Session manager for WebSocket agent sessions.
    session_manager: Arc<SessionManager>,
    /// Background workspace status manager.
    status_manager: Arc<StatusManager>,
}

impl WebAppState {
    /// Create a new web application state from a ConduitCore.
    pub fn new(core: ConduitCore) -> Self {
        let status_config = StatusManagerConfig::from_config(core.config());
        let inner = Arc::new(RwLock::new(core));
        let session_manager = Arc::new(SessionManager::new(inner.clone()));
        let status_manager = Arc::new(StatusManager::new(status_config));
        Self {
            inner,
            session_manager,
            status_manager,
        }
    }

    /// Get read access to the core.
    pub async fn core(&self) -> tokio::sync::RwLockReadGuard<'_, ConduitCore> {
        self.inner.read().await
    }

    /// Get write access to the core.
    pub async fn core_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, ConduitCore> {
        self.inner.write().await
    }

    /// Get the session manager for WebSocket connections.
    pub fn session_manager(&self) -> &Arc<SessionManager> {
        &self.session_manager
    }

    /// Get the workspace status manager.
    pub fn status_manager(&self) -> &Arc<StatusManager> {
        &self.status_manager
    }

    /// Kick the initial status scan for all workspaces.
    pub async fn start_status_manager(&self) {
        let core = self.core().await;
        let store = match core.workspace_store() {
            Some(store) => store,
            None => {
                tracing::warn!("Workspace store unavailable; skipping initial status scan");
                return;
            }
        };

        let workspaces = match store.get_all() {
            Ok(workspaces) => workspaces,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to list workspaces for initial status scan");
                return;
            }
        };

        self.status_manager.kick_initial_scan(workspaces);
    }
}
