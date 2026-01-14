//! Web application state for the Conduit web server.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::core::ConduitCore;

use super::ws::SessionManager;

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
}

impl WebAppState {
    /// Create a new web application state from a ConduitCore.
    pub fn new(core: ConduitCore) -> Self {
        let inner = Arc::new(RwLock::new(core));
        let session_manager = Arc::new(SessionManager::new(inner.clone()));
        Self {
            inner,
            session_manager,
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
}
