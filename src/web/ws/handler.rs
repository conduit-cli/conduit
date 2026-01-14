//! WebSocket connection handler for real-time agent communication.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

use crate::agent::events::AgentEvent;
use crate::agent::runner::{AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::core::ConduitCore;

use super::messages::{ClientMessage, ServerMessage};

/// Active session state tracked by the WebSocket handler.
struct ActiveSession {
    agent_type: AgentType,
    /// Process ID for stopping the agent
    pid: u32,
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

impl SessionManager {
    pub fn new(core: Arc<RwLock<ConduitCore>>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            core,
        }
    }

    /// Start a new agent session.
    pub async fn start_session(
        &self,
        session_id: Uuid,
        agent_type: AgentType,
        prompt: String,
        working_dir: PathBuf,
        model: Option<String>,
    ) -> Result<broadcast::Receiver<AgentEvent>, String> {
        // Check if session already exists
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&session_id) {
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

        // Start the agent
        let mut handle = runner
            .start(config)
            .await
            .map_err(|e| format!("Failed to start agent: {}", e))?;

        // Create broadcast channel for events
        let (event_tx, event_rx) = broadcast::channel(256);

        let pid = handle.pid;
        let input_tx = handle.input_tx.take();

        // Store the session (without handle, we'll own the events receiver in the spawn)
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(
                session_id,
                ActiveSession {
                    agent_type,
                    pid,
                    event_tx: event_tx.clone(),
                    input_tx,
                },
            );
        }

        // Spawn task to forward events from agent to broadcast channel
        let sessions_ref = self.sessions.clone();
        tokio::spawn(async move {
            while let Some(event) = handle.events.recv().await {
                // Try to send, ignore errors (no subscribers)
                let _ = event_tx.send(event);
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
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(&session_id) {
            Ok(session.event_tx.subscribe())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    /// Stop a running session.
    pub async fn stop_session(&self, session_id: Uuid) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(&session_id) {
            // Kill the process by PID
            #[cfg(unix)]
            {
                use std::process::Command;
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(session.pid.to_string())
                    .status();
            }
            #[cfg(windows)]
            {
                use std::process::Command;
                let _ = Command::new("taskkill")
                    .args(["/PID", &session.pid.to_string(), "/F"])
                    .status();
            }
        }
        Ok(())
    }

    /// Send input to a running session.
    pub async fn send_input(&self, session_id: Uuid, input: String) -> Result<(), String> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        // Get the input sender
        let input_tx = session
            .input_tx
            .as_ref()
            .ok_or_else(|| "Session does not support input".to_string())?;

        // Send as appropriate input type based on agent
        let agent_input = match session.agent_type {
            AgentType::Claude => AgentInput::ClaudeJsonl(input),
            AgentType::Codex | AgentType::Gemini => AgentInput::CodexPrompt {
                text: input,
                images: vec![],
            },
        };

        input_tx
            .send(agent_input)
            .await
            .map_err(|e| format!("Failed to send input: {}", e))?;

        Ok(())
    }

    /// Get the agent type for a session.
    pub async fn get_agent_type(&self, session_id: Uuid) -> Option<AgentType> {
        let sessions = self.sessions.read().await;
        sessions.get(&session_id).map(|s| s.agent_type)
    }
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
    while let Some(result) = ws_receiver.next().await {
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
                let _ = tx
                    .send(ServerMessage::error(format!("Invalid message: {}", e)))
                    .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::Ping => {
                let _ = tx.send(ServerMessage::Pong).await;
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
                        subs.insert(session_id, task);

                        let _ = tx.send(ServerMessage::Subscribed { session_id }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::session_error(session_id, e)).await;
                    }
                }
            }

            ClientMessage::Unsubscribe { session_id } => {
                let mut subs = subscriptions.write().await;
                if let Some(task) = subs.remove(&session_id) {
                    task.abort();
                }
                let _ = tx.send(ServerMessage::Unsubscribed { session_id }).await;
            }

            ClientMessage::StartSession {
                session_id,
                prompt,
                working_dir,
                model,
            } => {
                // Look up session in database to get agent type
                let core = session_manager.core.read().await;
                let agent_type = if let Some(store) = core.session_tab_store() {
                    match store.get_by_id(session_id) {
                        Ok(Some(tab)) => tab.agent_type,
                        Ok(None) => {
                            let _ = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    "Session not found in database",
                                ))
                                .await;
                            continue;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(ServerMessage::session_error(
                                    session_id,
                                    format!("Database error: {}", e),
                                ))
                                .await;
                            continue;
                        }
                    }
                } else {
                    let _ = tx
                        .send(ServerMessage::session_error(
                            session_id,
                            "Database not available",
                        ))
                        .await;
                    continue;
                };
                drop(core);

                match session_manager
                    .start_session(
                        session_id,
                        agent_type,
                        prompt,
                        PathBuf::from(working_dir),
                        model,
                    )
                    .await
                {
                    Ok(mut event_rx) => {
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
                        subs.insert(session_id, task);

                        let _ = tx
                            .send(ServerMessage::session_started(session_id, agent_type, None))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::session_error(session_id, e)).await;
                    }
                }
            }

            ClientMessage::SendInput { session_id, input } => {
                if let Err(e) = session_manager.send_input(session_id, input).await {
                    let _ = tx.send(ServerMessage::session_error(session_id, e)).await;
                }
            }

            ClientMessage::RespondToControl {
                session_id,
                request_id: _,
                allow: _,
            } => {
                // TODO: Implement control request handling
                // This requires integration with the agent's control request mechanism
                let _ = tx
                    .send(ServerMessage::session_error(
                        session_id,
                        "Control request handling not yet implemented",
                    ))
                    .await;
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
                        let _ = tx
                            .send(ServerMessage::SessionEnded {
                                session_id,
                                reason: "stopped".to_string(),
                                error: None,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::session_error(session_id, e)).await;
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
