use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, ErrorEvent, ReasoningEvent, SessionInitEvent,
    ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent, TurnFailedEvent,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;

const OPENCODE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const OPENCODE_SESSION_TIMEOUT: Duration = Duration::from_secs(10);
const OPENCODE_MODEL_CACHE_TTL_SECS: u64 = 60 * 60 * 24;

#[derive(Debug, Serialize)]
struct CreateSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PromptPart {
    Text { text: String },
}

#[derive(Debug, Serialize)]
struct PromptRequest {
    #[serde(rename = "sessionID")]
    session_id: String,
    parts: Vec<PromptPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<ModelRef>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelRef {
    #[serde(rename = "providerID")]
    provider_id: String,
    #[serde(rename = "modelID")]
    model_id: String,
}

impl ModelRef {
    fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
            return None;
        }
        let (provider_id, model_id) = trimmed.split_once('/')?;
        Some(Self {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct PermissionEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct MessagePart {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "type")]
    part_type: String,
    #[serde(rename = "callID")]
    call_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    state: Option<ToolState>,
    #[serde(default)]
    time: Option<TimeInfo>,
}

#[derive(Debug, Deserialize)]
struct ToolState {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    output: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TimeInfo {
    #[serde(default)]
    end: Option<u64>,
}

#[derive(Clone)]
struct OpenCodeClient {
    base_url: String,
    client: Client,
}

impl OpenCodeClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }

    async fn create_session(&self, title: Option<String>) -> io::Result<String> {
        let url = format!("{}/session", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&CreateSessionRequest { title })
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        if !status.is_success() {
            return Err(io::Error::other(format!(
                "Failed to create OpenCode session: {} - {}",
                status, text
            )));
        }

        let session: SessionResponse = serde_json::from_str(&text)
            .map_err(|err| io::Error::other(format!("Failed to parse session: {err} - {text}")))?;
        Ok(session.id)
    }

    async fn prompt(
        &self,
        session_id: &str,
        text: String,
        model: Option<ModelRef>,
    ) -> io::Result<()> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let request = PromptRequest {
            session_id: session_id.to_string(),
            parts: vec![PromptPart::Text { text }],
            model,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(io::Error::other(format!("Prompt failed: {text}")));
        }

        Ok(())
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        permission_id: &str,
        response: &str,
    ) -> io::Result<()> {
        let url = format!("{}/permission", self.base_url);
        let request = serde_json::json!({
            "sessionID": session_id,
            "permissionID": permission_id,
            "response": response,
        });

        self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        Ok(())
    }

    fn subscribe_events(&self) -> io::Result<EventSource> {
        let url = format!("{}/event", self.base_url);
        self.client
            .get(url)
            .eventsource()
            .map_err(|err| io::Error::other(err.to_string()))
    }
}

#[derive(Default)]
struct OpencodeEventState {
    started_tools: HashSet<String>,
}

pub struct OpencodeRunner {
    binary_path: Option<PathBuf>,
}

impl OpencodeRunner {
    pub fn new() -> Self {
        Self { binary_path: None }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            binary_path: Some(path),
        }
    }

    fn resolve_binary(&self) -> Option<PathBuf> {
        if let Some(path) = self.binary_path.clone() {
            if path.exists() {
                return Some(path);
            }
        }
        which::which("opencode").ok()
    }

    fn build_command(&self, config: &AgentStartConfig) -> Result<Command, AgentError> {
        let binary = self
            .resolve_binary()
            .ok_or_else(|| AgentError::BinaryNotFound("opencode".to_string()))?;

        let mut cmd = Command::new(binary);
        cmd.arg("serve");
        cmd.arg("--hostname").arg("127.0.0.1");
        cmd.arg("--port").arg("0");
        cmd.args(&config.additional_args);
        cmd.current_dir(&config.working_dir);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.env("NO_COLOR", "1");
        cmd.env("OPENCODE_CLIENT", "conduit");

        if std::env::var("OPENCODE_PERMISSION").is_err() {
            cmd.env("OPENCODE_PERMISSION", r#"{"*":"allow"}"#);
        }

        Ok(cmd)
    }

    fn parse_server_url(line: &str) -> Option<String> {
        let marker = "opencode server listening on";
        let idx = line.find(marker)?;
        let url = line[idx + marker.len()..].trim();
        if url.starts_with("http") {
            Some(url.to_string())
        } else {
            None
        }
    }

    fn parse_event(value: &Value) -> Option<(String, Value)> {
        if let Some(payload) = value.get("payload") {
            let event_type = payload.get("type")?.as_str()?.to_string();
            let props = payload.get("properties").cloned().unwrap_or(Value::Null);
            return Some((event_type, props));
        }
        let event_type = value.get("type")?.as_str()?.to_string();
        let props = value.get("properties").cloned().unwrap_or(Value::Null);
        Some((event_type, props))
    }

    async fn send_prompt(
        client: &OpenCodeClient,
        session_id: &str,
        model: Option<&ModelRef>,
        text: String,
        tx: &mpsc::Sender<AgentEvent>,
    ) {
        if tx.send(AgentEvent::TurnStarted).await.is_err() {
            return;
        }
        let result = client
            .prompt(session_id, text, model.cloned())
            .await
            .map_err(|err| err.to_string());
        if let Err(err) = result {
            let _ = tx
                .send(AgentEvent::Error(ErrorEvent {
                    message: format!("OpenCode prompt failed: {err}"),
                    is_fatal: true,
                }))
                .await;
        }
    }

    async fn handle_events(
        client: OpenCodeClient,
        session_id: String,
        event_tx: mpsc::Sender<AgentEvent>,
        pid: u32,
    ) {
        let mut state = OpencodeEventState::default();
        let mut events = match client.subscribe_events() {
            Ok(events) => events,
            Err(err) => {
                let _ = event_tx
                    .send(AgentEvent::Error(ErrorEvent {
                        message: format!("OpenCode SSE setup failed: {err}"),
                        is_fatal: false,
                    }))
                    .await;
                return;
            }
        };

        while let Some(event) = events.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    let value: Value = match serde_json::from_str(&msg.data) {
                        Ok(value) => value,
                        Err(err) => {
                            let _ = event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: format!("OpenCode event parse error: {err}"),
                                    is_fatal: false,
                                }))
                                .await;
                            continue;
                        }
                    };

                    let Some((event_type, properties)) = Self::parse_event(&value) else {
                        let _ = event_tx.send(AgentEvent::Raw { data: value }).await;
                        continue;
                    };

                    match event_type.as_str() {
                        "message.part.updated" => {
                            let part_value = properties.get("part").cloned().unwrap_or(Value::Null);
                            let delta = properties
                                .get("delta")
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            let part: MessagePart = match serde_json::from_value(part_value) {
                                Ok(part) => part,
                                Err(_) => continue,
                            };

                            if part.session_id != session_id {
                                continue;
                            }

                            match part.part_type.as_str() {
                                "text" => {
                                    if let Some(delta_text) = delta {
                                        let _ = event_tx
                                            .send(AgentEvent::AssistantMessage(
                                                AssistantMessageEvent {
                                                    text: delta_text,
                                                    is_final: false,
                                                },
                                            ))
                                            .await;
                                    }
                                    if part.time.as_ref().and_then(|t| t.end).is_some() {
                                        if let Some(text) = part.text {
                                            let _ = event_tx
                                                .send(AgentEvent::AssistantMessage(
                                                    AssistantMessageEvent {
                                                        text,
                                                        is_final: true,
                                                    },
                                                ))
                                                .await;
                                        }
                                    }
                                }
                                "reasoning" => {
                                    let text = delta.or(part.text).unwrap_or_default();
                                    if !text.is_empty() {
                                        let _ = event_tx
                                            .send(AgentEvent::AssistantReasoning(ReasoningEvent {
                                                text,
                                            }))
                                            .await;
                                    }
                                }
                                "tool" => {
                                    let tool_id = part
                                        .call_id
                                        .clone()
                                        .unwrap_or_else(|| part.session_id.clone());
                                    let mut tool_name = part.tool.clone();
                                    if let Some(state_info) = &part.state {
                                        if tool_name.is_none() {
                                            tool_name = state_info.title.clone();
                                        }
                                    }
                                    let tool_name = tool_name.unwrap_or_else(|| "tool".to_string());
                                    if let Some(state_info) = part.state {
                                        match state_info.status.as_deref() {
                                            Some("pending") | Some("running") => {
                                                if state.started_tools.insert(tool_id.clone()) {
                                                    let arguments = state_info
                                                        .input
                                                        .clone()
                                                        .unwrap_or(Value::Null);
                                                    let _ = event_tx
                                                        .send(AgentEvent::ToolStarted(
                                                            ToolStartedEvent {
                                                                tool_name,
                                                                tool_id,
                                                                arguments,
                                                            },
                                                        ))
                                                        .await;
                                                }
                                            }
                                            Some("completed") => {
                                                let result = state_info.output.map(|output| {
                                                    if let Some(text) = output.as_str() {
                                                        text.to_string()
                                                    } else {
                                                        output.to_string()
                                                    }
                                                });
                                                let _ = event_tx
                                                    .send(AgentEvent::ToolCompleted(
                                                        ToolCompletedEvent {
                                                            tool_id: tool_id.clone(),
                                                            success: true,
                                                            result,
                                                            error: None,
                                                        },
                                                    ))
                                                    .await;
                                                state.started_tools.remove(&tool_id);
                                            }
                                            Some("error") => {
                                                let error = state_info.error.or_else(|| {
                                                    state_info
                                                        .output
                                                        .as_ref()
                                                        .and_then(|o| o.as_str())
                                                        .map(|s| s.to_string())
                                                });
                                                let _ = event_tx
                                                    .send(AgentEvent::ToolCompleted(
                                                        ToolCompletedEvent {
                                                            tool_id: tool_id.clone(),
                                                            success: false,
                                                            result: None,
                                                            error,
                                                        },
                                                    ))
                                                    .await;
                                                state.started_tools.remove(&tool_id);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        "session.idle" => {
                            let matches_session = properties
                                .get("sessionID")
                                .and_then(|v| v.as_str())
                                .map(|sid| sid == session_id)
                                .unwrap_or(false);
                            if matches_session {
                                let _ = event_tx
                                    .send(AgentEvent::TurnCompleted(TurnCompletedEvent {
                                        usage: Default::default(),
                                    }))
                                    .await;
                                #[cfg(unix)]
                                unsafe {
                                    let _ = libc::kill(pid as i32, libc::SIGTERM);
                                }
                                break;
                            }
                        }
                        "session.error" => {
                            let matches_session = properties
                                .get("sessionID")
                                .and_then(|v| v.as_str())
                                .map(|sid| sid == session_id)
                                .unwrap_or(false);
                            if matches_session {
                                let message = properties
                                    .get("error")
                                    .map(|e| e.to_string())
                                    .unwrap_or_else(|| "OpenCode session error".to_string());
                                let _ = event_tx
                                    .send(AgentEvent::TurnFailed(TurnFailedEvent {
                                        error: message,
                                    }))
                                    .await;
                            }
                        }
                        "permission.asked" => {
                            let permission: PermissionEvent =
                                match serde_json::from_value(properties.clone()) {
                                    Ok(permission) => permission,
                                    Err(_) => continue,
                                };
                            if permission.session_id == session_id {
                                if let Err(err) = client
                                    .respond_permission(&session_id, &permission.id, "once")
                                    .await
                                {
                                    let _ = event_tx
                                        .send(AgentEvent::Error(ErrorEvent {
                                            message: format!(
                                                "Failed to respond to OpenCode permission: {err}"
                                            ),
                                            is_fatal: false,
                                        }))
                                        .await;
                                }
                            }
                        }
                        _ => {
                            let _ = event_tx
                                .send(AgentEvent::Raw {
                                    data: serde_json::json!({
                                        "type": event_type,
                                        "properties": properties,
                                    }),
                                })
                                .await;
                        }
                    }
                }
                Ok(Event::Open) => {}
                Err(err) => {
                    let _ = event_tx
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("OpenCode SSE error: {err}"),
                            is_fatal: false,
                        }))
                        .await;
                    break;
                }
            }
        }
    }
}

#[async_trait]
impl AgentRunner for OpencodeRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Opencode
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        let mut cmd = self.build_command(&config)?;
        let mut child = cmd.spawn().map_err(|_| AgentError::ProcessSpawnFailed)?;
        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;

        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;
        let stderr = child.stderr.take();

        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(256);
        let (url_tx, url_rx) = oneshot::channel::<String>();

        let event_tx_for_stdout = event_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut url_tx = Some(url_tx);

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(url) = OpencodeRunner::parse_server_url(&line) {
                            if let Some(sender) = url_tx.take() {
                                let _ = sender.send(url);
                            }
                        }
                    }
                    Err(err) => {
                        let _ = event_tx_for_stdout
                            .send(AgentEvent::Error(ErrorEvent {
                                message: format!("OpenCode stdout error: {err}"),
                                is_fatal: false,
                            }))
                            .await;
                        break;
                    }
                }
            }
        });

        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if !line.trim().is_empty() {
                                tracing::debug!("OpenCode stderr: {}", line.trim());
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let event_tx_for_wait = event_tx.clone();
        tokio::spawn(async move {
            if let Ok(status) = child.wait().await {
                if !status.success() {
                    let _ = event_tx_for_wait
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!(
                                "OpenCode server exited with status {:?}",
                                status.code()
                            ),
                            is_fatal: true,
                        }))
                        .await;
                }
            }
        });

        let base_url = match timeout(OPENCODE_READY_TIMEOUT, url_rx).await {
            Ok(Ok(url)) if !url.is_empty() => url,
            Ok(Ok(_)) => {
                return Err(AgentError::Timeout(
                    OPENCODE_READY_TIMEOUT.as_millis() as u64
                ))
            }
            Ok(Err(_)) => {
                return Err(AgentError::Timeout(
                    OPENCODE_READY_TIMEOUT.as_millis() as u64
                ))
            }
            Err(_) => {
                return Err(AgentError::Timeout(
                    OPENCODE_READY_TIMEOUT.as_millis() as u64
                ))
            }
        };

        let client = OpenCodeClient::new(base_url);

        let session_id = if let Some(resume) = &config.resume_session {
            resume.as_str().to_string()
        } else {
            timeout(
                OPENCODE_SESSION_TIMEOUT,
                client.create_session(Some("conduit".to_string())),
            )
            .await
            .map_err(|_| AgentError::Timeout(OPENCODE_SESSION_TIMEOUT.as_millis() as u64))?
            .map_err(AgentError::Io)?
        };

        let session_id = SessionId::from_string(session_id.clone());
        event_tx
            .send(AgentEvent::SessionInit(SessionInitEvent {
                session_id: session_id.clone(),
                model: config.model.clone(),
            }))
            .await
            .map_err(|_| AgentError::ChannelClosed)?;

        let model_ref = config.model.as_deref().and_then(ModelRef::parse);
        let input_tx = {
            let (input_tx, mut input_rx) = mpsc::channel::<AgentInput>(16);
            let client = client.clone();
            let event_tx = event_tx.clone();
            let session_id = session_id.as_str().to_string();
            let model_ref = model_ref.clone();
            tokio::spawn(async move {
                while let Some(input) = input_rx.recv().await {
                    match input {
                        AgentInput::CodexPrompt { text, images } => {
                            if !images.is_empty() {
                                let _ = event_tx
                                    .send(AgentEvent::Error(ErrorEvent {
                                        message: "OpenCode runner does not support image attachments yet.".to_string(),
                                        is_fatal: false,
                                    }))
                                    .await;
                            }
                            OpencodeRunner::send_prompt(
                                &client,
                                &session_id,
                                model_ref.as_ref(),
                                text,
                                &event_tx,
                            )
                            .await;
                        }
                        AgentInput::ClaudeJsonl(_) => {
                            let _ = event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: "OpenCode runner does not support Claude JSONL input."
                                        .to_string(),
                                    is_fatal: false,
                                }))
                                .await;
                        }
                    }
                }
            });
            input_tx
        };

        if !config.prompt.trim().is_empty() {
            OpencodeRunner::send_prompt(
                &client,
                session_id.as_str(),
                model_ref.as_ref(),
                config.prompt.clone(),
                &event_tx,
            )
            .await;
        }

        tokio::spawn({
            let client = client.clone();
            let event_tx = event_tx.clone();
            let session_id = session_id.as_str().to_string();
            async move { OpencodeRunner::handle_events(client, session_id, event_tx, pid).await }
        });

        Ok(AgentHandle::new(event_rx, pid, Some(input_tx)))
    }

    async fn send_input(&self, handle: &AgentHandle, input: AgentInput) -> Result<(), AgentError> {
        let Some(ref input_tx) = handle.input_tx else {
            return Err(AgentError::ChannelClosed);
        };
        input_tx
            .send(input)
            .await
            .map_err(|_| AgentError::ChannelClosed)
    }

    async fn stop(&self, handle: &AgentHandle) -> Result<(), AgentError> {
        #[cfg(unix)]
        {
            let result = unsafe { libc::kill(handle.pid as i32, libc::SIGTERM) };
            if result == -1 {
                return Err(AgentError::Io(std::io::Error::last_os_error()));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = handle;
            return Err(AgentError::NotSupported(
                "Stop not implemented on this platform".into(),
            ));
        }
        Ok(())
    }

    async fn kill(&self, handle: &AgentHandle) -> Result<(), AgentError> {
        #[cfg(unix)]
        {
            let result = unsafe { libc::kill(handle.pid as i32, libc::SIGKILL) };
            if result == -1 {
                return Err(AgentError::Io(std::io::Error::last_os_error()));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = handle;
            return Err(AgentError::NotSupported(
                "Kill not implemented on this platform".into(),
            ));
        }
        Ok(())
    }

    fn is_available(&self) -> bool {
        self.resolve_binary().is_some()
    }

    fn binary_path(&self) -> Option<PathBuf> {
        self.resolve_binary()
    }
}

impl Default for OpencodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelCache {
    generated_at: u64,
    models: Vec<String>,
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("conduit").join("opencode_models.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_cache(path: &PathBuf) -> Option<ModelCache> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn cache_is_fresh(cache: &ModelCache) -> bool {
    now_secs().saturating_sub(cache.generated_at) <= OPENCODE_MODEL_CACHE_TTL_SECS
}

fn save_cache(path: &PathBuf, models: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = ModelCache {
        generated_at: now_secs(),
        models: models.to_vec(),
    };
    let payload = serde_json::to_string_pretty(&cache).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, payload)
}

fn parse_models_output(text: &str) -> Vec<String> {
    let mut models = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.contains(' ') {
            continue;
        }
        if let Some((provider, model)) = trimmed.split_once('/') {
            if provider.is_empty() || model.is_empty() {
                continue;
            }
            if !trimmed.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '@')
            }) {
                continue;
            }
            models.push(trimmed.to_string());
        }
    }
    models.sort();
    models.dedup();
    models
}

fn discover_models(binary: &PathBuf) -> std::io::Result<Vec<String>> {
    let output = std::process::Command::new(binary).arg("models").output()?;
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "opencode models failed with status {:?}",
            output.status.code()
        )));
    }
    Ok(parse_models_output(&combined))
}

pub fn load_opencode_models(binary_path: Option<PathBuf>) -> Vec<String> {
    let binary = binary_path
        .filter(|path| path.exists())
        .or_else(|| which::which("opencode").ok());
    let Some(binary) = binary else {
        return Vec::new();
    };

    let cache = cache_path().and_then(|path| {
        let cached = load_cache(&path);
        if cached.as_ref().is_some_and(cache_is_fresh) {
            return cached.map(|cache| cache.models);
        }
        None
    });
    if let Some(models) = cache {
        return models;
    }

    match discover_models(&binary) {
        Ok(models) if !models.is_empty() => {
            if let Some(path) = cache_path() {
                if let Err(err) = save_cache(&path, &models) {
                    tracing::debug!(error = %err, "Failed to save OpenCode model cache");
                }
            }
            models
        }
        Ok(_) => Vec::new(),
        Err(err) => {
            tracing::debug!(error = %err, "Failed to discover OpenCode models");
            if let Some(path) = cache_path() {
                if let Some(cache) = load_cache(&path) {
                    return cache.models;
                }
            }
            Vec::new()
        }
    }
}
