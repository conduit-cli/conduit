//! Session queue handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::services::{ServiceError, SessionService};
use crate::data::{QueuedImageAttachment, QueuedMessage, QueuedMessageMode};
use crate::web::error::WebError;
use crate::web::state::WebAppState;

#[derive(Debug, Serialize)]
pub struct QueuedImageAttachmentResponse {
    pub path: String,
    pub placeholder: String,
}

impl From<&QueuedImageAttachment> for QueuedImageAttachmentResponse {
    fn from(image: &QueuedImageAttachment) -> Self {
        Self {
            path: image.path.to_string_lossy().to_string(),
            placeholder: image.placeholder.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QueuedMessageResponse {
    pub id: Uuid,
    pub mode: QueuedMessageMode,
    pub text: String,
    pub images: Vec<QueuedImageAttachmentResponse>,
    pub created_at: String,
}

impl From<QueuedMessage> for QueuedMessageResponse {
    fn from(message: QueuedMessage) -> Self {
        Self {
            id: message.id,
            mode: message.mode,
            text: message.text,
            images: message
                .images
                .iter()
                .map(QueuedImageAttachmentResponse::from)
                .collect(),
            created_at: message.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub messages: Vec<QueuedMessageResponse>,
}

#[derive(Debug, Deserialize)]
pub struct AddQueueRequest {
    pub mode: QueuedMessageMode,
    pub text: String,
    #[serde(default)]
    pub images: Vec<QueuedImageAttachment>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateQueueRequest {
    pub text: Option<String>,
    pub mode: Option<QueuedMessageMode>,
    pub position: Option<usize>,
}

pub async fn list_queue(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<QueueResponse>, WebError> {
    let core = state.core().await;
    let messages = SessionService::list_queue(&core, id).map_err(map_service_error)?;
    let response = QueueResponse {
        messages: messages
            .into_iter()
            .map(QueuedMessageResponse::from)
            .collect(),
    };
    Ok(Json(response))
}

pub async fn add_queue_message(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<AddQueueRequest>,
) -> Result<(StatusCode, Json<QueuedMessageResponse>), WebError> {
    let core = state.core().await;
    let message = SessionService::add_queue_message(&core, id, req.mode, req.text, req.images)
        .map_err(map_service_error)?;

    if let Err(err) = SessionService::append_input_history(&core, id, &message.text) {
        tracing::warn!(error = %err, %id, "Failed to update input history for queued message");
    }

    Ok((
        StatusCode::CREATED,
        Json(QueuedMessageResponse::from(message)),
    ))
}

pub async fn update_queue_message(
    State(state): State<WebAppState>,
    Path((id, message_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<Json<QueuedMessageResponse>, WebError> {
    let core = state.core().await;
    let updated = SessionService::update_queue_message(
        &core,
        id,
        message_id,
        req.text,
        req.mode,
        req.position,
    )
    .map_err(map_service_error)?;

    Ok(Json(QueuedMessageResponse::from(updated)))
}

pub async fn delete_queue_message(
    State(state): State<WebAppState>,
    Path((id, message_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    SessionService::remove_queue_message(&core, id, message_id).map_err(map_service_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}
