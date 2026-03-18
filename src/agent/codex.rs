use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use codex_app_server_protocol::{
    ApplyPatchApprovalResponse, ClientInfo, ClientNotification, ClientRequest,
    ExecCommandApprovalResponse, InitializeParams, JSONRPCError, JSONRPCMessage, JSONRPCResponse,
    McpToolCallStatus, PatchApplyStatus, PatchChangeKind, RequestId, ServerNotification,
    ServerRequest, ThreadItem, ThreadResumeParams, ThreadResumeResponse, ThreadStartParams,
    ThreadStartResponse, TurnStartParams, TurnStartResponse, TurnStatus, UserInput,
};
use codex_protocol::config_types::SandboxMode;
use codex_protocol::protocol::{
    AskForApproval, EventMsg, FileChange, HasLegacyEvent, ReviewDecision,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, CommandOutputEvent, ContextCompactionEvent, ErrorEvent,
    FileChangedEvent, FileOperation, ReasoningEvent, SessionInitEvent, TokenUsage, TokenUsageEvent,
    ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent, TurnFailedEvent,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;

const CODEX_NPX_PACKAGE: &str = "@openai/codex";
const CODEX_NPX_VERSION_ENV: &str = "CODEX_NPX_VERSION";
const CODEX_THREAD_RESUME_TIMEOUT: Duration = Duration::from_secs(5);
const CODEX_THREAD_START_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_TURN_START_TIMEOUT: Duration = Duration::from_secs(15);

/// Notification params containing an EventMsg
#[derive(Debug, Deserialize)]
struct CodexNotificationParams {
    #[serde(rename = "msg")]
    msg: EventMsg,
}

// ============================================================================
// JSON-RPC Peer (bidirectional communication)
// ============================================================================

#[derive(Clone)]
struct JsonRpcPeer {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<io::Result<Value>>>>>,
    id_counter: Arc<AtomicI64>,
}

impl JsonRpcPeer {
    fn new(stdin: ChildStdin) -> Self {
        Self {
            stdin: Arc::new(Mutex::new(stdin)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            id_counter: Arc::new(AtomicI64::new(1)),
        }
    }

    fn next_request_id(&self) -> RequestId {
        RequestId::Integer(self.id_counter.fetch_add(1, Ordering::Relaxed))
    }

    async fn send<T: Serialize>(&self, message: &T) -> io::Result<()> {
        let raw = serde_json::to_string(message)?;
        let mut guard = self.stdin.lock().await;
        guard.write_all(raw.as_bytes()).await?;
        guard.write_all(b"\n").await?;
        guard.flush().await
    }

    async fn request<R: DeserializeOwned>(&self, request: &ClientRequest) -> io::Result<R> {
        let request_id = match request {
            ClientRequest::Initialize { request_id, .. }
            | ClientRequest::ThreadStart { request_id, .. }
            | ClientRequest::ThreadResume { request_id, .. }
            | ClientRequest::TurnStart { request_id, .. } => request_id.clone(),
            _ => return Err(io::Error::other("unsupported request type")),
        };

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id.clone(), tx);
        if let Err(err) = self.send(request).await {
            self.pending.lock().await.remove(&request_id);
            return Err(err);
        }

        let value = rx
            .await
            .map_err(|_| io::Error::other("response dropped"))??;
        serde_json::from_value(value).map_err(|e| io::Error::other(e.to_string()))
    }

    async fn resolve(&self, request_id: RequestId, value: Value) {
        if let Some(tx) = self.pending.lock().await.remove(&request_id) {
            if tx.send(Ok(value)).is_err() {
                tracing::debug!("Dropping JSON-RPC response; receiver already closed");
            }
        }
    }

    async fn reject(&self, error: JSONRPCError) {
        if let Some(tx) = self.pending.lock().await.remove(&error.id) {
            let message = jsonrpc_error_message(&error);
            if tx.send(Err(io::Error::other(message))).is_err() {
                tracing::debug!("Dropping JSON-RPC response; receiver already closed");
            }
        }
    }
}

fn jsonrpc_error_message(error: &JSONRPCError) -> String {
    let mut message = format!("[Error {}] {}", error.error.code, error.error.message);
    if let Some(data) = error.error.data.as_ref() {
        let rendered = match data {
            Value::String(text) => text.clone(),
            _ => serde_json::to_string(data).unwrap_or_else(|_| "<unrenderable data>".to_string()),
        };
        if !rendered.is_empty() {
            message.push_str(": ");
            message.push_str(&rendered);
        }
    }
    message
}

#[derive(Default)]
struct CodexEventState {
    exec_command_by_id: HashMap<String, String>,
    exec_output_by_id: HashMap<String, String>,
    last_usage: Option<TokenUsage>,
    last_total_tokens: Option<i64>,
    pending_compaction: bool,
    message_stream_source: Option<MessageStreamSource>,
    reasoning_stream_source: Option<ReasoningStreamSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageStreamSource {
    Legacy,
    Content,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReasoningStreamSource {
    Legacy,
    Summary,
    Raw,
}

// ============================================================================
// Codex app-server runner
// ============================================================================

pub struct CodexCliRunner {
    binary_path: PathBuf,
}

impl CodexCliRunner {
    pub fn new() -> Self {
        Self {
            binary_path: Self::find_binary().unwrap_or_else(|| PathBuf::from("codex")),
        }
    }

    /// Create a runner with a specific binary path
    pub fn with_path(path: PathBuf) -> Self {
        Self { binary_path: path }
    }

    fn find_binary() -> Option<PathBuf> {
        which::which("codex").ok()
    }

    fn npx_package() -> String {
        if let Ok(version) = std::env::var(CODEX_NPX_VERSION_ENV) {
            format!("{CODEX_NPX_PACKAGE}@{version}")
        } else {
            CODEX_NPX_PACKAGE.to_string()
        }
    }

    fn approval_policy() -> AskForApproval {
        match std::env::var("CODEX_APPROVAL_POLICY")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "untrusted" => AskForApproval::UnlessTrusted,
            "on-failure" => AskForApproval::OnFailure,
            "on-request" => AskForApproval::OnRequest,
            _ => AskForApproval::Never,
        }
    }

    fn sandbox_mode() -> SandboxMode {
        match std::env::var("CODEX_SANDBOX_MODE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "read-only" => SandboxMode::ReadOnly,
            "workspace-write" => SandboxMode::WorkspaceWrite,
            _ => SandboxMode::DangerFullAccess,
        }
    }

    fn build_codex_command(&self, cwd: &Path) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("app-server");
        cmd.current_dir(cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("NODE_NO_WARNINGS", "1");
        cmd.env("NO_COLOR", "1");
        cmd
    }

    fn build_npx_command(&self, cwd: &Path) -> Command {
        let mut cmd = Command::new("npx");
        cmd.args(["-y", &Self::npx_package(), "app-server"]);
        cmd.current_dir(cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("NODE_NO_WARNINGS", "1");
        cmd.env("NO_COLOR", "1");
        cmd
    }

    fn build_user_inputs(
        prompt: &str,
        images: &[PathBuf],
        skill: Option<&crate::command_resolver::SkillReference>,
    ) -> Vec<UserInput> {
        let mut items = Vec::new();
        if let Some(skill) = skill {
            items.push(UserInput::Skill {
                name: skill.name.clone(),
                path: skill.path.clone(),
            });
        }
        if !prompt.trim().is_empty() {
            items.push(UserInput::Text {
                text: prompt.to_string(),
            });
        }
        for image in images {
            items.push(UserInput::LocalImage {
                path: image.clone(),
            });
        }
        items
    }

    fn to_file_operation(change: &FileChange) -> FileOperation {
        match change {
            FileChange::Add { .. } => FileOperation::Create,
            FileChange::Delete { .. } => FileOperation::Delete,
            FileChange::Update { .. } => FileOperation::Update,
        }
    }

    fn conversation_config(
        config: &AgentStartConfig,
    ) -> Option<HashMap<String, serde_json::Value>> {
        let effort = config.reasoning_effort?;
        let mut values = HashMap::new();
        values.insert(
            "model_reasoning_effort".to_string(),
            serde_json::Value::String(effort.codex_config_value().to_string()),
        );
        Some(values)
    }

    fn update_token_usage(
        state: &mut CodexEventState,
        usage: TokenUsage,
        context_window: Option<i64>,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let usage_percent = context_window.and_then(|window| {
            if window > 0 {
                Some((usage.total_tokens as f32 / window as f32) * 100.0)
            } else {
                None
            }
        });
        let previous_total = state.last_total_tokens;
        state.last_total_tokens = Some(usage.total_tokens);
        state.last_usage = Some(usage.clone());

        if let Some(prev) = previous_total {
            if state.pending_compaction && prev > 0 {
                events.push(AgentEvent::ContextCompaction(ContextCompactionEvent {
                    reason: "context_compacted".to_string(),
                    tokens_before: prev,
                    tokens_after: usage.total_tokens,
                }));
                state.pending_compaction = false;
            } else if usage.total_tokens < prev {
                events.push(AgentEvent::ContextCompaction(ContextCompactionEvent {
                    reason: "token_count_drop".to_string(),
                    tokens_before: prev,
                    tokens_after: usage.total_tokens,
                }));
            }
        }

        events.push(AgentEvent::TokenUsage(TokenUsageEvent {
            usage,
            context_window,
            usage_percent,
        }));
        events
    }

    fn convert_v2_item_started(item: &ThreadItem, state: &mut CodexEventState) -> Vec<AgentEvent> {
        match item {
            ThreadItem::CommandExecution { id, command, .. } => {
                state.exec_command_by_id.insert(id.clone(), command.clone());
                state.exec_output_by_id.insert(id.clone(), String::new());
                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "Bash".to_string(),
                    tool_id: id.clone(),
                    arguments: serde_json::json!({ "command": command }),
                })]
            }
            ThreadItem::FileChange { id, changes, .. } => {
                let files: Vec<String> = changes.iter().map(|change| change.path.clone()).collect();
                let mut events = vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "ApplyPatch".to_string(),
                    tool_id: id.clone(),
                    arguments: serde_json::json!({ "files": files }),
                })];
                for change in changes {
                    let operation = match &change.kind {
                        PatchChangeKind::Add => FileOperation::Create,
                        PatchChangeKind::Delete => FileOperation::Delete,
                        PatchChangeKind::Update { .. } => FileOperation::Update,
                    };
                    events.push(AgentEvent::FileChanged(FileChangedEvent {
                        path: change.path.clone(),
                        operation,
                    }));
                }
                events
            }
            ThreadItem::McpToolCall {
                id,
                server,
                tool,
                arguments,
                ..
            } => vec![AgentEvent::ToolStarted(ToolStartedEvent {
                tool_name: format!("mcp:{server}::{tool}"),
                tool_id: id.clone(),
                arguments: arguments.clone(),
            })],
            ThreadItem::WebSearch { id, .. } => {
                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "WebSearch".to_string(),
                    tool_id: id.clone(),
                    arguments: Value::Null,
                })]
            }
            ThreadItem::ImageView { id, path } => {
                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "ViewImage".to_string(),
                    tool_id: id.clone(),
                    arguments: serde_json::json!({ "path": path }),
                })]
            }
            _ => Vec::new(),
        }
    }

    fn convert_v2_item_completed(
        item: &ThreadItem,
        state: &mut CodexEventState,
    ) -> Vec<AgentEvent> {
        match item {
            ThreadItem::AgentMessage { text, .. } => {
                let had_stream = state.message_stream_source.is_some();
                state.message_stream_source = None;
                if had_stream {
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: String::new(),
                        is_final: true,
                    })]
                } else {
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: text.clone(),
                        is_final: true,
                    })]
                }
            }
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                if state.reasoning_stream_source.is_some() {
                    state.reasoning_stream_source = None;
                    Vec::new()
                } else {
                    let text = if !summary.is_empty() {
                        summary.join("")
                    } else {
                        content.join("")
                    };
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![AgentEvent::AssistantReasoning(ReasoningEvent { text })]
                    }
                }
            }
            ThreadItem::CommandExecution {
                id,
                command,
                aggregated_output,
                exit_code,
                ..
            } => {
                state.exec_output_by_id.remove(id);
                state.exec_command_by_id.remove(id);
                vec![AgentEvent::CommandOutput(CommandOutputEvent {
                    command: command.clone(),
                    output: aggregated_output.clone().unwrap_or_default(),
                    exit_code: *exit_code,
                    is_streaming: false,
                })]
            }
            ThreadItem::FileChange { id, status, .. } => {
                let success = matches!(status, PatchApplyStatus::Completed);
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: id.clone(),
                    success,
                    result: None,
                    error: if success {
                        None
                    } else {
                        Some("Patch application did not complete successfully".to_string())
                    },
                })]
            }
            ThreadItem::McpToolCall {
                id,
                status,
                result,
                error,
                ..
            } => {
                let (success, rendered_result, rendered_error) = match status {
                    McpToolCallStatus::Completed => (
                        true,
                        result
                            .as_ref()
                            .and_then(|value| serde_json::to_string(value).ok()),
                        None,
                    ),
                    _ => (
                        false,
                        None,
                        error
                            .as_ref()
                            .map(|value| value.message.clone())
                            .or_else(|| {
                                Some("MCP tool call did not complete successfully".to_string())
                            }),
                    ),
                };
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: id.clone(),
                    success,
                    result: rendered_result,
                    error: rendered_error,
                })]
            }
            ThreadItem::WebSearch { id, query } => {
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: id.clone(),
                    success: true,
                    result: Some(format!("Query: {query}")),
                    error: None,
                })]
            }
            ThreadItem::ImageView { id, path } => {
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: id.clone(),
                    success: true,
                    result: Some(path.clone()),
                    error: None,
                })]
            }
            _ => Vec::new(),
        }
    }

    fn convert_server_notification(
        notification: &ServerNotification,
        state: &mut CodexEventState,
    ) -> Vec<AgentEvent> {
        match notification {
            ServerNotification::ThreadTokenUsageUpdated(payload) => {
                let total = &payload.token_usage.total;
                Self::update_token_usage(
                    state,
                    TokenUsage {
                        input_tokens: total.input_tokens,
                        output_tokens: total.output_tokens,
                        cached_tokens: total.cached_input_tokens,
                        total_tokens: total.total_tokens,
                    },
                    payload.token_usage.model_context_window,
                )
            }
            ServerNotification::TurnStarted(_) => {
                state.message_stream_source = None;
                state.reasoning_stream_source = None;
                vec![AgentEvent::TurnStarted]
            }
            ServerNotification::TurnCompleted(payload) => {
                state.message_stream_source = None;
                state.reasoning_stream_source = None;
                match payload.turn.status {
                    TurnStatus::Completed => {
                        let usage = state.last_usage.take().unwrap_or_default();
                        vec![AgentEvent::TurnCompleted(TurnCompletedEvent { usage })]
                    }
                    TurnStatus::Interrupted => {
                        vec![AgentEvent::TurnFailed(TurnFailedEvent {
                            error: "Turn interrupted".to_string(),
                        })]
                    }
                    TurnStatus::Failed => {
                        vec![AgentEvent::TurnFailed(TurnFailedEvent {
                            error: payload
                                .turn
                                .error
                                .as_ref()
                                .map(|error| error.message.clone())
                                .unwrap_or_else(|| "Turn failed".to_string()),
                        })]
                    }
                    TurnStatus::InProgress => Vec::new(),
                }
            }
            ServerNotification::ItemStarted(payload) => {
                Self::convert_v2_item_started(&payload.item, state)
            }
            ServerNotification::ItemCompleted(payload) => {
                Self::convert_v2_item_completed(&payload.item, state)
            }
            ServerNotification::AgentMessageDelta(delta) => match state.message_stream_source {
                None | Some(MessageStreamSource::V2) => {
                    state.message_stream_source = Some(MessageStreamSource::V2);
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: delta.delta.clone(),
                        is_final: false,
                    })]
                }
                _ => Vec::new(),
            },
            ServerNotification::CommandExecutionOutputDelta(delta) => {
                let entry = state
                    .exec_output_by_id
                    .entry(delta.item_id.clone())
                    .or_default();
                entry.push_str(&delta.delta);
                let command = state
                    .exec_command_by_id
                    .get(&delta.item_id)
                    .cloned()
                    .unwrap_or_default();
                vec![AgentEvent::CommandOutput(CommandOutputEvent {
                    command,
                    output: entry.clone(),
                    exit_code: None,
                    is_streaming: true,
                })]
            }
            ServerNotification::ReasoningSummaryTextDelta(delta) => {
                match state.reasoning_stream_source {
                    None | Some(ReasoningStreamSource::Summary) => {
                        state.reasoning_stream_source = Some(ReasoningStreamSource::Summary);
                        vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                            text: delta.delta.clone(),
                        })]
                    }
                    _ => Vec::new(),
                }
            }
            ServerNotification::ReasoningTextDelta(delta) => match state.reasoning_stream_source {
                None | Some(ReasoningStreamSource::Raw) => {
                    state.reasoning_stream_source = Some(ReasoningStreamSource::Raw);
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: delta.delta.clone(),
                    })]
                }
                _ => Vec::new(),
            },
            ServerNotification::ContextCompacted(_) => {
                state.pending_compaction = true;
                Vec::new()
            }
            ServerNotification::Error(payload) => vec![AgentEvent::Error(ErrorEvent {
                message: payload.error.message.clone(),
                is_fatal: !payload.will_retry,
                code: None,
                details: None,
            })],
            _ => Vec::new(),
        }
    }

    fn convert_event(event: &EventMsg, state: &mut CodexEventState) -> Vec<AgentEvent> {
        match event {
            EventMsg::ItemStarted(item) => item
                .as_legacy_events(false)
                .into_iter()
                .flat_map(|legacy| Self::convert_event(&legacy, state))
                .collect(),
            EventMsg::ItemCompleted(item) => item
                .as_legacy_events(false)
                .into_iter()
                .flat_map(|legacy| Self::convert_event(&legacy, state))
                .collect(),
            EventMsg::TurnStarted(_) => {
                state.message_stream_source = None;
                state.reasoning_stream_source = None;
                vec![AgentEvent::TurnStarted]
            }
            EventMsg::TurnComplete(_) => {
                let usage = state.last_usage.take().unwrap_or_default();
                state.message_stream_source = None;
                state.reasoning_stream_source = None;
                vec![AgentEvent::TurnCompleted(TurnCompletedEvent { usage })]
            }
            EventMsg::TurnAborted(ev) => vec![AgentEvent::TurnFailed(TurnFailedEvent {
                error: format!("Turn aborted: {:?}", ev.reason),
            })],
            EventMsg::AgentMessageDelta(msg) => match state.message_stream_source {
                None | Some(MessageStreamSource::Legacy) => {
                    state.message_stream_source = Some(MessageStreamSource::Legacy);
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: msg.delta.clone(),
                        is_final: false,
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::AgentMessage(msg) => {
                let had_stream = state.message_stream_source.is_some();
                state.message_stream_source = None;
                if had_stream {
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: String::new(),
                        is_final: true,
                    })]
                } else {
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: msg.message.clone(),
                        is_final: true,
                    })]
                }
            }
            EventMsg::AgentMessageContentDelta(msg) => match state.message_stream_source {
                None | Some(MessageStreamSource::Content) => {
                    state.message_stream_source = Some(MessageStreamSource::Content);
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: msg.delta.clone(),
                        is_final: false,
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::AgentReasoningDelta(r) => match state.reasoning_stream_source {
                None | Some(ReasoningStreamSource::Legacy) => {
                    state.reasoning_stream_source = Some(ReasoningStreamSource::Legacy);
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.delta.clone(),
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::AgentReasoning(r) => {
                if state.reasoning_stream_source.is_some() {
                    state.reasoning_stream_source = None;
                    Vec::new()
                } else {
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.text.clone(),
                    })]
                }
            }
            EventMsg::AgentReasoningRawContent(r) => {
                if state.reasoning_stream_source.is_some() {
                    state.reasoning_stream_source = None;
                    Vec::new()
                } else {
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.text.clone(),
                    })]
                }
            }
            EventMsg::AgentReasoningRawContentDelta(r) => match state.reasoning_stream_source {
                None | Some(ReasoningStreamSource::Legacy) => {
                    state.reasoning_stream_source = Some(ReasoningStreamSource::Legacy);
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.delta.clone(),
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::ReasoningContentDelta(r) => match state.reasoning_stream_source {
                None | Some(ReasoningStreamSource::Summary) => {
                    state.reasoning_stream_source = Some(ReasoningStreamSource::Summary);
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.delta.clone(),
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::ReasoningRawContentDelta(r) => match state.reasoning_stream_source {
                None | Some(ReasoningStreamSource::Raw) => {
                    state.reasoning_stream_source = Some(ReasoningStreamSource::Raw);
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: r.delta.clone(),
                    })]
                }
                _ => Vec::new(),
            },
            EventMsg::ExecCommandBegin(cmd) => {
                let command_str = cmd.command.join(" ");
                state
                    .exec_command_by_id
                    .insert(cmd.call_id.clone(), command_str.clone());
                state
                    .exec_output_by_id
                    .insert(cmd.call_id.clone(), String::new());

                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "Bash".to_string(),
                    tool_id: cmd.call_id.clone(),
                    arguments: serde_json::json!({ "command": command_str }),
                })]
            }
            EventMsg::ExecCommandOutputDelta(delta) => {
                let chunk = String::from_utf8_lossy(&delta.chunk).to_string();
                let entry = state
                    .exec_output_by_id
                    .entry(delta.call_id.clone())
                    .or_default();
                entry.push_str(&chunk);
                let command = state
                    .exec_command_by_id
                    .get(&delta.call_id)
                    .cloned()
                    .unwrap_or_default();
                vec![AgentEvent::CommandOutput(CommandOutputEvent {
                    command,
                    output: entry.clone(),
                    exit_code: None,
                    is_streaming: true,
                })]
            }
            EventMsg::ExecCommandEnd(end) => {
                let output = if !end.aggregated_output.is_empty() {
                    end.aggregated_output.clone()
                } else {
                    format!("{}{}", end.stdout, end.stderr)
                };
                let command = end.command.join(" ");
                state.exec_output_by_id.remove(&end.call_id);
                state.exec_command_by_id.remove(&end.call_id);
                vec![AgentEvent::CommandOutput(CommandOutputEvent {
                    command,
                    output,
                    exit_code: Some(end.exit_code),
                    is_streaming: false,
                })]
            }
            EventMsg::McpToolCallBegin(ev) => {
                let tool_name = format!("mcp:{}::{}", ev.invocation.server, ev.invocation.tool);
                let args = ev.invocation.arguments.clone().unwrap_or(Value::Null);
                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name,
                    tool_id: ev.call_id.clone(),
                    arguments: args,
                })]
            }
            EventMsg::McpToolCallEnd(ev) => {
                let (success, result, error) = match &ev.result {
                    Ok(result) => {
                        let rendered = serde_json::to_string(result).unwrap_or_default();
                        (!result.is_error.unwrap_or(false), Some(rendered), None)
                    }
                    Err(err) => (false, None, Some(err.clone())),
                };
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: ev.call_id.clone(),
                    success,
                    result,
                    error,
                })]
            }
            EventMsg::WebSearchBegin(ev) => vec![AgentEvent::ToolStarted(ToolStartedEvent {
                tool_name: "WebSearch".to_string(),
                tool_id: ev.call_id.clone(),
                arguments: Value::Null,
            })],
            EventMsg::WebSearchEnd(ev) => vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                tool_id: ev.call_id.clone(),
                success: true,
                result: Some(format!("Query: {}", ev.query)),
                error: None,
            })],
            EventMsg::PatchApplyBegin(ev) => {
                let files: Vec<String> = ev
                    .changes
                    .keys()
                    .map(|path| path.display().to_string())
                    .collect();
                let mut events = vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: "ApplyPatch".to_string(),
                    tool_id: ev.call_id.clone(),
                    arguments: serde_json::json!({ "files": files }),
                })];
                for (path, change) in &ev.changes {
                    events.push(AgentEvent::FileChanged(FileChangedEvent {
                        path: path.display().to_string(),
                        operation: Self::to_file_operation(change),
                    }));
                }
                events
            }
            EventMsg::PatchApplyEnd(ev) => {
                let output = if ev.success {
                    ev.stdout.clone()
                } else {
                    format!("{}{}", ev.stdout, ev.stderr)
                };
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: ev.call_id.clone(),
                    success: ev.success,
                    result: Some(output),
                    error: if ev.success {
                        None
                    } else {
                        Some(ev.stderr.clone())
                    },
                })]
            }
            EventMsg::ViewImageToolCall(ev) => {
                let args = serde_json::json!({ "path": ev.path });
                vec![
                    AgentEvent::ToolStarted(ToolStartedEvent {
                        tool_name: "ViewImage".to_string(),
                        tool_id: ev.call_id.clone(),
                        arguments: args,
                    }),
                    AgentEvent::ToolCompleted(ToolCompletedEvent {
                        tool_id: ev.call_id.clone(),
                        success: true,
                        result: Some(ev.path.display().to_string()),
                        error: None,
                    }),
                ]
            }
            EventMsg::TokenCount(count) => {
                if let Some(info) = &count.info {
                    let total = &info.total_token_usage;
                    Self::update_token_usage(
                        state,
                        TokenUsage {
                            input_tokens: total.input_tokens,
                            output_tokens: total.output_tokens,
                            cached_tokens: total.cached_input_tokens,
                            total_tokens: total.total_tokens,
                        },
                        info.model_context_window,
                    )
                } else {
                    Vec::new()
                }
            }
            EventMsg::ContextCompacted(_) => {
                state.pending_compaction = true;
                Vec::new()
            }
            EventMsg::Error(err) => vec![
                AgentEvent::Error(ErrorEvent {
                    message: err.message.clone(),
                    is_fatal: true,
                    code: None,
                    details: None,
                }),
                AgentEvent::TurnFailed(TurnFailedEvent {
                    error: err.message.clone(),
                }),
            ],
            EventMsg::Warning(warn) => vec![AgentEvent::Error(ErrorEvent {
                message: format!("Warning: {}", warn.message),
                is_fatal: false,
                code: None,
                details: None,
            })],
            EventMsg::StreamError(err) => vec![AgentEvent::Error(ErrorEvent {
                message: format!("Stream error: {}", err.message),
                is_fatal: false,
                code: None,
                details: None,
            })],
            _ => serde_json::to_value(event)
                .ok()
                .map(|data| vec![AgentEvent::Raw { data }])
                .unwrap_or_default(),
        }
    }

    async fn send_user_message(
        peer: &JsonRpcPeer,
        thread_id: &str,
        prompt: &str,
        images: &[PathBuf],
        skill: Option<&crate::command_resolver::SkillReference>,
    ) -> io::Result<()> {
        let items = Self::build_user_inputs(prompt, images, skill);
        if items.is_empty() {
            return Ok(());
        }
        tracing::debug!(
            thread_id,
            prompt_len = prompt.len(),
            images = images.len(),
            has_skill = skill.is_some(),
            "Sending Codex turn/start request"
        );
        let request = ClientRequest::TurnStart {
            request_id: peer.next_request_id(),
            params: TurnStartParams {
                thread_id: thread_id.to_string(),
                input: items,
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model: None,
                effort: None,
                summary: None,
                output_schema: None,
            },
        };
        let _: TurnStartResponse =
            tokio::time::timeout(CODEX_TURN_START_TIMEOUT, peer.request(&request))
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "Codex turn start timed out after {}s",
                            CODEX_TURN_START_TIMEOUT.as_secs()
                        ),
                    )
                })??;
        Ok(())
    }

    async fn spawn_app_server(&self, cwd: &Path) -> Result<tokio::process::Child, AgentError> {
        if self.binary_path.exists() {
            let mut cmd = self.build_codex_command(cwd);
            match cmd.spawn() {
                Ok(child) => return Ok(child),
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to spawn codex app-server, falling back to npx");
                }
            }
        }

        let mut cmd = self.build_npx_command(cwd);
        let child = cmd.spawn()?;
        Ok(child)
    }

    async fn cleanup_startup_child(
        child: &mut tokio::process::Child,
        stage: &str,
    ) -> Result<(), AgentError> {
        match child.kill().await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
            Err(error) => {
                tracing::warn!(stage, error = %error, "Failed to kill Codex app-server");
                return Err(AgentError::Io(error));
            }
        }

        if let Err(error) = child.wait().await {
            tracing::warn!(stage, error = %error, "Failed to wait for Codex app-server shutdown");
            return Err(AgentError::Io(error));
        }

        Ok(())
    }
}

#[async_trait]
impl AgentRunner for CodexCliRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        let mut child = self.spawn_app_server(&config.working_dir).await?;
        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentError::Io(io::Error::other("failed to capture stdin")))?;
        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;
        let stderr = child.stderr.take();

        let peer = JsonRpcPeer::new(stdin);

        let (tx, rx) = mpsc::channel::<AgentEvent>(256);
        let tx_for_monitor = tx.clone();
        let tx_for_events = tx.clone();

        // Spawn JSON-RPC read loop
        let reader_peer = peer.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buffer = String::new();
            let mut state = CodexEventState::default();

            loop {
                buffer.clear();
                match reader.read_line(&mut buffer).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = buffer.trim();
                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<JSONRPCMessage>(line) {
                            Ok(JSONRPCMessage::Response(response)) => {
                                reader_peer
                                    .resolve(response.id.clone(), response.result)
                                    .await;
                            }
                            Ok(JSONRPCMessage::Notification(notification)) => {
                                if let Ok(server_notification) =
                                    ServerNotification::try_from(notification.clone())
                                {
                                    tracing::debug!(
                                        notification = ?server_notification,
                                        "Received Codex server notification"
                                    );
                                    let events = Self::convert_server_notification(
                                        &server_notification,
                                        &mut state,
                                    );
                                    for event in events {
                                        if tx_for_events.send(event).await.is_err() {
                                            return;
                                        }
                                    }
                                } else if notification.method.starts_with("codex/event/") {
                                    if let Some(params) = notification.params {
                                        match serde_json::from_value::<CodexNotificationParams>(
                                            params,
                                        ) {
                                            Ok(codex_params) => {
                                                tracing::debug!(
                                                    event = ?codex_params.msg,
                                                    "Received Codex event notification"
                                                );
                                                let events = Self::convert_event(
                                                    &codex_params.msg,
                                                    &mut state,
                                                );
                                                for event in events {
                                                    if tx_for_events.send(event).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tracing::warn!(
                                                    error = %err,
                                                    "Failed to parse codex event notification"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(JSONRPCMessage::Request(request)) => {
                                if let Ok(server_req) = ServerRequest::try_from(request) {
                                    match server_req {
                                        ServerRequest::ApplyPatchApproval {
                                            request_id, ..
                                        } => {
                                            let response = JSONRPCResponse {
                                                id: request_id,
                                                result: serde_json::to_value(
                                                    ApplyPatchApprovalResponse {
                                                        decision: ReviewDecision::Approved,
                                                    },
                                                )
                                                .unwrap_or(Value::Null),
                                            };
                                            if let Err(err) = reader_peer.send(&response).await {
                                                tracing::warn!(
                                                    error = %err,
                                                    "Failed to send patch approval response"
                                                );
                                            }
                                        }
                                        ServerRequest::ExecCommandApproval {
                                            request_id, ..
                                        } => {
                                            let response = JSONRPCResponse {
                                                id: request_id,
                                                result: serde_json::to_value(
                                                    ExecCommandApprovalResponse {
                                                        decision: ReviewDecision::Approved,
                                                    },
                                                )
                                                .unwrap_or(Value::Null),
                                            };
                                            if let Err(err) = reader_peer.send(&response).await {
                                                tracing::warn!(
                                                    error = %err,
                                                    "Failed to send exec approval response"
                                                );
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(JSONRPCMessage::Error(err)) => {
                                let message = jsonrpc_error_message(&err);
                                if let Err(err) = tx_for_events
                                    .send(AgentEvent::Error(ErrorEvent {
                                        message,
                                        is_fatal: true,
                                        code: None,
                                        details: None,
                                    }))
                                    .await
                                {
                                    tracing::debug!(
                                        error = ?err,
                                        "Failed to forward JSON-RPC error event"
                                    );
                                }
                                reader_peer.reject(err).await;
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "Non-JSON output from codex app-server");
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "Codex app-server read loop failed");
                        break;
                    }
                }
            }
        });

        // Initialize connection
        let init_request = ClientRequest::Initialize {
            request_id: peer.next_request_id(),
            params: InitializeParams {
                client_info: ClientInfo {
                    name: "conduit".to_string(),
                    title: Some("Conduit".to_string()),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
            },
        };
        if let Err(error) = peer.request::<Value>(&init_request).await {
            Self::cleanup_startup_child(&mut child, "initialize").await?;
            return Err(AgentError::Io(error));
        }
        if let Err(error) = peer.send(&ClientNotification::Initialized).await {
            Self::cleanup_startup_child(&mut child, "initialized").await?;
            return Err(AgentError::Io(error));
        }
        tracing::debug!("Initialized Codex app-server");

        let mut thread_id = None;
        let mut session_model: Option<String> = None;

        if let Some(resume_session) = &config.resume_session {
            tracing::debug!(
                resume_session = resume_session.as_str(),
                "Attempting Codex thread resume"
            );
            let request = ClientRequest::ThreadResume {
                request_id: peer.next_request_id(),
                params: ThreadResumeParams {
                    thread_id: resume_session.as_str().to_string(),
                    history: None,
                    path: None,
                    model: config.model.clone(),
                    model_provider: None,
                    cwd: Some(config.working_dir.to_string_lossy().to_string()),
                    approval_policy: Some(Self::approval_policy().into()),
                    sandbox: Some(Self::sandbox_mode().into()),
                    config: Self::conversation_config(&config),
                    base_instructions: None,
                    developer_instructions: None,
                },
            };
            match tokio::time::timeout(CODEX_THREAD_RESUME_TIMEOUT, peer.request(&request)).await {
                Ok(Ok(response)) => {
                    let response: ThreadResumeResponse = response;
                    thread_id = Some(response.thread.id);
                    session_model = Some(response.model);
                    tracing::debug!(
                        thread_id = thread_id.as_deref(),
                        "Codex thread resume succeeded"
                    );
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        resume_session = resume_session.as_str(),
                        error = %err,
                        "Codex thread resume failed; starting a fresh thread"
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        resume_session = resume_session.as_str(),
                        timeout_secs = CODEX_THREAD_RESUME_TIMEOUT.as_secs(),
                        "Codex thread resume timed out; starting a fresh thread"
                    );
                }
            }
        }

        if thread_id.is_none() {
            tracing::debug!("Starting fresh Codex thread");
            let conv_request = ClientRequest::ThreadStart {
                request_id: peer.next_request_id(),
                params: ThreadStartParams {
                    model: config.model.clone(),
                    cwd: Some(config.working_dir.to_string_lossy().to_string()),
                    approval_policy: Some(Self::approval_policy().into()),
                    sandbox: Some(Self::sandbox_mode().into()),
                    config: Self::conversation_config(&config),
                    base_instructions: None,
                    model_provider: None,
                    experimental_raw_events: false,
                    developer_instructions: None,
                },
            };
            let response: ThreadStartResponse =
                match tokio::time::timeout(CODEX_THREAD_START_TIMEOUT, peer.request(&conv_request))
                    .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => {
                        Self::cleanup_startup_child(&mut child, "thread_start").await?;
                        return Err(AgentError::Io(error));
                    }
                    Err(_) => {
                        Self::cleanup_startup_child(&mut child, "thread_start_timeout").await?;
                        return Err(AgentError::Io(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "Codex thread start timed out after {}s",
                                CODEX_THREAD_START_TIMEOUT.as_secs()
                            ),
                        )));
                    }
                };
            thread_id = Some(response.thread.id);
            session_model = Some(response.model);
            tracing::debug!(
                thread_id = thread_id.as_deref(),
                "Codex thread start succeeded"
            );
        }

        let thread_id = thread_id.ok_or_else(|| {
            AgentError::Config("Failed to establish Codex conversation".to_string())
        })?;

        if let Err(err) = tx
            .send(AgentEvent::SessionInit(SessionInitEvent {
                session_id: SessionId::from_string(thread_id.clone()),
                model: session_model,
            }))
            .await
        {
            tracing::debug!(error = ?err, "Failed to send Codex SessionInit event");
        }

        // Input channel for subsequent prompts
        let (input_tx, mut input_rx) = mpsc::channel::<AgentInput>(32);
        let input_peer = peer.clone();
        let input_thread_id = thread_id.clone();
        let tx_for_input = tx.clone();
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                match input {
                    AgentInput::CodexPrompt {
                        text,
                        images,
                        skill,
                        ..
                    } => {
                        if let Err(err) = Self::send_user_message(
                            &input_peer,
                            &input_thread_id,
                            &text,
                            &images,
                            skill.as_ref(),
                        )
                        .await
                        {
                            tracing::warn!(error = %err, "Failed to send Codex prompt");
                            if let Err(send_err) = tx_for_input
                                .send(AgentEvent::TurnFailed(TurnFailedEvent {
                                    error: format!("Codex prompt failed: {err}"),
                                }))
                                .await
                            {
                                tracing::debug!(
                                    error = ?send_err,
                                    "Failed to send Codex TurnFailed event after prompt error"
                                );
                            }
                        }
                    }
                    AgentInput::ClaudeJsonl(_) => {
                        tracing::warn!("Ignored Claude JSONL sent to Codex input channel");
                    }
                    AgentInput::OpencodeQuestion { .. } => {
                        tracing::warn!(
                            "Ignored OpenCode question response sent to Codex input channel"
                        );
                    }
                }
            }
        });

        // Send initial prompt if present
        if !config.prompt.trim().is_empty() || !config.images.is_empty() || config.skill.is_some() {
            if let Err(error) = Self::send_user_message(
                &peer,
                &thread_id,
                &config.prompt,
                &config.images,
                config.skill.as_ref(),
            )
            .await
            {
                Self::cleanup_startup_child(&mut child, "initial_turn").await?;
                return Err(AgentError::Io(error));
            }
        }

        // Monitor process and capture stderr on failure
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let status = child.wait().await;

            let stderr_content = if let Some(mut stderr) = stderr {
                let mut buf = String::new();
                if let Err(err) = stderr.read_to_string(&mut buf).await {
                    tracing::debug!(error = %err, "Failed to read Codex stderr");
                }
                buf
            } else {
                String::new()
            };

            match status {
                Ok(exit_status) if !exit_status.success() => {
                    let error_msg = if stderr_content.is_empty() {
                        format!("Codex process exited with status: {}", exit_status)
                    } else {
                        format!(
                            "Codex process failed ({}): {}",
                            exit_status,
                            stderr_content.trim()
                        )
                    };
                    if let Err(err) = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: error_msg,
                            is_fatal: true,
                            code: None,
                            details: None,
                        }))
                        .await
                    {
                        tracing::debug!(
                            error = ?err,
                            "Failed to send Codex process failure event"
                        );
                    }
                }
                Err(err) => {
                    if let Err(send_err) = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("Failed to wait for Codex process: {}", err),
                            is_fatal: true,
                            code: None,
                            details: None,
                        }))
                        .await
                    {
                        tracing::debug!(
                            error = ?send_err,
                            "Failed to send Codex wait error event"
                        );
                    }
                }
                Ok(_) => {
                    if !stderr_content.is_empty() {
                        tracing::debug!("Codex stderr: {}", stderr_content.trim());
                    }
                }
            }
        });

        Ok(AgentHandle::new(rx, pid, Some(input_tx)))
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
        self.binary_path.exists() || Self::find_binary().is_some() || which::which("npx").is_ok()
    }

    fn binary_path(&self) -> Option<PathBuf> {
        if self.binary_path.exists() {
            Some(self.binary_path.clone())
        } else {
            Self::find_binary()
        }
    }
}

impl Default for CodexCliRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::{
        AgentMessageDeltaNotification, ItemCompletedNotification, ThreadItem, Turn,
        TurnCompletedNotification, TurnStatus,
    };
    use codex_protocol::items::{AgentMessageContent, AgentMessageItem, TurnItem};
    use codex_protocol::protocol::ItemCompletedEvent;
    use codex_protocol::ThreadId;

    #[test]
    fn test_jsonrpc_error_message_includes_data() {
        let error = JSONRPCError {
            id: RequestId::Integer(1),
            error: codex_app_server_protocol::JSONRPCErrorError {
                code: 42,
                message: "model not found".to_string(),
                data: Some(serde_json::json!({
                    "model": "gpt-5.4",
                    "provider": "openai"
                })),
            },
        };

        assert_eq!(
            jsonrpc_error_message(&error),
            r#"[Error 42] model not found: {"model":"gpt-5.4","provider":"openai"}"#
        );
    }

    #[test]
    fn test_build_user_inputs_with_text_and_images() {
        let tmp = tempfile::Builder::new()
            .prefix("conduit-codex-image-")
            .suffix(".png")
            .tempfile()
            .expect("failed to create temp image");
        let path = tmp.path().to_path_buf();
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .save(&path)
            .expect("failed to write temp image");

        let items = CodexCliRunner::build_user_inputs("hello", &[PathBuf::from(&path)], None);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], UserInput::Text { .. }));
        assert!(matches!(items[1], UserInput::LocalImage { .. }));
    }

    #[test]
    fn test_conversation_config_includes_reasoning_effort() {
        let config = AgentStartConfig::new("hello", PathBuf::from("/tmp"))
            .with_reasoning_effort(crate::agent::ReasoningEffort::XHigh);

        let values = CodexCliRunner::conversation_config(&config).expect("expected config");
        assert_eq!(
            values.get("model_reasoning_effort"),
            Some(&serde_json::Value::String("xhigh".to_string()))
        );
    }

    #[test]
    fn test_convert_item_completed_agent_message_to_assistant_event() {
        let mut state = CodexEventState::default();
        let event = EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: ThreadId::new(),
            turn_id: "turn-1".to_string(),
            item: TurnItem::AgentMessage(AgentMessageItem {
                id: "item-1".to_string(),
                content: vec![AgentMessageContent::Text {
                    text: "hello from item completed".to_string(),
                }],
            }),
        });

        let converted = CodexCliRunner::convert_event(&event, &mut state);
        assert_eq!(converted.len(), 1);
        match &converted[0] {
            AgentEvent::AssistantMessage(message) => {
                assert_eq!(message.text, "hello from item completed");
                assert!(message.is_final);
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }

    #[test]
    fn test_convert_server_notification_turn_completed_to_turn_completed_event() {
        let mut state = CodexEventState {
            last_usage: Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cached_tokens: 2,
                total_tokens: 17,
            }),
            ..Default::default()
        };
        let notification = ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: Turn {
                id: "turn-1".to_string(),
                items: Vec::new(),
                status: TurnStatus::Completed,
                error: None,
            },
        });

        let converted = CodexCliRunner::convert_server_notification(&notification, &mut state);
        assert_eq!(converted.len(), 1);
        match &converted[0] {
            AgentEvent::TurnCompleted(event) => {
                assert_eq!(event.usage.total_tokens, 17);
            }
            other => panic!("expected turn completed, got {other:?}"),
        }
    }

    #[test]
    fn test_convert_server_notification_streamed_agent_message_finishes_cleanly() {
        let mut state = CodexEventState::default();
        let delta = ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "item-1".to_string(),
            delta: "hi".to_string(),
        });
        let completed = ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item: ThreadItem::AgentMessage {
                id: "item-1".to_string(),
                text: "hi".to_string(),
            },
        });

        let delta_events = CodexCliRunner::convert_server_notification(&delta, &mut state);
        assert_eq!(delta_events.len(), 1);
        match &delta_events[0] {
            AgentEvent::AssistantMessage(message) => {
                assert_eq!(message.text, "hi");
                assert!(!message.is_final);
            }
            other => panic!("expected assistant delta, got {other:?}"),
        }

        let completed_events = CodexCliRunner::convert_server_notification(&completed, &mut state);
        assert_eq!(completed_events.len(), 1);
        match &completed_events[0] {
            AgentEvent::AssistantMessage(message) => {
                assert!(message.text.is_empty());
                assert!(message.is_final);
            }
            other => panic!("expected final assistant message, got {other:?}"),
        }
    }
}
