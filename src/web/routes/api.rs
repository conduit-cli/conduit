//! REST API route definitions.

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::web::handlers::{
    bootstrap, external_sessions, models, onboarding, queue, repositories, sessions, themes,
    ui_state, workspaces,
};
use crate::web::state::WebAppState;

/// Build the API router with all REST endpoints.
pub fn api_routes() -> Router<WebAppState> {
    Router::new()
        .route("/bootstrap", get(bootstrap::get_bootstrap))
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
        .route(
            "/repositories/{id}/workspaces/auto",
            post(workspaces::auto_create_workspace),
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
        .route(
            "/workspaces/{id}/pr/preflight",
            get(workspaces::get_workspace_pr_preflight),
        )
        .route("/workspaces/{id}/pr", post(workspaces::create_workspace_pr))
        .route(
            "/workspaces/{id}/session",
            post(workspaces::get_or_create_session),
        )
        // Session routes
        .route("/sessions", get(sessions::list_sessions))
        .route("/sessions", post(sessions::create_session))
        .route("/sessions/{id}", get(sessions::get_session))
        .route("/sessions/{id}", patch(sessions::update_session))
        .route("/sessions/{id}", delete(sessions::close_session))
        .route("/sessions/{id}/events", get(sessions::get_session_events))
        .route("/sessions/{id}/history", get(sessions::get_session_history))
        .route("/sessions/{id}/fork", post(sessions::fork_session))
        .route("/sessions/{id}/queue", get(queue::list_queue))
        .route("/sessions/{id}/queue", post(queue::add_queue_message))
        .route(
            "/sessions/{id}/queue/{message_id}",
            patch(queue::update_queue_message),
        )
        .route(
            "/sessions/{id}/queue/{message_id}",
            delete(queue::delete_queue_message),
        )
        // Onboarding routes
        .route("/onboarding/base-dir", get(onboarding::get_base_dir))
        .route("/onboarding/base-dir", post(onboarding::set_base_dir))
        .route("/onboarding/projects", get(onboarding::list_projects))
        .route("/onboarding/add-project", post(onboarding::add_project))
        // External session import
        .route(
            "/external-sessions",
            get(external_sessions::list_external_sessions),
        )
        .route(
            "/external-sessions/{id}/import",
            post(external_sessions::import_external_session),
        )
        // Model routes
        .route("/models", get(models::list_models))
        .route("/models/default", patch(models::set_default_model))
        // Theme routes
        .route("/themes", get(themes::list_available_themes))
        .route("/themes/current", get(themes::get_current_theme))
        .route("/themes/current", post(themes::set_current_theme))
        // UI state routes
        .route("/ui/state", get(ui_state::get_ui_state))
        .route("/ui/state", post(ui_state::update_ui_state))
}
