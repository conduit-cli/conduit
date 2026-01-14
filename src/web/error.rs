//! Web error types for the Conduit web server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Error type for web API operations.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Bad request with validation error.
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Error response body.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, error_message, details) = match &self {
            WebError::NotFound(msg) => (StatusCode::NOT_FOUND, "Not Found", Some(msg.clone())),
            WebError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, "Bad Request", Some(msg.clone()))
            }
            WebError::Internal(msg) => {
                tracing::error!("Internal server error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    None,
                )
            }
            WebError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database Error", None)
            }
            WebError::Config(msg) => {
                tracing::error!("Configuration error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration Error",
                    Some(msg.clone()),
                )
            }
        };

        let body = Json(ErrorResponse {
            error: error_message.to_string(),
            details,
        });

        (status, body).into_response()
    }
}

impl From<anyhow::Error> for WebError {
    fn from(err: anyhow::Error) -> Self {
        WebError::Internal(err.to_string())
    }
}
