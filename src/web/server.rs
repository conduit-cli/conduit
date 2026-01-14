//! Axum web server implementation for Conduit.

use std::net::SocketAddr;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{header, Method},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::routes::api::api_routes;
use super::routes::static_files::{serve_index, serve_static_file};
use super::state::WebAppState;
use super::ws::handle_websocket;

/// Server configuration options.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Host address to bind to.
    pub host: String,
    /// Port to listen on.
    pub port: u16,
    /// Enable CORS for development (allows any origin).
    pub cors_permissive: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            cors_permissive: true,
        }
    }
}

/// Health check response.
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

/// Health check endpoint handler.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Agent types response.
#[derive(Serialize)]
struct AgentsResponse {
    agents: Vec<AgentInfo>,
}

#[derive(Serialize)]
struct AgentInfo {
    id: &'static str,
    name: &'static str,
    available: bool,
}

/// List available agents.
async fn list_agents(State(state): State<WebAppState>) -> Json<AgentsResponse> {
    use crate::util::Tool;

    let core = state.core().await;
    let tools = core.tools();

    Json(AgentsResponse {
        agents: vec![
            AgentInfo {
                id: "claude",
                name: "Claude Code",
                available: tools.is_available(Tool::Claude),
            },
            AgentInfo {
                id: "codex",
                name: "Codex CLI",
                available: tools.is_available(Tool::Codex),
            },
            AgentInfo {
                id: "gemini",
                name: "Gemini CLI",
                available: tools.is_available(Tool::Gemini),
            },
        ],
    })
}

/// WebSocket upgrade handler.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<WebAppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        handle_websocket(socket, state.session_manager().clone()).await
    })
}

/// Build the Axum router with all routes.
fn build_router(state: WebAppState, cors_permissive: bool) -> Router {
    // Build CORS layer
    let cors = if cors_permissive {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
    } else {
        CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
    };

    // Core API routes (health, agents)
    let core_routes = Router::new()
        .route("/health", get(health))
        .route("/agents", get(list_agents));

    // Build main router combining core routes, REST API routes, and static files
    Router::new()
        .nest("/api", core_routes.merge(api_routes()))
        .route("/ws", get(ws_handler))
        // Static file routes for frontend assets
        .route("/assets/{*path}", get(serve_static_file))
        .route("/", get(serve_index))
        // Fallback to index.html for SPA routing
        .fallback(serve_index)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Run the web server.
///
/// This starts the Axum server and blocks until shutdown.
pub async fn run_server(state: WebAppState, config: ServerConfig) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let app = build_router(state, config.cors_permissive);

    tracing::info!("Starting web server at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::ConduitCore;
    use crate::util::ToolAvailability;
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> WebAppState {
        let config = Config::default();
        let tools = ToolAvailability::default();
        let core = ConduitCore::new(config, tools);
        WebAppState::new(core)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_agents_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_repositories_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/repositories")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify response body structure
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("repositories").is_some());
    }

    #[tokio::test]
    async fn test_list_workspaces_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/workspaces")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify response body structure
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("workspaces").is_some());
    }

    #[tokio::test]
    async fn test_list_sessions_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify response body structure
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("sessions").is_some());
    }

    #[tokio::test]
    async fn test_create_repository_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let body = serde_json::json!({
            "name": "test-repo",
            "base_path": "/tmp/test-repo"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/repositories")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // Verify response body structure
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("test-repo"));
        assert!(json.get("id").is_some());
    }

    #[tokio::test]
    async fn test_create_repository_validation() {
        let state = test_state();
        let app = build_router(state, true);

        // Missing both base_path and repository_url
        let body = serde_json::json!({
            "name": "test-repo"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/repositories")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_repository_not_found() {
        let state = test_state();
        let app = build_router(state, true);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/repositories/00000000-0000-0000-0000-000000000000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_session_endpoint() {
        let state = test_state();
        let app = build_router(state, true);

        let body = serde_json::json!({
            "agent_type": "claude",
            "model": "claude-sonnet-4-20250514"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // Verify response body structure
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json.get("agent_type").and_then(|v| v.as_str()),
            Some("claude")
        );
        assert!(json.get("id").is_some());
    }

    #[tokio::test]
    async fn test_create_session_invalid_agent_type() {
        let state = test_state();
        let app = build_router(state, true);

        let body = serde_json::json!({
            "agent_type": "invalid"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
