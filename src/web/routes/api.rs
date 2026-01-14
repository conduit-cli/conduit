//! REST API route definitions.

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::web::handlers::{repositories, sessions, themes, workspaces};
use crate::web::state::WebAppState;

/// Build the API router with all REST endpoints.
pub fn api_routes() -> Router<WebAppState> {
    Router::new()
        // Repository routes
        .route("/repositories", get(repositories::list_repositories))
        .route("/repositories", post(repositories::create_repository))
        .route("/repositories/{id}", get(repositories::get_repository))
        .route(
            "/repositories/{id}",
            delete(repositories::delete_repository),
        )
        // Repository workspaces routes
        .route(
            "/repositories/{id}/workspaces",
            get(workspaces::list_repository_workspaces),
        )
        .route(
            "/repositories/{id}/workspaces",
            post(workspaces::create_workspace),
        )
        // Workspace routes
        .route("/workspaces", get(workspaces::list_workspaces))
        .route("/workspaces/{id}", get(workspaces::get_workspace))
        .route("/workspaces/{id}", delete(workspaces::delete_workspace))
        .route(
            "/workspaces/{id}/archive",
            post(workspaces::archive_workspace),
        )
        .route(
            "/workspaces/{id}/status",
            get(workspaces::get_workspace_status),
        )
        // Session routes
        .route("/sessions", get(sessions::list_sessions))
        .route("/sessions", post(sessions::create_session))
        .route("/sessions/{id}", get(sessions::get_session))
        .route("/sessions/{id}", delete(sessions::close_session))
        .route("/sessions/{id}/events", get(sessions::get_session_events))
        // Theme routes
        .route("/themes", get(themes::list_available_themes))
        .route("/themes/current", get(themes::get_current_theme))
        .route("/themes/current", post(themes::set_current_theme))
}
