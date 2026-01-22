//! WebSocket connection handler for real-time agent communication.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use base64::engine::general_purpose;
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

use crate::agent::events::AgentEvent;
use crate::agent::runner::{AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::core::services::SessionService;
use crate::core::ConduitCore;
use crate::ui::app_prompt;
use crate::util::{generate_title_and_branch, get_git_username, sanitize_branch_suffix};
use serde_json::json;

use super::messages::{ClientMessage, ImageAttachment, ServerMessage};

/// Active session state tracked by the WebSocket handler.
struct ActiveSession {
    agent_type: AgentType,
    /// Process ID for stopping the agent
    pid: Option<u32>,
    /// Sender to broadcast events to all subscribers
    event_tx: broadcast::Sender<AgentEvent>,
    /// Input sender for sending follow-up messages
    input_tx: Option<mpsc::Sender<AgentInput>>,
}

/// Manages active agent sessions and their event streams.
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<Uuid, ActiveSession>>>,
    core: Arc<RwLock<ConduitCore>>,
}

struct StartSessionArgs {
    session_id: Uuid,
    agent_type: AgentType,
    prompt: String,
    working_dir: PathBuf,
    model: Option<String>,
    images: Vec<PathBuf>,
    input_format: Option<String>,
    stdin_payload: Option<String>,
}

struct TitleGenerationOutcome {
    title: String,
    workspace_id: Option<Uuid>,
    new_branch: Option<String>,
}

async fn persist_agent_session_id(
    core: &Arc<RwLock<ConduitCore>>,
    session_id: Uuid,
    agent_session_id: &str,
) -> Result<(), String> {
    let store = {
        let core = core.read().await;
        core.session_tab_store_clone()
            .ok_or_else(|| "Database not available".to_string())?
    };

    let mut tab = store
        .get_by_id(session_id)
        .map_err(|e| format!("Failed to get session {}: {}", session_id, e))?
        .ok_or_else(|| format!("Session {} not found in database", session_id))?;

    if tab.agent_session_id.as_deref() == Some(agent_session_id) {
        return Ok(());
    }

    tab.agent_session_id = Some(agent_session_id.to_string());
    store
        .update(&tab)
        .map_err(|e| format!("Failed to update session {}: {}", session_id, e))?;

    Ok(())
}

async fn append_input_history(
    core: &Arc<RwLock<ConduitCore>>,
    session_id: Uuid,
    input: &str,
) -> Result<(), String> {
    let core = core.read().await;
    SessionService::append_input_history(&core, session_id, input)
        .map_err(|e| format!("Failed to append input history: {}", e))?;
    Ok(())
}

impl SessionManager {
    pub fn new(core: Arc<RwLock<ConduitCore>>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            core,
        }
    }

    /// Start a new agent session.
    async fn start_session(
        &self,
        args: StartSessionArgs,
    ) -> Result<broadcast::Receiver<AgentEvent>, String> {
        let StartSessionArgs {
            session_id,
            agent_type,
            prompt,
            working_dir,
            model,
            images,
            input_format,
            stdin_payload,
        } = args;

        // Check if session already exists
        {
            let sessions = self.sessions.read().await;
            if sessions
                .get(&session_id)
                .is_some_and(|existing| existing.pid.is_some())
            {
                return Err(format!("Session {} is already running", session_id));
            }
        }

        // Get the appropriate runner
        let core = self.core.read().await;
        let runner: Arc<dyn AgentRunner> = match agent_type {
            AgentType::Claude => core.claude_runner().clone(),
            AgentType::Codex => core.codex_runner().clone(),
            AgentType::Gemini => core.gemini_runner().clone(),
        };

        if !runner.is_available() {
            return Err(format!("{} is not available", agent_type.display_name()));
        }

        // Build start config
        let mut config = AgentStartConfig::new(prompt, working_dir);
        if let Some(m) = model {
            config = config.with_model(m);
        }
        if !images.is_empty() {
            config = config.with_images(images);
        }
        if let Some(format) = input_format {
            config = config.with_input_format(format);
        }
        if let Some(payload) = stdin_payload {
            config = config.with_stdin_payload(payload);
        }

        // Start the agent
        let mut handle = runner
            .start(config)
            .await
            .map_err(|e| format!("Failed to start agent: {}", e))?;

        let pid = handle.pid;
        let input_tx = handle.input_tx.take();

        // Reuse an existing event channel if we already have one (e.g. if the UI subscribed
        // before the session started). This prevents "Session <id> not found" errors when
        // selecting non-running session tabs.
        let (event_tx, event_rx) = {
            let mut sessions = self.sessions.write().await;
            if let Some(existing) = sessions.get_mut(&session_id) {
                // Another start could have raced us.
                if existing.pid.is_some() {
                    return Err(format!("Session {} is already running", session_id));
                }
                existing.agent_type = agent_type;
                existing.pid = Some(pid);
                existing.input_tx = input_tx;
                (existing.event_tx.clone(), existing.event_tx.subscribe())
            } else {
                let (event_tx, event_rx) = broadcast::channel(256);
                sessions.insert(
                    session_id,
                    ActiveSession {
                        agent_type,
                        pid: Some(pid),
                        event_tx: event_tx.clone(),
                        input_tx,
                    },
                );
                (event_tx, event_rx)
            }
        };

        // Spawn task to forward events from agent to broadcast channel
        let sessions_ref = self.sessions.clone();
        let core_ref = self.core.clone();
        tokio::spawn(async move {
            while let Some(event) = handle.events.recv().await {
                if let AgentEvent::SessionInit(init) = &event {
                    if let Err(error) =
                        persist_agent_session_id(&core_ref, session_id, init.session_id.as_str())
                            .await
                    {
                        tracing::warn!(
                            %session_id,
                            agent_session_id = %init.session_id,
                            error = %error,
                            "Failed to persist agent session id"
                        );
                    }
                }

                if let Err(error) = event_tx.send(event) {
                    tracing::debug!(
                        %session_id,
                        error = %error,
                        "No active subscribers for agent events"
                    );
                }
            }
            // Session ended, remove from map
            let mut sessions = sessions_ref.write().await;
            sessions.remove(&session_id);
        });

        Ok(event_rx)
    }

    /// Subscribe to events for an existing session.
    pub async fn subscribe(
        &self,
        session_id: Uuid,
    ) -> Result<broadcast::Receiver<AgentEvent>, String> {
        // If the session is running (or already has a channel), subscribe immediately.
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&session_id) {
                return Ok(session.event_tx.subscribe());
            }
        }

        // Otherwise, validate the session exists in the DB and create an idle channel so the UI
        // can safely subscribe without showing an error.
        let store = {
            let core = self.core.read().await;
            core.session_tab_store_clone()
                .ok_or_else(|| "Database not available".to_string())?
        };

        let tab = store
            .get_by_id(session_id)
            .map_err(|e| format!("Failed to get session {}: {}", session_id, e))?
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        let (event_tx, event_rx) = broadcast::channel(256);
        let mut sessions = self.sessions.write().await;
        // Another subscribe/start could have raced us.
        if let Some(existing) = sessions.get(&session_id) {
            return Ok(existing.event_tx.subscribe());
        }

        sessions.insert(
            session_id,
            ActiveSession {
                agent_type: tab.agent_type,
                pid: None,
                event_tx,
                input_tx: None,
            },
        );

        Ok(event_rx)
    }

    /// Stop a running session.
    pub async fn stop_session(&self, session_id: Uuid) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(&session_id) {
            // Kill the process by PID
            #[cfg(unix)]
            {
                use std::process::Command;
                if let Some(pid) = session.pid {
                    match Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .status()
                    {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            tracing::warn!(
                                pid,
                                exit_status = ?status.code(),
                                "Failed to terminate session process with kill"
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                pid,
                                "Failed to execute kill for session process"
                            );
                        }
                    }
                }
            }
            #[cfg(windows)]
            {
                use std::process::Command;
                if let Some(pid) = session.pid {
                    match Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/F"])
                        .status()
                    {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            tracing::warn!(
                                pid,
                                exit_status = ?status.code(),
                                "Failed to terminate session process with taskkill"
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                pid,
                                "Failed to execute taskkill for session process"
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Send input to a running session.
    pub async fn send_input(
        &self,
        session_id: Uuid,
        input: String,
        images: Vec<PathBuf>,
    ) -> Result<(), String> {
        let (input_tx, agent_type) = {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            let input_tx = session
                .input_tx
                .clone()
                .ok_or_else(|| "Session does not support input".to_string())?;
            (input_tx, session.agent_type)
        };

        // Send as appropriate input type based on agent
        let agent_input = match agent_type {
            AgentType::Claude => AgentInput::ClaudeJsonl(input),
            AgentType::Codex | AgentType::Gemini => AgentInput::CodexPrompt {
                text: input,
                images,
            },
        };

        input_tx
            .send(agent_input)
            .await
            .map_err(|e| format!("Failed to send input: {}", e))?;

        Ok(())
    }

    /// Respond to a control request for a running session.
    pub async fn respond_to_control(
        &self,
        session_id: Uuid,
        request_id: String,
        response: serde_json::Value,
    ) -> Result<(), String> {
        let input_tx = {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            let input_tx = session
                .input_tx
                .clone()
                .ok_or_else(|| "Session does not support control responses".to_string())?;
            if session.agent_type != AgentType::Claude {
                return Err("Control responses are only supported for Claude sessions".to_string());
            }
            input_tx
        };

        let payload = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": response,
            }
        });
        let json_payload = serde_json::to_string(&payload)
            .map_err(|e| format!("Failed to serialize control response: {}", e))?;

        input_tx
            .send(AgentInput::ClaudeJsonl(format!("{json_payload}\n")))
            .await
            .map_err(|e| format!("Failed to send control response: {}", e))?;

        Ok(())
    }

    /// Get the agent type for a session.
    pub async fn get_agent_type(&self, session_id: Uuid) -> Option<AgentType> {
        let sessions = self.sessions.read().await;
        sessions.get(&session_id).map(|s| s.agent_type)
    }
}

fn should_generate_title(hidden: bool, session: &crate::data::SessionTab) -> bool {
    !hidden && session.title.is_none() && session.agent_session_id.is_none()
}

async fn generate_title_and_branch_for_session(
    core: Arc<RwLock<ConduitCore>>,
    session_id: Uuid,
    user_message: String,
    working_dir: PathBuf,
) -> Result<Option<TitleGenerationOutcome>, String> {
    let (tools, worktree_manager, session_store, workspace_store) = {
        let core = core.read().await;
        (
            core.tools().clone(),
            core.worktree_manager().clone(),
            core.session_tab_store_clone(),
            core.workspace_store_clone(),
        )
    };

    let session_store =
        session_store.ok_or_else(|| "Database not available for sessions".to_string())?;

    let mut session = session_store
        .get_by_id(session_id)
        .map_err(|e| format!("Failed to load session {}: {}", session_id, e))?
        .ok_or_else(|| format!("Session {} not found for title generation", session_id))?;

    if session.title.is_some() || session.agent_session_id.is_some() {
        return Ok(None);
    }

    let metadata = generate_title_and_branch(&tools, &user_message, &working_dir)
        .await
        .map_err(|e| e.to_string())?;

    let mut new_branch: Option<String> = None;
    let workspace_id = session.workspace_id;

    if workspace_id.is_some() {
        let resolved_branch = {
            let wd = working_dir.clone();
            let wm = worktree_manager.clone();
            let wd_for_log = wd.clone();
            let fresh_branch = match tokio::task::spawn_blocking(move || {
                wm.get_current_branch(&wd).map_err(|e| e.to_string())
            })
            .await
            {
                Ok(Ok(branch)) => branch,
                Ok(Err(err)) => {
                    tracing::warn!(
                        error = %err,
                        working_dir = %wd_for_log.display(),
                        "Failed to fetch current branch from worktree"
                    );
                    String::new()
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "spawn_blocking failed while fetching current branch"
                    );
                    String::new()
                }
            };
            fresh_branch
        };

        if resolved_branch.is_empty() {
            tracing::debug!("Skipping branch rename: could not determine current branch");
        } else {
            let raw_username = get_git_username();
            let username = sanitize_branch_suffix(&raw_username);
            let suffix = sanitize_branch_suffix(&metadata.branch_suffix);

            if suffix == "task" {
                tracing::debug!(
                    suffix = %suffix,
                    "Skipping branch rename: sanitized suffix is generic fallback"
                );
            } else {
                let new_branch_name = if username == "task" {
                    tracing::debug!(
                        raw_username = %raw_username,
                        sanitized = %username,
                        "Username unusable; generating branch without username prefix"
                    );
                    suffix.clone()
                } else {
                    format!("{}/{}", username, suffix)
                };

                if new_branch_name != resolved_branch {
                    let wd = working_dir.clone();
                    let old = resolved_branch.clone();
                    let new_name = new_branch_name.clone();
                    let wm = worktree_manager.clone();

                    let rename_join_result = tokio::task::spawn_blocking(move || {
                        wm.rename_branch(&wd, &old, &new_name)
                            .map_err(|e| e.to_string())
                    })
                    .await;

                    match rename_join_result {
                        Ok(Ok(())) => {
                            if let (Some(ws_id), Some(ref dao)) = (workspace_id, &workspace_store) {
                                match dao.get_by_id(ws_id) {
                                    Ok(Some(mut ws)) => {
                                        ws.branch = new_branch_name.clone();
                                        if let Err(err) = dao.update(&ws) {
                                            tracing::warn!(
                                                error = %err,
                                                workspace_id = %ws_id,
                                                "Failed to persist branch rename to database"
                                            );
                                        }
                                    }
                                    Ok(None) => {
                                        tracing::warn!(
                                            workspace_id = %ws_id,
                                            "Workspace not found for branch update"
                                        );
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            error = %err,
                                            workspace_id = %ws_id,
                                            "Failed to load workspace for branch update"
                                        );
                                    }
                                }
                            }
                            new_branch = Some(new_branch_name);
                        }
                        Ok(Err(err)) => {
                            tracing::warn!(
                                error = %err,
                                old_branch = %resolved_branch,
                                new_branch = %new_branch_name,
                                "Failed to rename git branch"
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                old_branch = %resolved_branch,
                                new_branch = %new_branch_name,
                                "spawn_blocking join failed during branch rename"
                            );
                        }
                    }
                }
            }
        }
    }

    let sanitized_title = app_prompt::sanitize_title(&metadata.title);
    session.title = Some(sanitized_title.clone());
    if let Err(err) = session_store.update(&session) {
        tracing::warn!(
            error = %err,
            %session_id,
            "Failed to persist session title"
        );
    }

    Ok(Some(TitleGenerationOutcome {
        title: sanitized_title,
        workspace_id,
        new_branch,
    }))
}

fn decode_image_attachments(images: &[ImageAttachment]) -> Result<Vec<PathBuf>, String> {
    if images.is_empty() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::with_capacity(images.len());
    for image in images {
        paths.push(decode_image_attachment(image)?);
    }
    Ok(paths)
}

fn build_claude_prompt_jsonl(prompt: &str, images: &[ImageAttachment]) -> Result<String, String> {
    const SUPPORTED_MEDIA_TYPES: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/jpg",
        "image/webp",
        "image/gif",
    ];
    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

    for (index, image) in images.iter().enumerate() {
        let (bytes, media_type) = decode_base64_image(&image.data, &image.media_type)?;
        if !SUPPORTED_MEDIA_TYPES.contains(&media_type.as_str()) {
            tracing::warn!(
                media_type = %media_type,
                "Unsupported Claude image media type"
            );
            return Err(format!(
                "Unsupported image media type for Claude: {} (supported: {})",
                media_type,
                SUPPORTED_MEDIA_TYPES.join(", ")
            ));
        }
        let base64_data = general_purpose::STANDARD.encode(bytes);
        if images.len() > 1 {
            content_blocks.push(serde_json::json!({
                "type": "text",
                "text": format!("Image {}:", index + 1),
            }));
        }
        content_blocks.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": base64_data,
            }
        }));
    }

    if !prompt.is_empty() {
        content_blocks.push(serde_json::json!({
            "type": "text",
            "text": prompt,
        }));
    }

    let payload = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content_blocks,
        }
    });
    let json = serde_json::to_string(&payload)
        .map_err(|e| format!("Failed to serialize Claude JSONL payload: {}", e))?;
    Ok(format!("{json}\n"))
}

fn decode_image_attachment(image: &ImageAttachment) -> Result<PathBuf, String> {
    let (bytes, media_type) = decode_base64_image(&image.data, &image.media_type)?;
    let ext = match media_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/jpg" => "jpg",
        "image/webp" => "webp",
        _ => return Err(format!("Unsupported image media type: {}", media_type)),
    };

    let dir = uploads_dir()?;
    let filename = format!("ws-image-{}.{}", Uuid::new_v4(), ext);
    let path = dir.join(filename);
    fs::write(&path, bytes).map_err(|e| format!("Failed to write image: {}", e))?;
    Ok(path)
}

fn decode_base64_image(data: &str, fallback_media_type: &str) -> Result<(Vec<u8>, String), String> {
    if let Some(rest) = data.strip_prefix("data:") {
        let mut parts = rest.splitn(2, ";base64,");
        let media_type = parts.next().unwrap_or(fallback_media_type).to_string();
        let encoded = parts
            .next()
            .ok_or_else(|| "Invalid data URL image".to_string())?;
        let bytes = general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| format!("Failed to decode base64 image: {}", e))?;
        return Ok((bytes, media_type));
    }

    let bytes = general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("Failed to decode base64 image: {}", e))?;
    Ok((bytes, fallback_media_type.to_string()))
}

fn uploads_dir() -> Result<PathBuf, String> {
    let dir = crate::util::data_dir().join("uploads");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create uploads dir: {}", e))?;
    Ok(dir)
}

/// Handle a WebSocket connection.
pub async fn handle_websocket(socket: WebSocket, session_manager: Arc<SessionManager>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Channel for sending messages to the WebSocket
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(256);

    // Spawn task to forward messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    tracing::error!("Failed to serialize message: {}", e);
                    continue;
                }
            };
            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Track subscriptions for this connection
    let subscriptions: Arc<RwLock<HashMap<Uuid, tokio::task::JoinHandle<()>>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Handle incoming messages
    'ws_loop: while let Some(result) = ws_receiver.next().await {
        let msg = match result {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) => {
                // Pings are handled automatically by axum
                continue;
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
        };

        let client_msg: ClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                if let Err(send_err) = tx
                    .send(ServerMessage::error(format!("Invalid message: {}", e)))
                    .await
                {
                    tracing::debug!(
                        error = ?send_err,
                        "Failed to send invalid message error"
                    );
                    break 'ws_loop;
                }
                continue;
            }
        };

        match client_msg {
            ClientMessage::Ping => {
                if let Err(send_err) = tx.send(ServerMessage::Pong).await {
                    tracing::debug!(error = ?send_err, "Failed to send pong");
                    break 'ws_loop;
                }
            }

            ClientMessage::Subscribe { session_id } => {
                match session_manager.subscribe(session_id).await {
                    Ok(mut event_rx) => {
                        let tx_clone = tx.clone();
                        let task = tokio::spawn(async move {
                            while let Ok(event) = event_rx.recv().await {
                                if tx_clone
                                    .send(ServerMessage::agent_event(session_id, event))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });

                        let mut subs = subscriptions.write().await;
                        if let Some(existing) = subs.insert(session_id, task) {
                            existing.abort();
                        }

                        if let Err(send_err) =
                            tx.send(ServerMessage::Subscribed { session_id }).await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send subscribed message"
                            );
                            break 'ws_loop;
                        }
                    }
                    Err(e) => {
                        if let Err(send_err) =
                            tx.send(ServerMessage::session_error(session_id, e)).await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send session error"
                            );
                            break 'ws_loop;
                        }
                    }
                }
            }

            ClientMessage::Unsubscribe { session_id } => {
                let mut subs = subscriptions.write().await;
                if let Some(task) = subs.remove(&session_id) {
                    task.abort();
                }
                if let Err(send_err) = tx.send(ServerMessage::Unsubscribed { session_id }).await {
                    tracing::debug!(
                        %session_id,
                        error = ?send_err,
                        "Failed to send unsubscribed message"
                    );
                    break 'ws_loop;
                }
            }

            ClientMessage::StartSession {
                session_id,
                prompt,
                working_dir,
                model,
                hidden,
                images,
            } => {
                // Look up session in database to get agent type
                let core = session_manager.core.read().await;
                let session_tab = if let Some(store) = core.session_tab_store() {
                    match store.get_by_id(session_id) {
                        Ok(Some(tab)) => tab,
                        Ok(None) => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    "Session not found in database",
                                ))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                        Err(e) => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    format!("Database error: {}", e),
                                ))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                    }
                } else {
                    if let Err(send_err) = tx
                        .send(ServerMessage::session_error(
                            session_id,
                            "Database not available",
                        ))
                        .await
                    {
                        tracing::debug!(
                            %session_id,
                            error = ?send_err,
                            "Failed to send session error"
                        );
                        break 'ws_loop;
                    }
                    continue;
                };
                let agent_type = session_tab.agent_type;
                let should_generate = should_generate_title(hidden, &session_tab);
                drop(core);

                let mut input_format: Option<String> = None;
                let mut stdin_payload: Option<String> = None;
                let working_dir_path = PathBuf::from(working_dir);
                let prompt_for_agent = if agent_type == AgentType::Claude {
                    String::new()
                } else {
                    prompt.clone()
                };

                let image_paths = if images.is_empty() {
                    Vec::new()
                } else {
                    match agent_type {
                        AgentType::Codex => match decode_image_attachments(&images) {
                            Ok(paths) => paths,
                            Err(error) => {
                                if let Err(send_err) = tx
                                    .send(ServerMessage::session_error(session_id, error))
                                    .await
                                {
                                    tracing::debug!(
                                        %session_id,
                                        error = ?send_err,
                                        "Failed to send session error"
                                    );
                                    break 'ws_loop;
                                }
                                continue;
                            }
                        },
                        AgentType::Claude => {
                            match build_claude_prompt_jsonl(&prompt, &images) {
                                Ok(payload) => {
                                    input_format = Some("stream-json".to_string());
                                    stdin_payload = Some(payload);
                                }
                                Err(error) => {
                                    if let Err(send_err) = tx
                                        .send(ServerMessage::session_error(session_id, error))
                                        .await
                                    {
                                        tracing::debug!(
                                            %session_id,
                                            error = ?send_err,
                                            "Failed to send session error"
                                        );
                                        break 'ws_loop;
                                    }
                                    continue;
                                }
                            }
                            Vec::new()
                        }
                        AgentType::Gemini => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    "Image attachments are not supported for Gemini sessions",
                                ))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                    }
                };

                if agent_type == AgentType::Claude && stdin_payload.is_none() {
                    match build_claude_prompt_jsonl(&prompt, &[]) {
                        Ok(payload) => {
                            input_format = Some("stream-json".to_string());
                            stdin_payload = Some(payload);
                        }
                        Err(error) => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(session_id, error))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                    }
                }

                let prompt_for_history = prompt.clone();

                match session_manager
                    .start_session(StartSessionArgs {
                        session_id,
                        agent_type,
                        prompt: prompt_for_agent,
                        working_dir: working_dir_path.clone(),
                        model,
                        images: image_paths,
                        input_format,
                        stdin_payload,
                    })
                    .await
                {
                    Ok(mut event_rx) => {
                        if !hidden {
                            if let Err(error) = append_input_history(
                                &session_manager.core,
                                session_id,
                                &prompt_for_history,
                            )
                            .await
                            {
                                tracing::warn!(
                                    %session_id,
                                    error = %error,
                                    "Failed to persist input history"
                                );
                            }
                        }

                        if should_generate {
                            let core_ref = session_manager.core.clone();
                            let tx_clone = tx.clone();
                            let prompt_for_title = prompt.clone();
                            let working_dir_for_title = working_dir_path.clone();
                            tokio::spawn(async move {
                                match generate_title_and_branch_for_session(
                                    core_ref,
                                    session_id,
                                    prompt_for_title,
                                    working_dir_for_title,
                                )
                                .await
                                {
                                    Ok(Some(outcome)) => {
                                        if let Err(error) = tx_clone
                                            .send(ServerMessage::SessionMetadata {
                                                session_id,
                                                title: Some(outcome.title),
                                                workspace_id: outcome.workspace_id,
                                                workspace_branch: outcome.new_branch,
                                            })
                                            .await
                                        {
                                            tracing::debug!(
                                                %session_id,
                                                error = ?error,
                                                "Failed to send session metadata update"
                                            );
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(error) => {
                                        tracing::warn!(
                                            %session_id,
                                            error = %error,
                                            "Failed to generate session title"
                                        );
                                    }
                                }
                            });
                        }

                        // Auto-subscribe to the new session
                        let tx_clone = tx.clone();
                        let task = tokio::spawn(async move {
                            while let Ok(event) = event_rx.recv().await {
                                if tx_clone
                                    .send(ServerMessage::agent_event(session_id, event))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            // Session ended
                            let _ = tx_clone
                                .send(ServerMessage::SessionEnded {
                                    session_id,
                                    reason: "completed".to_string(),
                                    error: None,
                                })
                                .await;
                        });

                        let mut subs = subscriptions.write().await;
                        if let Some(existing) = subs.insert(session_id, task) {
                            existing.abort();
                        }

                        if let Err(send_err) = tx
                            .send(ServerMessage::session_started(session_id, agent_type, None))
                            .await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send session started"
                            );
                            break 'ws_loop;
                        }
                    }
                    Err(e) => {
                        if let Err(send_err) =
                            tx.send(ServerMessage::session_error(session_id, e)).await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send session error"
                            );
                            break 'ws_loop;
                        }
                    }
                }
            }

            ClientMessage::SendInput {
                session_id,
                input,
                hidden,
                images,
            } => {
                let agent_type = session_manager.get_agent_type(session_id).await;
                let mut input_payload = input.clone();
                let image_paths = if images.is_empty() {
                    Vec::new()
                } else {
                    match agent_type {
                        Some(AgentType::Codex) => match decode_image_attachments(&images) {
                            Ok(paths) => paths,
                            Err(error) => {
                                if let Err(send_err) = tx
                                    .send(ServerMessage::session_error(session_id, error))
                                    .await
                                {
                                    tracing::debug!(
                                        %session_id,
                                        error = ?send_err,
                                        "Failed to send session error"
                                    );
                                    break 'ws_loop;
                                }
                                continue;
                            }
                        },
                        Some(AgentType::Claude) => {
                            match build_claude_prompt_jsonl(&input, &images) {
                                Ok(payload) => {
                                    input_payload = payload;
                                    Vec::new()
                                }
                                Err(error) => {
                                    if let Err(send_err) = tx
                                        .send(ServerMessage::session_error(session_id, error))
                                        .await
                                    {
                                        tracing::debug!(
                                            %session_id,
                                            error = ?send_err,
                                            "Failed to send session error"
                                        );
                                        break 'ws_loop;
                                    }
                                    continue;
                                }
                            }
                        }
                        Some(AgentType::Gemini) => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    "Image attachments are not supported for Gemini sessions",
                                ))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                        None => Vec::new(),
                    }
                };

                if matches!(agent_type, Some(AgentType::Claude)) && images.is_empty() {
                    match build_claude_prompt_jsonl(&input, &[]) {
                        Ok(payload) => {
                            input_payload = payload;
                        }
                        Err(error) => {
                            if let Err(send_err) = tx
                                .send(ServerMessage::session_error(session_id, error))
                                .await
                            {
                                tracing::debug!(
                                    %session_id,
                                    error = ?send_err,
                                    "Failed to send session error"
                                );
                                break 'ws_loop;
                            }
                            continue;
                        }
                    }
                }

                if let Err(e) = session_manager
                    .send_input(session_id, input_payload, image_paths)
                    .await
                {
                    if let Err(send_err) =
                        tx.send(ServerMessage::session_error(session_id, e)).await
                    {
                        tracing::debug!(
                            %session_id,
                            error = ?send_err,
                            "Failed to send session error"
                        );
                        break 'ws_loop;
                    }
                } else if !hidden {
                    if let Err(error) =
                        append_input_history(&session_manager.core, session_id, &input).await
                    {
                        tracing::warn!(
                            %session_id,
                            error = %error,
                            "Failed to persist input history"
                        );
                    }
                }
            }

            ClientMessage::RespondToControl {
                session_id,
                request_id,
                response,
            } => {
                if let Err(e) = session_manager
                    .respond_to_control(session_id, request_id, response)
                    .await
                {
                    if let Err(send_err) =
                        tx.send(ServerMessage::session_error(session_id, e)).await
                    {
                        tracing::debug!(
                            %session_id,
                            error = ?send_err,
                            "Failed to send session error"
                        );
                        break 'ws_loop;
                    }
                }
            }

            ClientMessage::StopSession { session_id } => {
                // Clean up subscription first
                {
                    let mut subs = subscriptions.write().await;
                    if let Some(task) = subs.remove(&session_id) {
                        task.abort();
                    }
                }

                match session_manager.stop_session(session_id).await {
                    Ok(()) => {
                        if let Err(send_err) = tx
                            .send(ServerMessage::SessionEnded {
                                session_id,
                                reason: "stopped".to_string(),
                                error: None,
                            })
                            .await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send session ended"
                            );
                            break 'ws_loop;
                        }
                    }
                    Err(e) => {
                        if let Err(send_err) =
                            tx.send(ServerMessage::session_error(session_id, e)).await
                        {
                            tracing::debug!(
                                %session_id,
                                error = ?send_err,
                                "Failed to send session error"
                            );
                            break 'ws_loop;
                        }
                    }
                }
            }
        }
    }

    // Clean up all subscriptions when connection closes
    let subs = subscriptions.read().await;
    for (_, task) in subs.iter() {
        task.abort();
    }

    send_task.abort();
}
