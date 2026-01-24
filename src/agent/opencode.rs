use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;

use crate::agent::display::MessageDisplay;
use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, ErrorEvent, QuestionOption, ReasoningEvent,
    SessionInitEvent, ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent, TurnFailedEvent,
    UserQuestion,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;
use crate::agent::ModelRegistry;

const OPENCODE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const OPENCODE_SESSION_TIMEOUT: Duration = Duration::from_secs(10);
const OPENCODE_PROMPT_TIMEOUT: Duration = Duration::from_secs(60);
const OPENCODE_MODEL_CACHE_TTL_SECS: u64 = 60 * 60 * 24;
const OPENCODE_LOG_PREVIEW_CHARS: usize = 200;

async fn send_event_or_log(
    event_tx: &mpsc::Sender<AgentEvent>,
    event: AgentEvent,
    context: &'static str,
) -> bool {
    if let Err(err) = event_tx.send(event).await {
        tracing::debug!(error = %err, context, "OpenCode event channel closed");
        return false;
    }
    true
}

fn truncate_for_log(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}...(truncated, {char_count} chars)")
}

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
struct QuestionOptionInfo {
    label: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
struct QuestionInfo {
    #[serde(default)]
    header: String,
    question: String,
    options: Vec<QuestionOptionInfo>,
    #[serde(default, rename = "multiple")]
    multiple: bool,
}

#[derive(Debug, Deserialize)]
struct QuestionRequest {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    questions: Vec<QuestionInfo>,
}

#[derive(Debug, Deserialize)]
struct QuestionResponseEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "requestID")]
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct MessagePart {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "messageID")]
    message_id: Option<String>,
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
struct MessagePartInfo {
    #[serde(rename = "type")]
    part_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageTime {
    #[serde(default)]
    completed: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MessageInfo {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    role: String,
    #[serde(default)]
    parts: Vec<MessagePartInfo>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    time: Option<MessageTime>,
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

#[derive(Default)]
struct OpencodeSharedState {
    completed_messages: Mutex<HashSet<String>>,
    part_texts: Mutex<HashMap<String, String>>,
    turn_in_flight: AtomicBool,
    sse_active: AtomicBool,
}

impl OpencodeSharedState {
    async fn mark_completed(&self, message_id: &str) -> bool {
        let mut guard = self.completed_messages.lock().await;
        guard.insert(message_id.to_string())
    }

    async fn part_text(&self, key: &str) -> Option<String> {
        let guard = self.part_texts.lock().await;
        guard.get(key).cloned()
    }

    async fn set_part_text(&self, key: String, text: String) {
        let mut guard = self.part_texts.lock().await;
        guard.insert(key, text);
    }

    async fn append_part_text(&self, key: String, text: &str) {
        let mut guard = self.part_texts.lock().await;
        guard
            .entry(key)
            .and_modify(|value| value.push_str(text))
            .or_insert_with(|| text.to_string());
    }

    async fn clear_part_text(&self, key: &str) {
        let mut guard = self.part_texts.lock().await;
        guard.remove(key);
    }

    fn set_turn_in_flight(&self, in_flight: bool) {
        self.turn_in_flight.store(in_flight, Ordering::SeqCst);
    }

    fn take_turn_in_flight(&self) -> bool {
        self.turn_in_flight.swap(false, Ordering::SeqCst)
    }

    fn try_mark_sse_active(&self) -> bool {
        self.sse_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn is_sse_active(&self) -> bool {
        self.sse_active.load(Ordering::SeqCst)
    }

    fn mark_sse_inactive(&self) {
        self.sse_active.store(false, Ordering::SeqCst);
    }
}

struct SseActiveGuard {
    shared_state: Arc<OpencodeSharedState>,
}

impl SseActiveGuard {
    fn new(shared_state: Arc<OpencodeSharedState>) -> Self {
        Self { shared_state }
    }
}

impl Drop for SseActiveGuard {
    fn drop(&mut self) {
        self.shared_state.mark_sse_inactive();
    }
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
        tracing::debug!(url = %url, title = ?title, "OpenCode create session request");
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
        tracing::debug!(
            status = %status,
            body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
            "OpenCode create session response"
        );

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
    ) -> io::Result<String> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let model_label = model
            .as_ref()
            .map(|m| format!("{}/{}", m.provider_id, m.model_id))
            .unwrap_or_else(|| "default".to_string());
        tracing::debug!(
            session_id,
            model = %model_label,
            text_len = text.len(),
            text_preview = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
            "OpenCode prompt request"
        );
        let request = PromptRequest {
            session_id: session_id.to_string(),
            parts: vec![PromptPart::Text { text }],
            model,
        };

        let response = match timeout(
            OPENCODE_PROMPT_TIMEOUT,
            self.client.post(&url).json(&request).send(),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                tracing::error!(
                    session_id,
                    error = %err,
                    url = %url,
                    "OpenCode prompt request failed"
                );
                return Err(io::Error::other(err.to_string()));
            }
            Err(_) => {
                tracing::error!(
                    session_id,
                    url = %url,
                    timeout_secs = OPENCODE_PROMPT_TIMEOUT.as_secs(),
                    "OpenCode prompt request timed out"
                );
                return Err(io::Error::other(format!(
                    "Prompt request timed out after {}s",
                    OPENCODE_PROMPT_TIMEOUT.as_secs()
                )));
            }
        };
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        if !status.is_success() {
            tracing::debug!(
                session_id,
                status = %status,
                body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
                "OpenCode prompt response error"
            );
            return Err(io::Error::other(format!("Prompt failed: {text}")));
        }

        tracing::debug!(
            session_id,
            status = %status,
            body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
            "OpenCode prompt response"
        );
        Ok(text)
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
        tracing::debug!(
            session_id,
            permission_id,
            response = %response,
            "OpenCode permission response request"
        );

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::debug!(
            session_id,
            permission_id,
            status = %status,
            body = %truncate_for_log(&body, OPENCODE_LOG_PREVIEW_CHARS),
            "OpenCode permission response"
        );
        if !status.is_success() {
            return Err(io::Error::other(format!(
                "Permission response failed: {status} - {body}"
            )));
        }

        Ok(())
    }

    async fn reply_question(&self, request_id: &str, answers: Vec<Vec<String>>) -> io::Result<()> {
        let url = format!("{}/question/{}/reply", self.base_url, request_id);
        let answer_count = answers.len();
        let request = serde_json::json!({
            "answers": answers,
        });
        tracing::debug!(request_id, answer_count, "OpenCode question reply request");

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;
        let status = response.status();

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            tracing::debug!(
                request_id,
                status = %status,
                body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
                "OpenCode question reply error"
            );
            return Err(io::Error::other(format!("Question reply failed: {}", text)));
        }

        tracing::debug!(
            request_id,
            status = %status,
            "OpenCode question reply response"
        );
        Ok(())
    }

    async fn reject_question(&self, request_id: &str) -> io::Result<()> {
        let url = format!("{}/question/{}/reject", self.base_url, request_id);
        tracing::debug!(request_id, "OpenCode question reject request");
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;
        let status = response.status();

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            tracing::debug!(
                request_id,
                status = %status,
                body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
                "OpenCode question reject error"
            );
            return Err(io::Error::other(format!(
                "Question reject failed: {}",
                text
            )));
        }

        tracing::debug!(
            request_id,
            status = %status,
            "OpenCode question reject response"
        );
        Ok(())
    }

    async fn get_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> io::Result<(MessageInfo, Vec<Value>, Value)> {
        let url = format!(
            "{}/session/{}/message/{}",
            self.base_url, session_id, message_id
        );
        tracing::debug!(
            session_id,
            message_id,
            url = %url,
            "OpenCode get message request"
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            tracing::debug!(
                session_id,
                message_id,
                status = %status,
                body = %truncate_for_log(&text, OPENCODE_LOG_PREVIEW_CHARS),
                "OpenCode get message error"
            );
            return Err(io::Error::other(format!(
                "Get message failed: {status} - {text}"
            )));
        }

        let value: Value =
            serde_json::from_str(&text).map_err(|err| io::Error::other(err.to_string()))?;
        let info_value = value.get("info").cloned().unwrap_or(Value::Null);
        let info: MessageInfo = serde_json::from_value(info_value.clone())
            .map_err(|err| io::Error::other(err.to_string()))?;
        let parts = value
            .get("parts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok((info, parts, info_value))
    }

    fn subscribe_events(&self) -> io::Result<EventSource> {
        let url = format!("{}/event", self.base_url);
        tracing::debug!(url = %url, "OpenCode SSE subscribe");
        self.client
            .get(url)
            .eventsource()
            .map_err(|err| io::Error::other(err.to_string()))
    }
}

#[derive(Default)]
struct OpencodeEventState {
    started_tools: HashSet<String>,
    message_roles: HashMap<String, String>,
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

    fn should_ignore_part(part: &MessagePart, state: &OpencodeEventState) -> bool {
        if let Some(message_id) = part.message_id.as_ref() {
            if matches!(
                state.message_roles.get(message_id).map(String::as_str),
                Some("user")
            ) {
                return true;
            }
        }
        false
    }

    fn extract_tool_error_message(state: &ToolState) -> Option<String> {
        fn non_empty(value: Option<&str>) -> Option<String> {
            value
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        }

        if let Some(error) = non_empty(state.error.as_deref()) {
            return Some(error);
        }

        let output = state.output.as_ref()?;
        if let Some(text) = non_empty(output.as_str()) {
            return Some(text);
        }

        if let Some(obj) = output.as_object() {
            if let Some(text) = non_empty(obj.get("message").and_then(|v| v.as_str())) {
                return Some(text);
            }
            if let Some(err) = obj.get("error") {
                if let Some(text) = non_empty(err.get("message").and_then(|v| v.as_str())) {
                    return Some(text);
                }
                if let Some(text) = non_empty(err.as_str()) {
                    return Some(text);
                }
            }
            if let Some(issues) = obj.get("issues") {
                let rendered = issues.to_string();
                if !rendered.is_empty() {
                    return Some(rendered);
                }
            }
        }

        let rendered = output.to_string();
        if rendered.is_empty() {
            None
        } else {
            Some(rendered)
        }
    }

    async fn send_prompt(
        client: &OpenCodeClient,
        session_id: &str,
        model: Option<&ModelRef>,
        text: String,
        tx: &mpsc::Sender<AgentEvent>,
        shared_state: &OpencodeSharedState,
    ) {
        shared_state.set_turn_in_flight(true);
        let model_label = model
            .map(|m| format!("{}/{}", m.provider_id, m.model_id))
            .unwrap_or_else(|| "default".to_string());
        tracing::debug!(
            session_id,
            model = %model_label,
            text_len = text.len(),
            "OpenCode send prompt"
        );
        if tx.send(AgentEvent::TurnStarted).await.is_err() {
            shared_state.set_turn_in_flight(false);
            return;
        }
        let response_body = match client.prompt(session_id, text, model.cloned()).await {
            Ok(body) => body,
            Err(err) => {
                shared_state.set_turn_in_flight(false);
                tracing::error!(
                    session_id,
                    error = %err,
                    "OpenCode prompt failed"
                );
                if tx
                    .send(AgentEvent::Error(ErrorEvent {
                        message: format!("OpenCode prompt failed: {err}"),
                        is_fatal: true,
                        code: None,
                        details: None,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
                return;
            }
        };

        if let Err(err) =
            Self::maybe_emit_prompt_response(client, session_id, &response_body, tx, shared_state)
                .await
        {
            tracing::debug!(
                session_id,
                error = %err,
                "OpenCode prompt response parse failed"
            );
        }
    }

    async fn maybe_emit_prompt_response(
        client: &OpenCodeClient,
        session_id: &str,
        response_body: &str,
        tx: &mpsc::Sender<AgentEvent>,
        shared_state: &OpencodeSharedState,
    ) -> io::Result<()> {
        let value: Value =
            serde_json::from_str(response_body).map_err(|err| io::Error::other(err.to_string()))?;
        let info_value = value.get("info").cloned().unwrap_or(Value::Null);
        let info: MessageInfo = serde_json::from_value(info_value.clone())
            .map_err(|err| io::Error::other(err.to_string()))?;
        let parts = value
            .get("parts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut content_override = if parts.is_empty() {
            None
        } else {
            Some(Self::extract_message_parts_from_parts(&parts))
        };
        let mut info_value_for_emit = info_value.clone();
        let mut info_for_emit = info;
        let has_error = info_for_emit.error.is_some();

        if shared_state.is_sse_active() && !has_error {
            return Ok(());
        }

        let completed = info_for_emit
            .time
            .as_ref()
            .and_then(|t| t.completed)
            .is_some();
        let missing_content = info_for_emit
            .text
            .as_ref()
            .map(|t| t.is_empty())
            .unwrap_or(true)
            && info_for_emit.parts.is_empty()
            && content_override
                .as_ref()
                .map(|(text, reasoning)| text.is_empty() && reasoning.is_empty())
                .unwrap_or(true);
        let message_id = info_for_emit.id.clone();

        if completed && missing_content {
            match client.get_message(session_id, &message_id).await {
                Ok((full_info, fetched_parts, raw_info)) => {
                    info_for_emit = full_info;
                    info_value_for_emit = raw_info;
                    if !fetched_parts.is_empty() {
                        content_override =
                            Some(Self::extract_message_parts_from_parts(&fetched_parts));
                    }
                }
                Err(err) => {
                    tracing::debug!(
                        session_id,
                        message_id = %message_id,
                        error = %err,
                        "OpenCode get message failed"
                    );
                }
            }
        }
        Self::emit_message_from_info(
            session_id,
            &info_for_emit,
            Some(&info_value_for_emit),
            content_override,
            tx,
            shared_state,
        )
        .await;
        Ok(())
    }

    async fn emit_message_from_info(
        session_id: &str,
        info: &MessageInfo,
        info_value: Option<&Value>,
        content_override: Option<(String, String)>,
        tx: &mpsc::Sender<AgentEvent>,
        shared_state: &OpencodeSharedState,
    ) {
        if info.session_id != session_id || info.role != "assistant" {
            return;
        }

        let completed = info.time.as_ref().and_then(|t| t.completed).is_some();
        if !completed {
            return;
        }

        if !shared_state.mark_completed(&info.id).await {
            return;
        }

        let content_override_ref = content_override.as_ref();
        let (mut text, mut reasoning) = content_override_ref
            .map(|(text, reasoning)| (text.clone(), reasoning.clone()))
            .unwrap_or_else(|| (String::new(), String::new()));
        let error_message = info.error.as_ref().and_then(|value| {
            value
                .get("data")
                .and_then(|data| data.get("message"))
                .and_then(|message| message.as_str())
                .or_else(|| value.get("message").and_then(|message| message.as_str()))
                .map(|message| message.to_string())
        });
        if text.is_empty() && reasoning.is_empty() {
            let (fallback_text, fallback_reasoning) = Self::extract_message_parts(info);
            text = fallback_text;
            reasoning = fallback_reasoning;
        }
        if text.is_empty() && reasoning.is_empty() {
            if let Some(info_value) = info_value {
                let (fallback_text, fallback_reasoning) =
                    Self::extract_message_parts_value(info_value);
                if text.is_empty() {
                    text = fallback_text;
                }
                if reasoning.is_empty() {
                    reasoning = fallback_reasoning;
                }
            }
        }

        if text.is_empty() && reasoning.is_empty() {
            if let Some(error) = error_message {
                let lowered = error.to_lowercase();
                let is_model_error = lowered.contains("model")
                    && (lowered.contains("not supported")
                        || lowered.contains("not found")
                        || lowered.contains("unavailable"));
                if is_model_error {
                    invalidate_model_cache();
                    if let Some(model_id) = extract_model_id_from_error(&error) {
                        let normalized = if model_id.contains('/') {
                            model_id
                        } else {
                            format!("opencode/{}", model_id)
                        };
                        ModelRegistry::drop_opencode_model(&normalized);
                    }
                }
                tracing::warn!(
                    session_id,
                    message_id = %info.id,
                    error = %error,
                    "OpenCode assistant message error"
                );
                if !send_event_or_log(
                    tx,
                    AgentEvent::Error(ErrorEvent {
                        message: format!("OpenCode error: {}", error),
                        is_fatal: true,
                        code: if is_model_error {
                            Some("model_not_found".to_string())
                        } else {
                            None
                        },
                        details: None,
                    }),
                    "opencode_assistant_error",
                )
                .await
                {
                    return;
                }
            }
        }

        tracing::debug!(
            session_id,
            message_id = %info.id,
            role = %info.role,
            completed,
            part_count = content_override_ref.map(|_| 1).unwrap_or(0),
            text_len = text.len(),
            reasoning_len = reasoning.len(),
            "OpenCode assistant message parsed"
        );
        if text.is_empty() && reasoning.is_empty() {
            tracing::debug!(
                session_id,
                message_id = %info.id,
                "OpenCode assistant message has no text content"
            );
            if let Some(info_value) = info_value {
                tracing::debug!(
                    session_id,
                    message_id = %info.id,
                    info = %truncate_for_log(
                        &info_value.to_string(),
                        OPENCODE_LOG_PREVIEW_CHARS
                    ),
                    "OpenCode assistant info payload"
                );
            }
        }
        if !reasoning.is_empty()
            && tx
                .send(AgentEvent::AssistantReasoning(ReasoningEvent {
                    text: reasoning,
                }))
                .await
                .is_err()
        {
            return;
        }

        if !text.is_empty()
            && tx
                .send(AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text,
                    is_final: true,
                }))
                .await
                .is_err()
        {
            return;
        }

        if shared_state.take_turn_in_flight()
            && tx
                .send(AgentEvent::TurnCompleted(TurnCompletedEvent {
                    usage: Default::default(),
                }))
                .await
                .is_err()
        {
            tracing::debug!("OpenCode turn completion dropped; channel closed");
        }
    }

    fn extract_message_parts(info: &MessageInfo) -> (String, String) {
        let mut text = String::new();
        let mut reasoning = String::new();

        for part in &info.parts {
            match part.part_type.as_str() {
                "text" => {
                    if let Some(chunk) = &part.text {
                        text.push_str(chunk);
                    }
                }
                "reasoning" => {
                    if let Some(chunk) = &part.text {
                        reasoning.push_str(chunk);
                    }
                }
                _ => {}
            }
        }

        if text.is_empty() {
            if let Some(fallback) = &info.text {
                text.push_str(fallback);
            }
        }

        (text, reasoning)
    }

    fn extract_message_parts_value(info: &Value) -> (String, String) {
        let mut text = String::new();
        let mut reasoning = String::new();

        if let Some(parts) = info.get("parts").and_then(|v| v.as_array()) {
            for part in parts {
                let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let chunk = part
                    .get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| part.get("content").and_then(|v| v.as_str()))
                    .or_else(|| part.get("output").and_then(|v| v.as_str()));

                if let Some(chunk) = chunk {
                    match part_type {
                        "reasoning" => reasoning.push_str(chunk),
                        "text" | "markdown" | "output" => text.push_str(chunk),
                        _ => text.push_str(chunk),
                    }
                }
            }
        }

        if text.is_empty() {
            if let Some(fallback) = info.get("text").and_then(|v| v.as_str()) {
                text.push_str(fallback);
            }
        }

        if text.is_empty() {
            if let Some(fallback) = info.get("content").and_then(|v| v.as_str()) {
                text.push_str(fallback);
            }
        }

        if text.is_empty() {
            if let Some(fallback) = info.get("output").and_then(|v| v.as_str()) {
                text.push_str(fallback);
            }
        }

        if text.is_empty() {
            if let Some(fallback) = info.get("message").and_then(|v| v.as_str()) {
                text.push_str(fallback);
            }
        }

        if text.is_empty() {
            if let Some(result) = info.get("result") {
                if let Some(fallback) = result.as_str() {
                    text.push_str(fallback);
                } else if let Some(obj) = result.as_object() {
                    if let Some(fallback) = obj
                        .get("text")
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("content").and_then(|v| v.as_str()))
                        .or_else(|| obj.get("output").and_then(|v| v.as_str()))
                        .or_else(|| obj.get("message").and_then(|v| v.as_str()))
                    {
                        text.push_str(fallback);
                    }
                }
            }
        }

        if text.is_empty() {
            if let Some(summary) = info.get("summary") {
                if let Some(fallback) = summary
                    .get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| summary.get("title").and_then(|v| v.as_str()))
                {
                    text.push_str(fallback);
                }
            }
        }

        (text, reasoning)
    }

    fn extract_message_parts_from_parts(parts: &[Value]) -> (String, String) {
        let mut text = String::new();
        let mut reasoning = String::new();

        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let chunk = part
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| part.get("content").and_then(|v| v.as_str()))
                .or_else(|| part.get("output").and_then(|v| v.as_str()))
                .or_else(|| part.get("message").and_then(|v| v.as_str()));

            if let Some(chunk) = chunk {
                match part_type {
                    "reasoning" => reasoning.push_str(chunk),
                    "text" | "markdown" | "output" => text.push_str(chunk),
                    _ => text.push_str(chunk),
                }
            }
        }

        (text, reasoning)
    }

    async fn handle_events(
        client: OpenCodeClient,
        session_id: String,
        event_tx: mpsc::Sender<AgentEvent>,
        shared_state: Arc<OpencodeSharedState>,
    ) {
        let _sse_guard = SseActiveGuard::new(shared_state.clone());
        let mut state = OpencodeEventState::default();
        let mut events = match client.subscribe_events() {
            Ok(events) => events,
            Err(err) => {
                if !send_event_or_log(
                    &event_tx,
                    AgentEvent::Error(ErrorEvent {
                        message: format!("OpenCode SSE setup failed: {err}"),
                        is_fatal: false,
                        code: None,
                        details: None,
                    }),
                    "opencode_sse_setup",
                )
                .await
                {
                    return;
                }
                return;
            }
        };

        while let Some(event) = events.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    tracing::debug!(
                        event = %msg.event,
                        id = ?msg.id,
                        data_len = msg.data.len(),
                        data_preview = %truncate_for_log(&msg.data, OPENCODE_LOG_PREVIEW_CHARS),
                        "OpenCode SSE message"
                    );
                    let value: Value = match serde_json::from_str(&msg.data) {
                        Ok(value) => value,
                        Err(err) => {
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::Error(ErrorEvent {
                                    message: format!("OpenCode event parse error: {err}"),
                                    is_fatal: false,
                                    code: None,
                                    details: None,
                                }),
                                "opencode_event_parse",
                            )
                            .await
                            {
                                return;
                            }
                            continue;
                        }
                    };

                    let Some((event_type, properties)) = Self::parse_event(&value) else {
                        if !send_event_or_log(
                            &event_tx,
                            AgentEvent::Raw { data: value },
                            "opencode_raw",
                        )
                        .await
                        {
                            return;
                        }
                        continue;
                    };

                    match event_type.as_str() {
                        "message.updated" => {
                            let info_value = properties.get("info").cloned().unwrap_or(Value::Null);
                            let mut info: MessageInfo =
                                match serde_json::from_value(info_value.clone()) {
                                    Ok(info) => info,
                                    Err(_) => continue,
                                };
                            state
                                .message_roles
                                .insert(info.id.clone(), info.role.clone());
                            if info.role == "assistant" {
                                let has_streamable_parts = info.parts.iter().any(|part| {
                                    matches!(part.part_type.as_str(), "text" | "reasoning")
                                });
                                let has_error = info.error.is_some();
                                if has_streamable_parts && !has_error {
                                    continue;
                                }
                            }
                            let mut info_value_for_emit = info_value.clone();
                            let mut content_override: Option<(String, String)> = None;

                            let completed = info.time.as_ref().and_then(|t| t.completed).is_some();
                            if info.role == "assistant" && completed {
                                let missing_content =
                                    info.text.as_ref().map(|t| t.is_empty()).unwrap_or(true)
                                        && info.parts.is_empty();
                                if missing_content {
                                    match client.get_message(&session_id, &info.id).await {
                                        Ok((full_info, parts, raw_info)) => {
                                            info = full_info;
                                            info_value_for_emit = raw_info;
                                            if !parts.is_empty() {
                                                content_override = Some(
                                                    Self::extract_message_parts_from_parts(&parts),
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            tracing::debug!(
                                                session_id,
                                                message_id = %info.id,
                                                error = %err,
                                                "OpenCode get message failed"
                                            );
                                        }
                                    }
                                }
                            }

                            Self::emit_message_from_info(
                                &session_id,
                                &info,
                                Some(&info_value_for_emit),
                                content_override,
                                &event_tx,
                                &shared_state,
                            )
                            .await;
                        }
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

                            if Self::should_ignore_part(&part, &state) {
                                continue;
                            }

                            match part.part_type.as_str() {
                                "text" => {
                                    Self::handle_text_part(&part, delta, &event_tx, &shared_state)
                                        .await;
                                }
                                "reasoning" => {
                                    Self::handle_reasoning_part(
                                        &part,
                                        delta,
                                        &event_tx,
                                        &shared_state,
                                    )
                                    .await;
                                }
                                "tool" => {
                                    if part.tool.as_deref() == Some("question") {
                                        continue;
                                    }
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
                                    let tool_name = tool_name.unwrap_or_else(|| "Tool".to_string());
                                    let tool_name =
                                        MessageDisplay::tool_display_name_owned(&tool_name);
                                    if let Some(state_info) = part.state {
                                        match state_info.status.as_deref() {
                                            Some("pending") | Some("running") => {
                                                if state.started_tools.insert(tool_id.clone()) {
                                                    let arguments = state_info
                                                        .input
                                                        .clone()
                                                        .unwrap_or(Value::Null);
                                                    if !send_event_or_log(
                                                        &event_tx,
                                                        AgentEvent::ToolStarted(ToolStartedEvent {
                                                            tool_name,
                                                            tool_id,
                                                            arguments,
                                                        }),
                                                        "opencode_tool_started",
                                                    )
                                                    .await
                                                    {
                                                        return;
                                                    }
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
                                                if !send_event_or_log(
                                                    &event_tx,
                                                    AgentEvent::ToolCompleted(ToolCompletedEvent {
                                                        tool_id: tool_id.clone(),
                                                        success: true,
                                                        result,
                                                        error: None,
                                                    }),
                                                    "opencode_tool_completed",
                                                )
                                                .await
                                                {
                                                    return;
                                                }
                                                state.started_tools.remove(&tool_id);
                                            }
                                            Some("error") => {
                                                let error =
                                                    Self::extract_tool_error_message(&state_info);
                                                if !send_event_or_log(
                                                    &event_tx,
                                                    AgentEvent::ToolCompleted(ToolCompletedEvent {
                                                        tool_id: tool_id.clone(),
                                                        success: false,
                                                        result: None,
                                                        error,
                                                    }),
                                                    "opencode_tool_completed",
                                                )
                                                .await
                                                {
                                                    return;
                                                }
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
                                tracing::debug!(session_id, "OpenCode session idle");
                                if shared_state.take_turn_in_flight()
                                    && !send_event_or_log(
                                        &event_tx,
                                        AgentEvent::TurnCompleted(TurnCompletedEvent {
                                            usage: Default::default(),
                                        }),
                                        "opencode_turn_completed",
                                    )
                                    .await
                                {
                                    return;
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
                                tracing::debug!(
                                    session_id,
                                    error = %message,
                                    "OpenCode session error event"
                                );
                                if !send_event_or_log(
                                    &event_tx,
                                    AgentEvent::TurnFailed(TurnFailedEvent { error: message }),
                                    "opencode_turn_failed",
                                )
                                .await
                                {
                                    return;
                                }
                            }
                        }
                        "permission.asked" => {
                            let permission: PermissionEvent =
                                match serde_json::from_value(properties.clone()) {
                                    Ok(permission) => permission,
                                    Err(_) => continue,
                                };
                            if permission.session_id == session_id {
                                tracing::debug!(
                                    session_id,
                                    permission_id = %permission.id,
                                    "OpenCode permission asked"
                                );
                                if let Err(err) = client
                                    .respond_permission(&session_id, &permission.id, "once")
                                    .await
                                {
                                    if !send_event_or_log(
                                        &event_tx,
                                        AgentEvent::Error(ErrorEvent {
                                            message: format!(
                                                "Failed to respond to OpenCode permission: {err}"
                                            ),
                                            is_fatal: false,
                                            code: None,
                                            details: None,
                                        }),
                                        "opencode_permission_error",
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                        "question.asked" => {
                            let request: QuestionRequest =
                                match serde_json::from_value(properties.clone()) {
                                    Ok(request) => request,
                                    Err(_) => continue,
                                };
                            if request.session_id != session_id {
                                continue;
                            }
                            tracing::debug!(
                                session_id,
                                request_id = %request.id,
                                question_count = request.questions.len(),
                                "OpenCode question asked"
                            );

                            let questions: Vec<UserQuestion> = request
                                .questions
                                .into_iter()
                                .map(|question| UserQuestion {
                                    header: question.header,
                                    question: question.question,
                                    options: question
                                        .options
                                        .into_iter()
                                        .map(|option| QuestionOption {
                                            label: option.label,
                                            description: option.description,
                                        })
                                        .collect(),
                                    multi_select: question.multiple,
                                })
                                .collect();

                            let arguments = serde_json::json!({ "questions": questions });
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::ToolStarted(ToolStartedEvent {
                                    tool_name: "AskUserQuestion".to_string(),
                                    tool_id: request.id,
                                    arguments,
                                }),
                                "opencode_question_tool_started",
                            )
                            .await
                            {
                                return;
                            }
                        }
                        "question.replied" => {
                            let reply: QuestionResponseEvent =
                                match serde_json::from_value(properties.clone()) {
                                    Ok(reply) => reply,
                                    Err(_) => continue,
                                };
                            if reply.session_id != session_id {
                                continue;
                            }
                            tracing::debug!(
                                session_id,
                                request_id = %reply.request_id,
                                "OpenCode question replied"
                            );
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::ToolCompleted(ToolCompletedEvent {
                                    tool_id: reply.request_id,
                                    success: true,
                                    result: Some("Question answered".to_string()),
                                    error: None,
                                }),
                                "opencode_question_tool_completed",
                            )
                            .await
                            {
                                return;
                            }
                        }
                        "question.rejected" => {
                            let reply: QuestionResponseEvent =
                                match serde_json::from_value(properties.clone()) {
                                    Ok(reply) => reply,
                                    Err(_) => continue,
                                };
                            if reply.session_id != session_id {
                                continue;
                            }
                            tracing::debug!(
                                session_id,
                                request_id = %reply.request_id,
                                "OpenCode question rejected"
                            );
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::ToolCompleted(ToolCompletedEvent {
                                    tool_id: reply.request_id,
                                    success: false,
                                    result: None,
                                    error: Some("Question rejected".to_string()),
                                }),
                                "opencode_question_tool_completed",
                            )
                            .await
                            {
                                return;
                            }
                        }
                        _ => {
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::Raw {
                                    data: serde_json::json!({
                                        "type": event_type,
                                        "properties": properties,
                                    }),
                                },
                                "opencode_raw",
                            )
                            .await
                            {
                                return;
                            }
                        }
                    }
                }
                Ok(Event::Open) => {
                    tracing::debug!("OpenCode SSE connected");
                }
                Err(err) => {
                    if !send_event_or_log(
                        &event_tx,
                        AgentEvent::Error(ErrorEvent {
                            message: format!("OpenCode SSE error: {err}"),
                            is_fatal: false,
                            code: None,
                            details: None,
                        }),
                        "opencode_sse_error",
                    )
                    .await
                    {
                        return;
                    }
                    break;
                }
            }
        }
    }

    async fn handle_text_part(
        part: &MessagePart,
        delta: Option<String>,
        event_tx: &mpsc::Sender<AgentEvent>,
        shared_state: &Arc<OpencodeSharedState>,
    ) {
        let part_key = part
            .id
            .as_ref()
            .map(|id| format!("part:{}", id))
            .or_else(|| {
                part.message_id
                    .as_ref()
                    .map(|id| format!("msg:{}:text", id))
            })
            .unwrap_or_else(|| format!("session:{}", part.session_id));
        if let Some(full_text) = part.text.clone() {
            let previous = shared_state.part_text(&part_key).await;
            if let Some(delta_text) = compute_text_delta(previous.as_deref(), &full_text) {
                if !send_event_or_log(
                    event_tx,
                    AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: delta_text,
                        is_final: false,
                    }),
                    "opencode_assistant_delta",
                )
                .await
                {
                    return;
                }
            }
            shared_state
                .set_part_text(part_key.clone(), full_text)
                .await;
        } else if let Some(delta_text) = delta {
            if !delta_text.is_empty() {
                if !send_event_or_log(
                    event_tx,
                    AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: delta_text.clone(),
                        is_final: false,
                    }),
                    "opencode_assistant_delta",
                )
                .await
                {
                    return;
                }
                shared_state
                    .append_part_text(part_key.clone(), &delta_text)
                    .await;
            }
        }
        if part.time.as_ref().and_then(|t| t.end).is_some() {
            shared_state.clear_part_text(&part_key).await;
            if let Some(message_id) = part.message_id.as_deref() {
                shared_state.mark_completed(message_id).await;
            }
            if !send_event_or_log(
                event_tx,
                AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text: String::new(),
                    is_final: true,
                }),
                "opencode_assistant_final",
            )
            .await
            {
                return;
            }
            if shared_state.take_turn_in_flight() {
                let _ = send_event_or_log(
                    event_tx,
                    AgentEvent::TurnCompleted(TurnCompletedEvent {
                        usage: Default::default(),
                    }),
                    "opencode_turn_completed",
                )
                .await;
            }
        }
    }

    async fn handle_reasoning_part(
        part: &MessagePart,
        delta: Option<String>,
        event_tx: &mpsc::Sender<AgentEvent>,
        shared_state: &Arc<OpencodeSharedState>,
    ) {
        let part_key = part
            .id
            .as_ref()
            .map(|id| format!("part:{}", id))
            .or_else(|| {
                part.message_id
                    .as_ref()
                    .map(|id| format!("msg:{}:reasoning", id))
            })
            .unwrap_or_else(|| format!("session:{}", part.session_id));
        if let Some(full_text) = part.text.clone() {
            let previous = shared_state.part_text(&part_key).await;
            if let Some(delta_text) = compute_text_delta(previous.as_deref(), &full_text) {
                if !send_event_or_log(
                    event_tx,
                    AgentEvent::AssistantReasoning(ReasoningEvent { text: delta_text }),
                    "opencode_reasoning_delta",
                )
                .await
                {
                    return;
                }
            }
            shared_state
                .set_part_text(part_key.clone(), full_text)
                .await;
        } else if let Some(delta_text) = delta {
            if !delta_text.is_empty() {
                if !send_event_or_log(
                    event_tx,
                    AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: delta_text.clone(),
                    }),
                    "opencode_reasoning_delta",
                )
                .await
                {
                    return;
                }
                shared_state
                    .append_part_text(part_key.clone(), &delta_text)
                    .await;
            }
        }
        if part.time.as_ref().and_then(|t| t.end).is_some() {
            shared_state.clear_part_text(&part_key).await;
        }
    }
}

fn compute_text_delta(previous: Option<&str>, current: &str) -> Option<String> {
    if current.is_empty() {
        return None;
    }
    match previous {
        None => Some(current.to_string()),
        Some(prev) if prev == current => None,
        Some(prev) if current.starts_with(prev) => Some(current[prev.len()..].to_string()),
        Some(_) => Some(current.to_string()),
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
                                if sender.send(url).is_err() {
                                    tracing::debug!("OpenCode server url receiver dropped");
                                }
                            }
                        } else if !line.trim().is_empty() {
                            tracing::debug!("OpenCode stdout: {}", line.trim());
                        }
                    }
                    Err(err) => {
                        if !send_event_or_log(
                            &event_tx_for_stdout,
                            AgentEvent::Error(ErrorEvent {
                                message: format!("OpenCode stdout error: {err}"),
                                is_fatal: false,
                                code: None,
                                details: None,
                            }),
                            "opencode_stdout_error",
                        )
                        .await
                        {
                            return;
                        }
                        break;
                    }
                }
            }
        });

        if let Some(stderr) = stderr {
            let event_tx_for_stderr = event_tx.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                let mut capturing_model_error = false;
                let mut provider_id: Option<String> = None;
                let mut model_id: Option<String> = None;
                let mut suggestions: Vec<String> = Vec::new();
                let mut line_count = 0usize;

                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            tracing::debug!("OpenCode stderr: {}", trimmed);

                            if trimmed.contains("ProviderModelNotFoundError")
                                || trimmed.contains("ModelNotFoundError")
                            {
                                capturing_model_error = true;
                                provider_id = None;
                                model_id = None;
                                suggestions.clear();
                                line_count = 0;
                                tracing::warn!(
                                    "OpenCode model lookup failed; collecting error details"
                                );
                                continue;
                            }

                            if capturing_model_error {
                                line_count += 1;
                                if trimmed.contains("providerID:") {
                                    if let Some(value) = trimmed.split('"').nth(1) {
                                        provider_id = Some(value.to_string());
                                    }
                                }
                                if trimmed.contains("modelID:") {
                                    if let Some(value) = trimmed.split('"').nth(1) {
                                        model_id = Some(value.to_string());
                                    }
                                }
                                if trimmed.contains("suggestions:") {
                                    if let Some(list) = trimmed.split('[').nth(1) {
                                        if let Some(list) = list.split(']').next() {
                                            for suggestion in list.split(',') {
                                                let suggestion =
                                                    suggestion.trim().trim_matches('"');
                                                if !suggestion.is_empty() {
                                                    suggestions.push(suggestion.to_string());
                                                }
                                            }
                                        }
                                    }
                                }

                                if trimmed.contains('}') || line_count > 12 {
                                    invalidate_model_cache();
                                    if let (Some(provider), Some(model)) =
                                        (provider_id.as_ref(), model_id.as_ref())
                                    {
                                        let model_id = format!("{}/{}", provider, model);
                                        ModelRegistry::drop_opencode_model(&model_id);
                                    }
                                    let message = match (provider_id.as_ref(), model_id.as_ref()) {
                                        (Some(provider), Some(model)) => {
                                            if suggestions.is_empty() {
                                                format!(
                                                    "OpenCode model not found: {provider}/{model}"
                                                )
                                            } else {
                                                format!(
                                                    "OpenCode model not found: {provider}/{model} (suggestions: {})",
                                                    suggestions.join(", ")
                                                )
                                            }
                                        }
                                        _ => "OpenCode model not found.".to_string(),
                                    };
                                    let details = serde_json::json!({
                                        "provider": provider_id,
                                        "model": model_id,
                                        "suggestions": suggestions,
                                    });
                                    if !send_event_or_log(
                                        &event_tx_for_stderr,
                                        AgentEvent::Error(ErrorEvent {
                                            message,
                                            is_fatal: true,
                                            code: Some("model_not_found".to_string()),
                                            details: Some(details),
                                        }),
                                        "opencode_model_not_found",
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                    capturing_model_error = false;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let ready_timeout_ms = OPENCODE_READY_TIMEOUT.as_millis() as u64;
        let start_error_event = || {
            AgentEvent::Error(ErrorEvent {
                message: format!("OpenCode server startup timed out after {ready_timeout_ms}ms"),
                is_fatal: true,
                code: Some("opencode_start_timeout".to_string()),
                details: None,
            })
        };

        let base_url = match timeout(OPENCODE_READY_TIMEOUT, url_rx).await {
            Ok(Ok(url)) if !url.is_empty() => url,
            Ok(Ok(_)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = send_event_or_log(&event_tx, start_error_event(), "opencode_start_timeout")
                    .await;
                return Err(AgentError::Timeout(ready_timeout_ms));
            }
            Ok(Err(_)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = send_event_or_log(&event_tx, start_error_event(), "opencode_start_timeout")
                    .await;
                return Err(AgentError::Timeout(ready_timeout_ms));
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = send_event_or_log(&event_tx, start_error_event(), "opencode_start_timeout")
                    .await;
                return Err(AgentError::Timeout(ready_timeout_ms));
            }
        };

        let event_tx_for_wait = event_tx.clone();
        tokio::spawn(async move {
            if let Ok(status) = child.wait().await {
                if !status.success() {
                    tracing::warn!(
                        status = ?status,
                        code = ?status.code(),
                        "OpenCode server exited"
                    );
                    let _ = send_event_or_log(
                        &event_tx_for_wait,
                        AgentEvent::Error(ErrorEvent {
                            message: format!(
                                "OpenCode server exited with status {:?}",
                                status.code()
                            ),
                            is_fatal: true,
                            code: None,
                            details: None,
                        }),
                        "opencode_server_exit",
                    )
                    .await;
                }
            }
        });

        tracing::debug!(base_url = %base_url, "OpenCode server ready");
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

        tracing::debug!(session_id = %session_id, "OpenCode session ready");
        let session_id = SessionId::from_string(session_id.clone());
        event_tx
            .send(AgentEvent::SessionInit(SessionInitEvent {
                session_id: session_id.clone(),
                model: config.model.clone(),
            }))
            .await
            .map_err(|_| AgentError::ChannelClosed)?;

        let model_ref = config.model.as_deref().and_then(ModelRef::parse);
        let shared_state = Arc::new(OpencodeSharedState::default());
        let spawn_event_stream =
            |client: OpenCodeClient,
             session_id: String,
             event_tx: mpsc::Sender<AgentEvent>,
             shared_state: Arc<OpencodeSharedState>| {
                if !shared_state.try_mark_sse_active() {
                    return;
                }
                tokio::spawn(async move {
                    OpencodeRunner::handle_events(client, session_id, event_tx, shared_state).await
                });
            };
        let input_tx = {
            let (input_tx, mut input_rx) = mpsc::channel::<AgentInput>(16);
            let client = client.clone();
            let event_tx = event_tx.clone();
            let session_id = session_id.as_str().to_string();
            let model_ref = model_ref.clone();
            let shared_state = shared_state.clone();
            tokio::spawn(async move {
                while let Some(input) = input_rx.recv().await {
                    spawn_event_stream(
                        client.clone(),
                        session_id.clone(),
                        event_tx.clone(),
                        shared_state.clone(),
                    );
                    match input {
                        AgentInput::CodexPrompt {
                            text,
                            images,
                            model,
                        } => {
                            if !images.is_empty()
                                && !send_event_or_log(
                                    &event_tx,
                                    AgentEvent::Error(ErrorEvent {
                                        message: "OpenCode runner does not support image attachments yet.".to_string(),
                                        is_fatal: false,
                                        code: None,
                                        details: None,
                                    }),
                                    "opencode_images_unsupported",
                                )
                                .await
                            {
                                return;
                            }
                            let model_ref_for_input = model
                                .as_deref()
                                .and_then(ModelRef::parse)
                                .or_else(|| model_ref.clone());
                            OpencodeRunner::send_prompt(
                                &client,
                                &session_id,
                                model_ref_for_input.as_ref(),
                                text,
                                &event_tx,
                                &shared_state,
                            )
                            .await;
                        }
                        AgentInput::ClaudeJsonl(_) => {
                            if !send_event_or_log(
                                &event_tx,
                                AgentEvent::Error(ErrorEvent {
                                    message: "OpenCode runner does not support Claude JSONL input."
                                        .to_string(),
                                    is_fatal: false,
                                    code: None,
                                    details: None,
                                }),
                                "opencode_claude_jsonl_unsupported",
                            )
                            .await
                            {
                                return;
                            }
                        }
                        AgentInput::OpencodeQuestion {
                            request_id,
                            answers,
                        } => {
                            let result = match answers {
                                Some(answers) => client.reply_question(&request_id, answers).await,
                                None => client.reject_question(&request_id).await,
                            };
                            if let Err(err) = result {
                                if !send_event_or_log(
                                    &event_tx,
                                    AgentEvent::Error(ErrorEvent {
                                        message: format!(
                                            "OpenCode question response failed: {err}"
                                        ),
                                        is_fatal: false,
                                        code: None,
                                        details: None,
                                    }),
                                    "opencode_question_response_failed",
                                )
                                .await
                                {
                                    return;
                                }
                            }
                        }
                    }
                }
            });
            input_tx
        };

        spawn_event_stream(
            client.clone(),
            session_id.as_str().to_string(),
            event_tx.clone(),
            shared_state.clone(),
        );

        if !config.prompt.trim().is_empty() {
            OpencodeRunner::send_prompt(
                &client,
                session_id.as_str(),
                model_ref.as_ref(),
                config.prompt.clone(),
                &event_tx,
                &shared_state,
            )
            .await;
        }

        let mut handle = AgentHandle::new(event_rx, pid, Some(input_tx));
        handle.set_session_id(session_id);
        Ok(handle)
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
                let err = std::io::Error::last_os_error();
                tracing::error!(pid = handle.pid, error = %err, "OpenCode SIGTERM failed");
                return Err(AgentError::Io(err));
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
                let err = std::io::Error::last_os_error();
                tracing::error!(pid = handle.pid, error = %err, "OpenCode SIGKILL failed");
                return Err(AgentError::Io(err));
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

fn invalidate_model_cache() {
    if let Some(path) = cache_path() {
        match fs::remove_file(&path) {
            Ok(()) => {
                tracing::info!("OpenCode model cache invalidated");
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::debug!(error = %err, "Failed to remove OpenCode model cache");
            }
        }
    }
}

fn extract_model_id_from_error(error: &str) -> Option<String> {
    for prefix in ["Model ", "model "] {
        if let Some(rest) = error.split(prefix).nth(1) {
            if let Some(candidate) = rest.split_whitespace().next() {
                let trimmed = candidate.trim_matches(|c: char| c == '"' || c == '\'');
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::{
        compute_text_delta, MessagePart, OpencodeEventState, OpencodeRunner, OpencodeSharedState,
        ToolState,
    };
    use crate::agent::events::AgentEvent;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::timeout;

    #[test]
    fn test_compute_text_delta_none_prev() {
        assert_eq!(compute_text_delta(None, "hello"), Some("hello".to_string()));
    }

    #[test]
    fn test_compute_text_delta_same() {
        assert_eq!(compute_text_delta(Some("hello"), "hello"), None);
    }

    #[test]
    fn test_compute_text_delta_prefix() {
        assert_eq!(
            compute_text_delta(Some("hel"), "hello"),
            Some("lo".to_string())
        );
    }

    #[test]
    fn test_compute_text_delta_non_prefix() {
        assert_eq!(
            compute_text_delta(Some("world"), "hello"),
            Some("hello".to_string())
        );
    }

    #[tokio::test]
    async fn test_opencode_text_part_dedupes_full_updates() {
        let shared_state = Arc::new(OpencodeSharedState::default());
        let (event_tx, mut event_rx) = mpsc::channel(8);
        let mut part = MessagePart {
            id: Some("prt-1".to_string()),
            session_id: "session-1".to_string(),
            message_id: Some("msg-1".to_string()),
            part_type: "text".to_string(),
            call_id: None,
            text: Some("Hello".to_string()),
            tool: None,
            state: None,
            time: None,
        };

        OpencodeRunner::handle_text_part(&part, None, &event_tx, &shared_state).await;
        part.text = Some("Hello world".to_string());
        OpencodeRunner::handle_text_part(&part, None, &event_tx, &shared_state).await;

        let first = timeout(Duration::from_millis(50), event_rx.recv())
            .await
            .expect("first event")
            .expect("first event value");
        let second = timeout(Duration::from_millis(50), event_rx.recv())
            .await
            .expect("second event")
            .expect("second event value");

        match first {
            AgentEvent::AssistantMessage(msg) => assert_eq!(msg.text, "Hello"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
        match second {
            AgentEvent::AssistantMessage(msg) => assert_eq!(msg.text, " world"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        }

        assert!(
            timeout(Duration::from_millis(50), event_rx.recv())
                .await
                .is_err(),
            "no extra events expected"
        );
    }

    #[test]
    fn test_opencode_ignores_user_parts() {
        let mut state = OpencodeEventState::default();
        state
            .message_roles
            .insert("msg-user".to_string(), "user".to_string());

        let user_part = MessagePart {
            id: Some("prt-user".to_string()),
            session_id: "session-1".to_string(),
            message_id: Some("msg-user".to_string()),
            part_type: "text".to_string(),
            call_id: None,
            text: Some("hi".to_string()),
            tool: None,
            state: None,
            time: None,
        };

        let assistant_part = MessagePart {
            id: Some("prt-assistant".to_string()),
            session_id: "session-1".to_string(),
            message_id: Some("msg-assistant".to_string()),
            part_type: "text".to_string(),
            call_id: None,
            text: Some("hi".to_string()),
            tool: None,
            state: None,
            time: None,
        };

        assert!(OpencodeRunner::should_ignore_part(&user_part, &state));
        assert!(!OpencodeRunner::should_ignore_part(&assistant_part, &state));
    }
    #[test]
    fn test_opencode_tool_error_message_prefers_error_field() {
        let state = ToolState {
            status: Some("error".to_string()),
            title: None,
            input: None,
            output: Some(serde_json::json!({"message": "ignored"})),
            error: Some("boom".to_string()),
        };

        let message = OpencodeRunner::extract_tool_error_message(&state);
        assert_eq!(message.as_deref(), Some("boom"));
    }

    #[test]
    fn test_opencode_tool_error_message_reads_output_object() {
        let state = ToolState {
            status: Some("error".to_string()),
            title: None,
            input: None,
            output: Some(serde_json::json!({
                "error": {"message": "Invalid input"}
            })),
            error: None,
        };

        let message = OpencodeRunner::extract_tool_error_message(&state);
        assert_eq!(message.as_deref(), Some("Invalid input"));
    }

    #[test]
    fn test_opencode_tool_error_message_reads_output_string() {
        let state = ToolState {
            status: Some("error".to_string()),
            title: None,
            input: None,
            output: Some(serde_json::Value::String("bad args".to_string())),
            error: None,
        };

        let message = OpencodeRunner::extract_tool_error_message(&state);
        assert_eq!(message.as_deref(), Some("bad args"));
    }
}
