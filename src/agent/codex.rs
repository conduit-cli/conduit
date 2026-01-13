use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

use async_trait::async_trait;
use codex_app_server_protocol::{
    AddConversationListenerParams, ApplyPatchApprovalResponse, ClientInfo, ClientNotification,
    ClientRequest, ExecCommandApprovalResponse, InitializeParams, InputItem, JSONRPCMessage,
    JSONRPCResponse, NewConversationParams, NewConversationResponse, RequestId,
    ResumeConversationParams, ResumeConversationResponse, SendUserMessageParams,
    SendUserMessageResponse, ServerRequest,
};
use codex_protocol::config_types::SandboxMode;
use codex_protocol::protocol::{AskForApproval, EventMsg, FileChange, ReviewDecision};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, CommandOutputEvent, ContextCompactionEvent, ErrorEvent,
    FileChangedEvent, FileOperation, ReasoningEvent, TokenUsage, TokenUsageEvent,
    ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent, TurnFailedEvent,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};

const CODEX_NPX_PACKAGE: &str = "@openai/codex";
const CODEX_NPX_VERSION_ENV: &str = "CODEX_NPX_VERSION";

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
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Value>>>>,
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
            | ClientRequest::NewConversation { request_id, .. }
            | ClientRequest::ResumeConversation { request_id, .. }
            | ClientRequest::AddConversationListener { request_id, .. }
            | ClientRequest::SendUserMessage { request_id, .. } => request_id.clone(),
            _ => return Err(io::Error::other("unsupported request type")),
        };

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id, tx);
        self.send(request).await?;

        let value = rx.await.map_err(|_| io::Error::other("response dropped"))?;
        serde_json::from_value(value).map_err(|e| io::Error::other(e.to_string()))
    }

    async fn resolve(&self, request_id: RequestId, value: Value) {
        if let Some(tx) = self.pending.lock().await.remove(&request_id) {
            if tx.send(value).is_err() {
                tracing::debug!("Dropping JSON-RPC response; receiver already closed");
            }
        }
    }
}

#[derive(Default)]
struct CodexEventState {
    exec_command_by_id: HashMap<String, String>,
    exec_output_by_id: HashMap<String, String>,
    last_usage: Option<TokenUsage>,
    last_total_tokens: Option<i64>,
    pending_compaction: bool,
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
            .unwrap_or_else(|_| "never".to_string())
            .to_lowercase()
            .as_str()
        {
            "untrusted" => AskForApproval::UnlessTrusted,
            "on-failure" => AskForApproval::OnFailure,
            "on-request" => AskForApproval::OnRequest,
            "never" => AskForApproval::Never,
            _ => AskForApproval::Never,
        }
    }

    fn sandbox_mode() -> SandboxMode {
        match std::env::var("CODEX_SANDBOX_MODE")
            .unwrap_or_else(|_| "danger-full-access".to_string())
            .to_lowercase()
            .as_str()
        {
            "read-only" => SandboxMode::ReadOnly,
            "workspace-write" => SandboxMode::WorkspaceWrite,
            "danger-full-access" => SandboxMode::DangerFullAccess,
            _ => SandboxMode::DangerFullAccess,
        }
    }

    fn build_codex_command(&self, cwd: &PathBuf) -> io::Result<Command> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("app-server");
        cmd.current_dir(cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("NODE_NO_WARNINGS", "1");
        cmd.env("NO_COLOR", "1");
        Ok(cmd)
    }

    fn build_npx_command(&self, cwd: &PathBuf) -> io::Result<Command> {
        let mut cmd = Command::new("npx");
        cmd.args(["-y", &Self::npx_package(), "app-server"]);
        cmd.current_dir(cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("NODE_NO_WARNINGS", "1");
        cmd.env("NO_COLOR", "1");
        Ok(cmd)
    }

    fn build_input_items(prompt: &str, images: &[PathBuf]) -> Vec<InputItem> {
        let mut items = Vec::new();
        if !prompt.trim().is_empty() {
            items.push(InputItem::Text {
                text: prompt.to_string(),
            });
        }
        for image in images {
            items.push(InputItem::LocalImage {
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

    fn convert_event(event: &EventMsg, state: &mut CodexEventState) -> Vec<AgentEvent> {
        match event {
            EventMsg::TurnStarted(_) => vec![AgentEvent::TurnStarted],
            EventMsg::TurnComplete(_) => {
                let usage = state.last_usage.clone().unwrap_or_default();
                vec![AgentEvent::TurnCompleted(TurnCompletedEvent { usage })]
            }
            EventMsg::TurnAborted(ev) => vec![AgentEvent::TurnFailed(TurnFailedEvent {
                error: format!("Turn aborted: {:?}", ev.reason),
            })],
            EventMsg::AgentMessageDelta(msg) => {
                vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text: msg.delta.clone(),
                    is_final: false,
                })]
            }
            EventMsg::AgentMessage(msg) => {
                vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text: msg.message.clone(),
                    is_final: true,
                })]
            }
            EventMsg::AgentReasoningDelta(r) => {
                vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                    text: r.delta.clone(),
                })]
            }
            EventMsg::AgentReasoning(r) => vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                text: r.text.clone(),
            })],
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
                let tool_name = format!("mcp:{}::{}", ev.invocation.server, ev.invocation.tool);
                let (success, result, error) = match &ev.result {
                    Ok(result) => {
                        let rendered = serde_json::to_string(result).unwrap_or_default();
                        (!result.is_error.unwrap_or(false), Some(rendered), None)
                    }
                    Err(err) => (false, None, Some(err.clone())),
                };
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id: tool_name,
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
                tool_id: "WebSearch".to_string(),
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
                    tool_id: "ApplyPatch".to_string(),
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
                        tool_id: "ViewImage".to_string(),
                        success: true,
                        result: Some(ev.path.display().to_string()),
                        error: None,
                    }),
                ]
            }
            EventMsg::TokenCount(count) => {
                let mut events = Vec::new();
                if let Some(info) = &count.info {
                    let total = &info.total_token_usage;
                    let usage = TokenUsage {
                        input_tokens: total.input_tokens,
                        output_tokens: total.output_tokens,
                        cached_tokens: total.cached_input_tokens,
                        total_tokens: total.total_tokens,
                    };
                    let context_window = info.model_context_window;
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
                }
                events
            }
            EventMsg::ContextCompacted(_) => {
                state.pending_compaction = true;
                Vec::new()
            }
            EventMsg::Error(err) => vec![
                AgentEvent::Error(ErrorEvent {
                    message: err.message.clone(),
                    is_fatal: true,
                }),
                AgentEvent::TurnFailed(TurnFailedEvent {
                    error: err.message.clone(),
                }),
            ],
            EventMsg::Warning(warn) => vec![AgentEvent::Error(ErrorEvent {
                message: format!("Warning: {}", warn.message),
                is_fatal: false,
            })],
            EventMsg::StreamError(err) => vec![AgentEvent::Error(ErrorEvent {
                message: format!("Stream error: {}", err.message),
                is_fatal: false,
            })],
            _ => serde_json::to_value(event)
                .ok()
                .map(|data| vec![AgentEvent::Raw { data }])
                .unwrap_or_default(),
        }
    }

    async fn send_user_message(
        peer: &JsonRpcPeer,
        conversation_id: codex_protocol::ThreadId,
        prompt: &str,
        images: &[PathBuf],
    ) -> io::Result<()> {
        let items = Self::build_input_items(prompt, images);
        if items.is_empty() {
            return Ok(());
        }
        let request = ClientRequest::SendUserMessage {
            request_id: peer.next_request_id(),
            params: SendUserMessageParams {
                conversation_id,
                items,
            },
        };
        let _: SendUserMessageResponse = peer.request(&request).await?;
        Ok(())
    }

    async fn spawn_app_server(&self, cwd: &PathBuf) -> Result<tokio::process::Child, AgentError> {
        if self.binary_path.exists() {
            let mut cmd = self.build_codex_command(cwd)?;
            match cmd.spawn() {
                Ok(child) => return Ok(child),
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to spawn codex app-server, falling back to npx");
                }
            }
        }

        let mut cmd = self.build_npx_command(cwd)?;
        let child = cmd.spawn()?;
        Ok(child)
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
                                if notification.method.starts_with("codex/event/") {
                                    if let Some(params) = notification.params {
                                        match serde_json::from_value::<CodexNotificationParams>(
                                            params,
                                        ) {
                                            Ok(codex_params) => {
                                                let events = Self::convert_event(
                                                    &codex_params.msg,
                                                    &mut state,
                                                );
                                                for event in events {
                                                    if tx.send(event).await.is_err() {
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
                                let message = format!("[Error] {}", err.error.message);
                                if let Err(err) = tx
                                    .send(AgentEvent::Error(ErrorEvent {
                                        message,
                                        is_fatal: true,
                                    }))
                                    .await
                                {
                                    tracing::debug!(
                                        error = ?err,
                                        "Failed to forward JSON-RPC error event"
                                    );
                                }
                                reader_peer.resolve(err.id.clone(), Value::Null).await;
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
        let _: Value = peer.request(&init_request).await?;
        peer.send(&ClientNotification::Initialized).await?;

        let mut conversation_id = None;

        if let Some(resume_session) = &config.resume_session {
            let thread_id = codex_protocol::ThreadId::from_string(resume_session.as_str())
                .map_err(|err| AgentError::Config(err.to_string()))?;
            let request = ClientRequest::ResumeConversation {
                request_id: peer.next_request_id(),
                params: ResumeConversationParams {
                    path: None,
                    conversation_id: Some(thread_id),
                    history: None,
                    overrides: Some(NewConversationParams {
                        model: config.model.clone(),
                        model_provider: None,
                        profile: None,
                        cwd: Some(config.working_dir.to_string_lossy().to_string()),
                        approval_policy: Some(Self::approval_policy()),
                        sandbox: Some(Self::sandbox_mode()),
                        config: None,
                        base_instructions: None,
                        developer_instructions: None,
                        compact_prompt: None,
                        include_apply_patch_tool: None,
                    }),
                },
            };
            let response: ResumeConversationResponse = peer.request(&request).await?;
            conversation_id = Some(response.conversation_id);
        }

        if conversation_id.is_none() {
            let conv_request = ClientRequest::NewConversation {
                request_id: peer.next_request_id(),
                params: NewConversationParams {
                    model: config.model.clone(),
                    profile: None,
                    cwd: Some(config.working_dir.to_string_lossy().to_string()),
                    approval_policy: Some(Self::approval_policy()),
                    sandbox: Some(Self::sandbox_mode()),
                    config: None,
                    base_instructions: None,
                    include_apply_patch_tool: None,
                    model_provider: None,
                    compact_prompt: None,
                    developer_instructions: None,
                },
            };
            let response: NewConversationResponse = peer.request(&conv_request).await?;
            conversation_id = Some(response.conversation_id);
        }

        let conversation_id = conversation_id.ok_or_else(|| {
            AgentError::Config("Failed to establish Codex conversation".to_string())
        })?;

        // Subscribe to conversation events
        let listen_request = ClientRequest::AddConversationListener {
            request_id: peer.next_request_id(),
            params: AddConversationListenerParams {
                conversation_id,
                experimental_raw_events: false,
            },
        };
        let _: Value = peer.request(&listen_request).await?;

        // Input channel for subsequent prompts
        let (input_tx, mut input_rx) = mpsc::channel::<AgentInput>(32);
        let input_peer = peer.clone();
        let input_conversation_id = conversation_id;
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                match input {
                    AgentInput::CodexPrompt { text, images } => {
                        if let Err(err) = Self::send_user_message(
                            &input_peer,
                            input_conversation_id,
                            &text,
                            &images,
                        )
                        .await
                        {
                            tracing::warn!(error = %err, "Failed to send Codex prompt");
                        }
                    }
                    AgentInput::ClaudeJsonl(_) => {
                        tracing::warn!("Ignored Claude JSONL sent to Codex input channel");
                    }
                }
            }
        });

        // Send initial prompt if present
        if !config.prompt.trim().is_empty() || !config.images.is_empty() {
            Self::send_user_message(&peer, conversation_id, &config.prompt, &config.images).await?;
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

    #[test]
    fn test_build_input_items_with_text_and_images() {
        let items = CodexCliRunner::build_input_items(
            "hello",
            &[
                PathBuf::from("/tmp/test.png"),
                PathBuf::from("/tmp/other.png"),
            ],
        );
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], InputItem::Text { .. }));
        assert!(matches!(items[1], InputItem::LocalImage { .. }));
        assert!(matches!(items[2], InputItem::LocalImage { .. }));
    }
}
