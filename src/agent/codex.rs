use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent::display::MessageDisplay;
use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, CommandOutputEvent, ErrorEvent, ReasoningEvent,
    SessionInitEvent, TokenUsage, TokenUsageEvent, ToolCompletedEvent, ToolStartedEvent,
    TurnCompletedEvent, TurnFailedEvent,
};
use crate::agent::runner::{AgentHandle, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;
use crate::agent::stream::{CodexErrorInfo, CodexThreadItem, CodexUsage, JsonlStreamParser};

pub struct CodexCliRunner {
    binary_path: PathBuf,
}

#[derive(Debug, Clone)]
struct FunctionCallInfo {
    name: String,
    command: String,
}

impl CodexCliRunner {
    pub fn new() -> Self {
        Self {
            binary_path: Self::find_binary().unwrap_or_else(|| PathBuf::from("codex")),
        }
    }

    fn find_binary() -> Option<PathBuf> {
        which::which("codex").ok()
    }

    fn build_command(&self, config: &AgentStartConfig) -> Command {
        let mut cmd = Command::new(&self.binary_path);

        // Start with exec subcommand
        cmd.arg("exec");

        // Flags must come before positional arguments in Codex CLI
        cmd.arg("--json");

        // Model selection (flag, so comes before positional args)
        if let Some(model) = &config.model {
            cmd.arg("-m").arg(model);
        }

        if !config.images.is_empty() {
            for image in &config.images {
                cmd.arg("--image").arg(image);
            }
        }

        // High reasoning effort for better responses
        cmd.arg("-c").arg("model_reasoning_effort=\"high\"");

        // Enable web search
        cmd.arg("-c").arg("features.web_search_request=true");

        // Enable skills
        cmd.arg("--enable").arg("skills");

        // --yolo bypasses approvals and sandbox restrictions
        // Required for git operations in worktrees and complex repo structures
        cmd.arg("--yolo");

        // Additional args (assumed to be flags)
        for arg in &config.additional_args {
            cmd.arg(arg);
        }

        // Now add positional arguments: resume/prompt
        if let Some(session_id) = &config.resume_session {
            // Resume existing session: exec [flags] resume <session_id> [prompt]
            cmd.arg("resume").arg(session_id.as_str());
            if !config.prompt.is_empty() {
                cmd.arg(&config.prompt);
            }
        } else {
            // New session: exec [flags] <prompt>
            cmd.arg(&config.prompt);
        }

        // Working directory
        cmd.current_dir(&config.working_dir);

        // Stdio setup
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        cmd
    }

    fn extract_text_content(payload: &Value) -> String {
        if let Some(blocks) = payload.get("content").and_then(|c| c.as_array()) {
            return blocks
                .iter()
                .filter_map(|block| {
                    let block_type = block.get("type")?.as_str()?;
                    match block_type {
                        "input_text" | "output_text" | "text" => {
                            block.get("text")?.as_str().map(|s| s.to_string())
                        }
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }

        payload
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn extract_summary_text(payload: &Value) -> Option<String> {
        let summary = payload.get("summary")?.as_array()?;
        let text = summary
            .iter()
            .filter_map(|entry| entry.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn parse_args(payload: &Value) -> Option<Value> {
        if let Some(args_str) = payload.get("arguments").and_then(|a| a.as_str()) {
            serde_json::from_str::<Value>(args_str).ok()
        } else if let Some(args_obj) = payload.get("arguments").and_then(|a| a.as_object()) {
            Some(Value::Object(args_obj.clone()))
        } else {
            None
        }
    }

    fn extract_command_from_args(args: &Value) -> Option<String> {
        args.get("command")
            .or_else(|| args.get("cmd"))
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn extract_function_call_info(raw: &Value) -> Option<(String, FunctionCallInfo)> {
        let entry_type = raw.get("type")?.as_str()?;
        if entry_type != "response_item" {
            return None;
        }
        let payload = raw.get("payload")?;
        let payload_type = payload.get("type")?.as_str()?;
        if payload_type != "function_call" {
            return None;
        }

        let call_id = payload.get("call_id")?.as_str()?.to_string();
        let name = payload.get("name")?.as_str()?.to_string();
        let args = Self::parse_args(payload).unwrap_or(Value::Null);
        let command = Self::extract_command_from_args(&args).unwrap_or_default();

        Some((call_id, FunctionCallInfo { name, command }))
    }

    fn convert_message(payload: &Value) -> Option<AgentEvent> {
        let role = payload.get("role").and_then(|r| r.as_str())?;
        let text = Self::extract_text_content(payload);
        if text.is_empty() {
            return None;
        }

        match role {
            "assistant" => Some(AgentEvent::AssistantMessage(AssistantMessageEvent {
                text,
                is_final: true,
            })),
            "user" => None,
            _ => None,
        }
    }

    fn convert_response_item(
        payload: &Value,
        function_calls: &HashMap<String, FunctionCallInfo>,
    ) -> Option<AgentEvent> {
        let payload_type = payload.get("type").and_then(|t| t.as_str())?;
        match payload_type {
            "message" => Self::convert_message(payload),
            "function_call" => {
                let name = payload
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool");
                let args = Self::parse_args(payload).unwrap_or(Value::Null);
                Some(AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name: name.to_string(),
                    tool_id: name.to_string(),
                    arguments: args,
                }))
            }
            "function_call_output" => {
                let call_id = payload
                    .get("call_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let raw_output = payload.get("output").and_then(|o| o.as_str()).unwrap_or("");
                let (output, exit_code) = MessageDisplay::parse_codex_tool_output(raw_output);
                let info = function_calls.get(call_id);
                let tool_name = info.map(|i| i.name.as_str()).unwrap_or("tool");
                let command = info.map(|i| i.command.as_str()).unwrap_or(call_id);

                if Self::is_shell_tool(tool_name) {
                    Some(AgentEvent::CommandOutput(CommandOutputEvent {
                        command: command.to_string(),
                        output,
                        exit_code,
                        is_streaming: false,
                    }))
                } else {
                    Some(AgentEvent::ToolCompleted(ToolCompletedEvent {
                        tool_id: tool_name.to_string(),
                        success: true,
                        result: Some(output),
                        error: None,
                    }))
                }
            }
            "reasoning" => Self::extract_summary_text(payload)
                .map(|text| AgentEvent::AssistantReasoning(ReasoningEvent { text })),
            _ => Some(AgentEvent::Raw {
                data: payload.clone(),
            }),
        }
    }

    fn convert_event_msg(payload: &Value) -> Option<AgentEvent> {
        let payload_type = payload.get("type").and_then(|t| t.as_str())?;
        match payload_type {
            "agent_message" => payload.get("message").and_then(|m| m.as_str()).map(|text| {
                AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text: text.to_string(),
                    is_final: true,
                })
            }),
            "agent_reasoning" => payload.get("text").and_then(|t| t.as_str()).map(|text| {
                AgentEvent::AssistantReasoning(ReasoningEvent {
                    text: text.to_string(),
                })
            }),
            "token_count" => {
                let info = payload.get("info")?;
                let total = info
                    .get("total_token_usage")
                    .or_else(|| info.get("last_token_usage"))?;
                let input_tokens = total.get("input_tokens")?.as_i64()?;
                let output_tokens = total.get("output_tokens")?.as_i64()?;
                let cached_tokens = total
                    .get("cached_input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let total_tokens = total
                    .get("total_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(input_tokens + output_tokens);
                let context_window = info.get("model_context_window").and_then(|v| v.as_i64());

                Some(AgentEvent::TokenUsage(TokenUsageEvent {
                    usage: TokenUsage {
                        input_tokens,
                        output_tokens,
                        cached_tokens,
                        total_tokens,
                    },
                    context_window,
                    usage_percent: None,
                }))
            }
            _ => Some(AgentEvent::Raw {
                data: payload.clone(),
            }),
        }
    }

    fn convert_thread_event(raw: &Value) -> Option<AgentEvent> {
        let event_type = raw.get("type").and_then(|t| t.as_str())?;
        match event_type {
            "thread.started" => raw.get("thread_id").and_then(|v| v.as_str()).map(|id| {
                AgentEvent::SessionInit(SessionInitEvent {
                    session_id: SessionId::from_string(id.to_string()),
                    model: None,
                })
            }),
            "turn.started" => Some(AgentEvent::TurnStarted),
            "turn.completed" => {
                let usage: CodexUsage = serde_json::from_value(raw.get("usage")?.clone()).ok()?;
                Some(AgentEvent::TurnCompleted(TurnCompletedEvent {
                    usage: TokenUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cached_tokens: usage.cached_input_tokens,
                        total_tokens: usage.input_tokens + usage.output_tokens,
                    },
                }))
            }
            "turn.failed" => {
                let error: CodexErrorInfo =
                    serde_json::from_value(raw.get("error")?.clone()).ok()?;
                Some(AgentEvent::TurnFailed(TurnFailedEvent {
                    error: error.message,
                }))
            }
            "item.completed" | "item.updated" => {
                let item: CodexThreadItem =
                    serde_json::from_value(raw.get("item")?.clone()).ok()?;
                Self::convert_item_event(&item)
            }
            "error" => raw.get("message").and_then(|m| m.as_str()).map(|message| {
                AgentEvent::Error(ErrorEvent {
                    message: message.to_string(),
                    is_fatal: true,
                })
            }),
            _ => None,
        }
    }

    /// Convert Codex-specific event to unified AgentEvent
    fn convert_event(
        raw: &Value,
        function_calls: &HashMap<String, FunctionCallInfo>,
    ) -> Option<AgentEvent> {
        let event_type = raw.get("type").and_then(|t| t.as_str())?;
        match event_type {
            "session_meta" => raw.get("payload").and_then(|p| {
                p.get("id").and_then(|id| id.as_str()).map(|id| {
                    AgentEvent::SessionInit(SessionInitEvent {
                        session_id: SessionId::from_string(id.to_string()),
                        model: None,
                    })
                })
            }),
            "response_item" => raw
                .get("payload")
                .and_then(|payload| Self::convert_response_item(payload, function_calls)),
            "event_msg" => raw
                .get("payload")
                .and_then(|payload| Self::convert_event_msg(payload)),
            "message" => Self::convert_message(raw),
            "thread.started" | "turn.started" | "turn.completed" | "turn.failed"
            | "item.updated" | "item.completed" | "error" => Self::convert_thread_event(raw),
            _ => Some(AgentEvent::Raw { data: raw.clone() }),
        }
    }

    fn convert_item_event(item: &CodexThreadItem) -> Option<AgentEvent> {
        let item_type = item.item_type.as_deref()?;

        match item_type {
            "agent_message" | "message" => {
                let text = Self::extract_text_content(&item.details);
                if text.is_empty() {
                    return None;
                }
                Some(AgentEvent::AssistantMessage(AssistantMessageEvent {
                    text,
                    is_final: true,
                }))
            }
            "command_execution" | "local_shell_call" => {
                let command = item
                    .details
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let output = item
                    .details
                    .get("aggregated_output")
                    .or_else(|| item.details.get("output"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let exit_code = item
                    .details
                    .get("exit_code")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32);
                Some(AgentEvent::CommandOutput(CommandOutputEvent {
                    command: command.to_string(),
                    output: output.to_string(),
                    exit_code,
                    is_streaming: false,
                }))
            }
            _ => None,
        }
    }

    fn is_shell_tool(name: &str) -> bool {
        matches!(
            name,
            "shell_command"
                | "exec_command"
                | "command_execution"
                | "local_shell_call"
                | "shell"
                | "Bash"
        )
    }
}

impl Default for CodexCliRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentRunner for CodexCliRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        let mut cmd = self.build_command(&config);
        let mut child = cmd.spawn()?;

        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;
        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;
        let stderr = child.stderr.take();

        let (tx, rx) = mpsc::channel::<AgentEvent>(256);
        let tx_for_monitor = tx.clone();

        // Spawn JSONL parser task
        tokio::spawn(async move {
            let (raw_tx, mut raw_rx) = mpsc::channel::<Value>(256);

            let parse_handle = tokio::spawn(async move {
                let _ = JsonlStreamParser::parse_stream(stdout, raw_tx).await;
            });

            let mut function_calls: HashMap<String, FunctionCallInfo> = HashMap::new();
            let mut last_assistant_text: Option<String> = None;

            while let Some(raw_event) = raw_rx.recv().await {
                if let Some((call_id, info)) = Self::extract_function_call_info(&raw_event) {
                    function_calls.insert(call_id, info);
                }
                if let Some(event) = Self::convert_event(&raw_event, &function_calls) {
                    if let AgentEvent::AssistantMessage(msg) = &event {
                        if last_assistant_text.as_deref() == Some(msg.text.as_str()) {
                            continue;
                        }
                        last_assistant_text = Some(msg.text.clone());
                    }
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            }

            let _ = parse_handle.await;
        });

        // Monitor process and capture stderr on failure
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let status = child.wait().await;

            // Read stderr if available
            let stderr_content = if let Some(mut stderr) = stderr {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                buf
            } else {
                String::new()
            };

            // Check if process failed
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
                    let _ = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: error_msg,
                            is_fatal: true,
                        }))
                        .await;
                }
                Err(e) => {
                    let error_msg = format!("Failed to wait for Codex process: {}", e);
                    let _ = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: error_msg,
                            is_fatal: true,
                        }))
                        .await;
                }
                Ok(_) => {
                    // Process exited successfully, but if there was stderr output, log it
                    if !stderr_content.is_empty() {
                        // Could log this for debugging, but don't treat as error
                    }
                }
            }
        });

        Ok(AgentHandle::new(rx, pid))
    }

    async fn send_input(&self, _handle: &AgentHandle, _input: &str) -> Result<(), AgentError> {
        Err(AgentError::NotSupported(
            "Codex exec mode doesn't support interactive input".into(),
        ))
    }

    async fn stop(&self, handle: &AgentHandle) -> Result<(), AgentError> {
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(handle.pid as i32, libc::SIGTERM);
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
            unsafe {
                libc::kill(handle.pid as i32, libc::SIGKILL);
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
        self.binary_path.exists() || Self::find_binary().is_some()
    }

    fn binary_path(&self) -> Option<PathBuf> {
        if self.binary_path.exists() {
            Some(self.binary_path.clone())
        } else {
            Self::find_binary()
        }
    }
}
