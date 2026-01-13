use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, ControlRequestEvent, ErrorEvent, SessionInitEvent,
    TokenUsage, ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;
use crate::agent::stream::{ClaudeRawEvent, JsonlStreamParser};

pub struct ClaudeCodeRunner {
    binary_path: PathBuf,
}

impl ClaudeCodeRunner {
    pub fn new() -> Self {
        Self {
            binary_path: Self::find_binary().unwrap_or_else(|| PathBuf::from("claude")),
        }
    }

    /// Create a runner with a specific binary path
    pub fn with_path(path: PathBuf) -> Self {
        Self { binary_path: path }
    }

    fn find_binary() -> Option<PathBuf> {
        which::which("claude").ok()
    }

    fn build_command(&self, config: &AgentStartConfig) -> Command {
        let mut cmd = Command::new(&self.binary_path);

        let use_stream_input = config
            .input_format
            .as_deref()
            .is_some_and(|format| format == "stream-json");

        // Core headless mode flags
        if !use_stream_input {
            cmd.arg("-p"); // Print mode (standalone flag, prompt is positional)
        }
        cmd.arg("--output-format").arg("stream-json");
        cmd.arg("--verbose"); // verbose is now required
                              // Claude process failed (exit status: 1): Error: When
                              // using --print,--output-format=stream-json requires
                              // --verbose
        if use_stream_input {
            cmd.arg("--permission-prompt-tool").arg("stdio");
        }

        // Permission mode (Build vs Plan)
        cmd.arg("--permission-mode")
            .arg(config.agent_mode.as_permission_mode());

        // Allowed tools
        if !config.allowed_tools.is_empty() {
            cmd.arg("--allowedTools")
                .arg(config.allowed_tools.join(","));
        }

        // Resume session if provided
        if let Some(session_id) = &config.resume_session {
            cmd.arg("--resume").arg(session_id.as_str());
        }

        // Model selection
        if let Some(model) = &config.model {
            cmd.arg("--model").arg(model);
        }

        // Working directory
        cmd.current_dir(&config.working_dir);

        // Input format override (e.g. stream-json for structured input)
        if let Some(format) = &config.input_format {
            cmd.arg("--input-format").arg(format);
        }

        // Additional args
        for arg in &config.additional_args {
            cmd.arg(arg);
        }

        // Use "--" to signal end of flags, so prompts starting with "-" (like "- [ ] task")
        // are not interpreted as CLI arguments
        if !use_stream_input && !config.prompt.is_empty() {
            cmd.arg("--").arg(&config.prompt);
        }

        // Stdio setup for JSONL capture / streaming input
        let needs_stdin = config
            .input_format
            .as_deref()
            .is_some_and(|format| format == "stream-json")
            || config.stdin_payload.is_some();
        if needs_stdin {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        cmd
    }

    /// Convert Claude-specific event to unified AgentEvent(s)
    /// Returns a Vec because Assistant events can contain both text and tool_use blocks
    fn convert_event(raw: ClaudeRawEvent) -> Vec<AgentEvent> {
        tracing::debug!("Claude raw event: {:?}", raw);
        match raw {
            ClaudeRawEvent::System(sys) => {
                if sys.subtype.as_deref() == Some("init") {
                    sys.session_id
                        .map(|id| {
                            vec![AgentEvent::SessionInit(SessionInitEvent {
                                session_id: SessionId::from_string(id),
                                model: sys.model,
                            })]
                        })
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            }
            ClaudeRawEvent::Assistant(assistant) => {
                let mut events = Vec::new();

                // Check for authentication failure or other errors
                if let Some(ref error) = assistant.error {
                    if error == "authentication_failed" {
                        return vec![AgentEvent::Error(ErrorEvent {
                            message: "Authentication failed. Please run `claude /login` in your terminal to authenticate.".to_string(),
                            is_fatal: true,
                        })];
                    }
                    // Handle other error types as fatal errors
                    return vec![AgentEvent::Error(ErrorEvent {
                        message: format!("Claude error: {}", error),
                        is_fatal: true,
                    })];
                }

                // Extract text content
                let text = assistant.extract_text().unwrap_or_default();
                if !text.is_empty() {
                    events.push(AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text,
                        is_final: true,
                    }));
                }

                // Extract embedded tool_use blocks
                for tool_use in assistant.extract_tool_uses() {
                    events.push(AgentEvent::ToolStarted(ToolStartedEvent {
                        tool_name: tool_use.name,
                        tool_id: tool_use.id,
                        arguments: tool_use.input,
                    }));
                }

                events
            }
            ClaudeRawEvent::ToolUse(tool) => {
                let tool_name = tool.tool.or(tool.name).unwrap_or_default();
                let tool_id = tool
                    .id
                    .unwrap_or_else(|| format!("claude_{}", uuid::Uuid::new_v4()));
                let arguments = if tool.arguments.is_null() {
                    tool.input
                } else {
                    tool.arguments
                };
                vec![AgentEvent::ToolStarted(ToolStartedEvent {
                    tool_name,
                    tool_id,
                    arguments,
                })]
            }
            ClaudeRawEvent::ToolResult(result) => {
                let tool_id = result.tool_use_id.clone().unwrap_or_default();
                let is_error = result.is_error.unwrap_or(false);
                tracing::debug!(
                    "ToolResult received: tool_id={}, is_error={}, content_len={}",
                    tool_id,
                    is_error,
                    result.content.as_ref().map(|c| c.len()).unwrap_or(0)
                );
                vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
                    tool_id,
                    success: !is_error,
                    result: if !is_error {
                        result.content.clone()
                    } else {
                        None
                    },
                    error: if is_error { result.content } else { None },
                })]
            }
            ClaudeRawEvent::Result(res) => {
                // Result event always signals turn completion
                // Use default values if usage is not provided
                let usage = res
                    .usage
                    .map(|u| TokenUsage {
                        input_tokens: u.input_tokens.unwrap_or(0),
                        output_tokens: u.output_tokens.unwrap_or(0),
                        cached_tokens: 0,
                        total_tokens: u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0),
                    })
                    .unwrap_or_default();

                vec![AgentEvent::TurnCompleted(TurnCompletedEvent { usage })]
            }
            ClaudeRawEvent::User(user) => {
                // User events contain tool results from Claude Code CLI
                let mut events = Vec::new();
                for (tool_id, content, is_error) in user.extract_tool_results() {
                    tracing::debug!(
                        "User event tool result: tool_id={}, is_error={}, content_len={}",
                        tool_id,
                        is_error,
                        content.len()
                    );
                    events.push(AgentEvent::ToolCompleted(ToolCompletedEvent {
                        tool_id,
                        success: !is_error,
                        result: if !is_error {
                            Some(content.clone())
                        } else {
                            None
                        },
                        error: if is_error { Some(content) } else { None },
                    }));
                }
                events
            }
            ClaudeRawEvent::ControlRequest(_) => vec![],
            ClaudeRawEvent::Unknown => vec![],
        }
    }

    fn build_control_initialize_jsonl() -> String {
        let payload = json!({
            "type": "control_request",
            "request_id": uuid::Uuid::new_v4().to_string(),
            "request": {
                "subtype": "initialize",
                "hooks": serde_json::Value::Null,
            }
        });
        format!("{}\n", payload)
    }

    fn build_control_response_jsonl(
        request_id: &str,
        response_payload: serde_json::Value,
    ) -> anyhow::Result<String> {
        let payload = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": response_payload,
            }
        });
        let json = serde_json::to_string(&payload)?;
        Ok(format!("{json}\n"))
    }

    fn is_interactive_tool(tool_name: &str) -> bool {
        matches!(tool_name, "AskUserQuestion" | "ExitPlanMode")
    }
}

impl Default for ClaudeCodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentRunner for ClaudeCodeRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Claude
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        let mut cmd = self.build_command(&config);
        let mut child = cmd.spawn()?;

        let use_stream_input = config
            .input_format
            .as_deref()
            .is_some_and(|format| format == "stream-json");
        let stdin_payload = config.stdin_payload.clone();
        let mut input_tx: Option<mpsc::Sender<AgentInput>> = None;

        if let Some(stdin) = child.stdin.take() {
            if use_stream_input {
                let (tx, mut rx) = mpsc::channel::<AgentInput>(32);
                input_tx = Some(tx);
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let mut stdin = stdin;
                    let init_payload = Self::build_control_initialize_jsonl();
                    if let Err(err) = stdin.write_all(init_payload.as_bytes()).await {
                        tracing::error!(
                            "Failed to write control initialize to Claude stdin: {}",
                            err
                        );
                        return;
                    }
                    if let Some(payload) = stdin_payload {
                        tracing::info!(
                            "Writing initial payload to Claude stdin: {} bytes",
                            payload.len()
                        );
                        if let Err(err) = stdin.write_all(payload.as_bytes()).await {
                            tracing::error!(
                                "Failed to write initial payload to Claude stdin: {}",
                                err
                            );
                            return;
                        }
                        tracing::info!("Successfully wrote initial payload to Claude stdin");
                    } else {
                        tracing::info!("No initial stdin_payload to write to Claude");
                    }
                    while let Some(input) = rx.recv().await {
                        match input {
                            AgentInput::ClaudeJsonl(line) => {
                                if let Err(err) = stdin.write_all(line.as_bytes()).await {
                                    tracing::error!("Failed to write to Claude stdin: {}", err);
                                    break;
                                }
                            }
                            AgentInput::CodexPrompt { .. } => {
                                tracing::warn!("Ignored Codex prompt sent to Claude input channel");
                            }
                        }
                    }
                    if let Err(err) = stdin.shutdown().await {
                        tracing::warn!("Failed to close Claude stdin: {}", err);
                    }
                });
            } else if let Some(payload) = stdin_payload {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let mut stdin = stdin;
                    if let Err(err) = stdin.write_all(payload.as_bytes()).await {
                        tracing::error!("Failed to write to Claude stdin: {}", err);
                        return;
                    }
                    if let Err(err) = stdin.shutdown().await {
                        tracing::warn!("Failed to close Claude stdin: {}", err);
                    }
                });
            }
        }

        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;
        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;
        let stderr = child.stderr.take();

        let (tx, rx) = mpsc::channel::<AgentEvent>(256);
        let tx_for_monitor = tx.clone();
        let control_tx = input_tx.clone();

        // Spawn JSONL parser task
        tokio::spawn(async move {
            let (raw_tx, mut raw_rx) = mpsc::channel::<ClaudeRawEvent>(256);
            let tx_for_parser = tx.clone();

            // Parse raw events
            let parse_handle = tokio::spawn(async move {
                if let Err(e) = JsonlStreamParser::parse_stream(stdout, raw_tx).await {
                    if let Err(send_err) = tx_for_parser
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("Stream parsing error: {}", e),
                            is_fatal: true,
                        }))
                        .await
                    {
                        tracing::debug!(
                            error = ?send_err,
                            "Failed to send Claude stream parsing error"
                        );
                    }
                }
            });

            // Convert and forward events
            'outer: while let Some(raw_event) = raw_rx.recv().await {
                if let ClaudeRawEvent::ControlRequest(request) = &raw_event {
                    match &request.request {
                        crate::agent::stream::ClaudeControlRequestType::CanUseTool {
                            tool_name,
                            input,
                            tool_use_id,
                        } => {
                            if Self::is_interactive_tool(tool_name) {
                                let event = AgentEvent::ControlRequest(ControlRequestEvent {
                                    request_id: request.request_id.clone(),
                                    tool_name: tool_name.clone(),
                                    tool_use_id: tool_use_id.clone(),
                                    input: input.clone(),
                                });
                                if tx.send(event).await.is_err() {
                                    break 'outer;
                                }
                                if control_tx.is_none() {
                                    tracing::warn!(
                                        tool_name = tool_name,
                                        "Control request for interactive tool received without stdin channel"
                                    );
                                }
                            } else if let Some(ref tx) = control_tx {
                                let mut response_payload = serde_json::Map::new();
                                response_payload.insert("behavior".to_string(), json!("allow"));
                                response_payload.insert("updatedInput".to_string(), input.clone());
                                if let Some(tool_use_id) = tool_use_id.as_ref() {
                                    response_payload
                                        .insert("toolUseID".to_string(), json!(tool_use_id));
                                }
                                if let Ok(response) = Self::build_control_response_jsonl(
                                    &request.request_id,
                                    serde_json::Value::Object(response_payload),
                                ) {
                                    if let Err(err) =
                                        tx.send(AgentInput::ClaudeJsonl(response)).await
                                    {
                                        tracing::warn!(
                                            "Failed to respond to control request: {}",
                                            err
                                        );
                                    }
                                }
                            }
                        }
                        crate::agent::stream::ClaudeControlRequestType::HookCallback { .. } => {
                            if let Some(ref tx) = control_tx {
                                if let Ok(response) = Self::build_control_response_jsonl(
                                    &request.request_id,
                                    json!({ "decision": "allow" }),
                                ) {
                                    if let Err(err) =
                                        tx.send(AgentInput::ClaudeJsonl(response)).await
                                    {
                                        tracing::warn!(
                                            "Failed to respond to hook callback: {}",
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }
                    continue 'outer;
                }

                for event in Self::convert_event(raw_event) {
                    if tx.send(event).await.is_err() {
                        break 'outer;
                    }
                }
            }

            if let Err(join_err) = parse_handle.await {
                tracing::warn!(error = ?join_err, "Claude parser task failed to join");
            }
        });

        // Monitor process exit and capture stderr
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let status = child.wait().await;

            // Read stderr if available
            let stderr_content = if let Some(mut stderr) = stderr {
                let mut buf = String::new();
                if let Err(e) = stderr.read_to_string(&mut buf).await {
                    tracing::debug!(error = %e, "Failed to read Claude stderr");
                }
                buf
            } else {
                String::new()
            };

            // Check if process failed
            match status {
                Ok(exit_status) if !exit_status.success() => {
                    let error_msg = if stderr_content.is_empty() {
                        format!("Claude process exited with status: {}", exit_status)
                    } else {
                        format!(
                            "Claude process failed ({}): {}",
                            exit_status,
                            stderr_content.trim()
                        )
                    };
                    if let Err(send_err) = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: error_msg,
                            is_fatal: true,
                        }))
                        .await
                    {
                        tracing::debug!(
                            error = ?send_err,
                            "Failed to send Claude process failure"
                        );
                    }
                }
                Err(e) => {
                    if let Err(send_err) = tx_for_monitor
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("Failed to wait for Claude process: {}", e),
                            is_fatal: true,
                        }))
                        .await
                    {
                        tracing::debug!(
                            error = ?send_err,
                            "Failed to send Claude wait error"
                        );
                    }
                }
                Ok(_) => {}
            }
        });

        Ok(AgentHandle::new(rx, pid, input_tx))
    }

    async fn send_input(
        &self,
        _handle: &AgentHandle,
        _input: AgentInput,
    ) -> Result<(), AgentError> {
        // Claude Code headless mode doesn't support interactive input via stdin
        // Tool results must be sent by resuming the session with a new prompt
        Err(AgentError::NotSupported(
            "Claude headless mode doesn't support interactive input".into(),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream::{
        ClaudeAssistantEvent, ClaudeContentBlock, ClaudeMessageObject, ClaudeRawEvent,
        ClaudeResultEvent, ClaudeSystemEvent, ClaudeUsage,
    };

    /// Test that a system init event is correctly converted to SessionInit
    #[test]
    fn test_convert_system_init_event() {
        let raw = ClaudeRawEvent::System(ClaudeSystemEvent {
            subtype: Some("init".to_string()),
            session_id: Some("test-session-123".to_string()),
            model: Some("claude-sonnet-4-5-20250929".to_string()),
        });

        let events = ClaudeCodeRunner::convert_event(raw);
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgentEvent::SessionInit(init) => {
                assert_eq!(init.session_id.as_str(), "test-session-123");
                assert_eq!(init.model, Some("claude-sonnet-4-5-20250929".to_string()));
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }
    }

    /// Test that a normal assistant event (no error) produces AssistantMessage
    #[test]
    fn test_convert_normal_assistant_event() {
        let raw = ClaudeRawEvent::Assistant(ClaudeAssistantEvent {
            message: Some(ClaudeMessageObject {
                model: Some("claude-sonnet".to_string()),
                id: Some("029c1c0f-6927-4a48-aae1-21a3e895456f".to_string()),
                role: Some("assistant".to_string()),
                content: Some(vec![ClaudeContentBlock::Text {
                    text: "Hello, how can I help?".to_string(),
                }]),
                stop_reason: Some("stop_sequence".to_string()),
                usage: Some(ClaudeUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                }),
            }),
            text: None,
            session_id: Some("50884eed-28b7-431e-9ad8-78b326696ae7".to_string()),
            error: None,
        });

        let events = ClaudeCodeRunner::convert_event(raw);
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgentEvent::AssistantMessage(msg) => {
                assert_eq!(msg.text, "Hello, how can I help?");
                assert!(msg.is_final);
            }
            other => panic!("Expected AssistantMessage, got {:?}", other),
        }
    }

    /// Test that a result event with is_error produces TurnCompleted
    /// Note: The is_error field is currently not used to emit an error event
    #[test]
    fn test_convert_auth_failure_result_event() {
        let raw = ClaudeRawEvent::Result(ClaudeResultEvent {
            result: Some("Invalid API key · Please run /login".to_string()),
            output: None,
            session_id: Some("50884eed-28b7-431e-9ad8-78b326696ae7".to_string()),
            usage: Some(ClaudeUsage {
                input_tokens: Some(0),
                output_tokens: Some(0),
            }),
        });

        let events = ClaudeCodeRunner::convert_event(raw);
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgentEvent::TurnCompleted(completed) => {
                // Usage should be parsed correctly even with zero values
                assert_eq!(completed.usage.input_tokens, 0);
                assert_eq!(completed.usage.output_tokens, 0);
            }
            other => panic!("Expected TurnCompleted, got {:?}", other),
        }
    }

    /// Test the full auth failure sequence conversion
    /// This simulates what happens when Claude CLI returns an auth error
    #[test]
    fn test_convert_auth_failure_full_sequence() {
        let raw_events = vec![
            ClaudeRawEvent::System(ClaudeSystemEvent {
                subtype: Some("init".to_string()),
                session_id: Some("test-session".to_string()),
                model: Some("claude-sonnet-4-5-20250929".to_string()),
            }),
            ClaudeRawEvent::Assistant(ClaudeAssistantEvent {
                message: Some(ClaudeMessageObject {
                    model: Some("<synthetic>".to_string()),
                    id: Some("test-id".to_string()),
                    role: Some("assistant".to_string()),
                    content: Some(vec![ClaudeContentBlock::Text {
                        text: "Invalid API key · Please run /login".to_string(),
                    }]),
                    stop_reason: Some("stop_sequence".to_string()),
                    usage: None,
                }),
                text: None,
                session_id: Some("test-session".to_string()),
                error: Some("authentication_failed".to_string()),
            }),
            ClaudeRawEvent::Result(ClaudeResultEvent {
                result: Some("Invalid API key · Please run /login".to_string()),
                output: None,
                session_id: Some("test-session".to_string()),
                usage: Some(ClaudeUsage {
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                }),
            }),
        ];

        let all_events: Vec<AgentEvent> = raw_events
            .into_iter()
            .flat_map(ClaudeCodeRunner::convert_event)
            .collect();

        // Should produce: SessionInit, Error, TurnCompleted
        // (no AssistantMessage - the error replaces it)
        assert_eq!(all_events.len(), 3);
        assert!(matches!(all_events[0], AgentEvent::SessionInit(_)));
        assert!(matches!(all_events[1], AgentEvent::Error(_)));
        assert!(matches!(all_events[2], AgentEvent::TurnCompleted(_)));

        // Verify the error message contains helpful instructions
        if let AgentEvent::Error(err) = &all_events[1] {
            assert!(err.is_fatal, "Auth failure should be fatal");
            assert!(
                err.message.contains("claude") || err.message.contains("login"),
                "Error should mention how to login: {}",
                err.message
            );
        }
    }

    /// Test that auth failure assistant event produces Error instead of AssistantMessage
    #[test]
    fn test_convert_auth_failure_produces_error_event() {
        let raw = ClaudeRawEvent::Assistant(ClaudeAssistantEvent {
            message: Some(ClaudeMessageObject {
                model: Some("<synthetic>".to_string()),
                id: Some("test-id".to_string()),
                role: Some("assistant".to_string()),
                content: Some(vec![ClaudeContentBlock::Text {
                    text: "Invalid API key · Please run /login".to_string(),
                }]),
                stop_reason: Some("stop_sequence".to_string()),
                usage: None,
            }),
            text: None,
            session_id: Some("test-session".to_string()),
            error: Some("authentication_failed".to_string()),
        });

        let events = ClaudeCodeRunner::convert_event(raw);
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgentEvent::Error(err) => {
                assert!(err.is_fatal);
                assert!(
                    err.message.contains("Authentication failed"),
                    "Should mention auth failed: {}",
                    err.message
                );
                assert!(
                    err.message.contains("claude") && err.message.contains("/login"),
                    "Should tell user how to fix: {}",
                    err.message
                );
            }
            other => panic!("Expected Error event, got {:?}", other),
        }
    }

    /// Test that normal assistant events (no error field) still work
    #[test]
    fn test_normal_assistant_event_still_produces_message() {
        let raw = ClaudeRawEvent::Assistant(ClaudeAssistantEvent {
            message: Some(ClaudeMessageObject {
                model: Some("claude-sonnet".to_string()),
                id: Some("test-id".to_string()),
                role: Some("assistant".to_string()),
                content: Some(vec![ClaudeContentBlock::Text {
                    text: "Hello! How can I help?".to_string(),
                }]),
                stop_reason: Some("end_turn".to_string()),
                usage: None,
            }),
            text: None,
            session_id: Some("test-session".to_string()),
            error: None,
        });

        let events = ClaudeCodeRunner::convert_event(raw);
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgentEvent::AssistantMessage(msg) => {
                assert_eq!(msg.text, "Hello! How can I help?");
            }
            other => panic!("Expected AssistantMessage, got {:?}", other),
        }
    }

    /// Helper to extract command args for testing
    fn get_command_args(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect()
    }

    /// Test that a prompt starting with "-" is properly escaped with "--" separator
    #[test]
    fn test_prompt_starting_with_dash_is_escaped() {
        let runner = ClaudeCodeRunner {
            binary_path: PathBuf::from("/usr/bin/claude"),
        };
        let config = AgentStartConfig::new(
            "- [ ] Try to understand the source of this error",
            PathBuf::from("/tmp"),
        );

        let cmd = runner.build_command(&config);
        let args = get_command_args(&cmd);

        // Find the position of "--" and verify the prompt comes after it
        let double_dash_pos = args.iter().position(|a| a == "--");
        assert!(
            double_dash_pos.is_some(),
            "Command should contain '--' separator. Args: {:?}",
            args
        );

        let prompt_pos = args
            .iter()
            .position(|a| a == "- [ ] Try to understand the source of this error");
        assert!(
            prompt_pos.is_some(),
            "Command should contain the prompt. Args: {:?}",
            args
        );

        assert!(
            prompt_pos.unwrap() > double_dash_pos.unwrap(),
            "Prompt should come after '--' separator. Args: {:?}",
            args
        );
    }

    /// Test that normal prompts (not starting with dash) still work
    #[test]
    fn test_prompt_without_dash_still_works() {
        let runner = ClaudeCodeRunner {
            binary_path: PathBuf::from("/usr/bin/claude"),
        };
        let config = AgentStartConfig::new("Hello, can you help me?", PathBuf::from("/tmp"));

        let cmd = runner.build_command(&config);
        let args = get_command_args(&cmd);

        // Should still contain "--" for consistency
        assert!(
            args.contains(&"--".to_string()),
            "Command should contain '--' separator. Args: {:?}",
            args
        );
        assert!(
            args.contains(&"Hello, can you help me?".to_string()),
            "Command should contain the prompt. Args: {:?}",
            args
        );
    }

    /// Test that resume session with dash-prefixed prompt works
    #[test]
    fn test_resume_session_with_dash_prompt() {
        let runner = ClaudeCodeRunner {
            binary_path: PathBuf::from("/usr/bin/claude"),
        };
        let config = AgentStartConfig::new("- continue with this task", PathBuf::from("/tmp"))
            .with_resume(SessionId::from_string("session-123".to_string()));

        let cmd = runner.build_command(&config);
        let args = get_command_args(&cmd);

        // Check command structure includes --resume, --, and prompt in correct order
        let resume_pos = args.iter().position(|a| a == "--resume");
        let double_dash_pos = args.iter().position(|a| a == "--");
        let prompt_pos = args.iter().position(|a| a == "- continue with this task");

        assert!(
            resume_pos.is_some(),
            "Should contain '--resume'. Args: {:?}",
            args
        );
        assert!(
            double_dash_pos.is_some(),
            "Should contain '--'. Args: {:?}",
            args
        );
        assert!(
            prompt_pos.is_some(),
            "Should contain prompt. Args: {:?}",
            args
        );

        // Prompt should come after both --resume and --
        assert!(
            prompt_pos.unwrap() > double_dash_pos.unwrap(),
            "Prompt should come after '--'. Args: {:?}",
            args
        );
        assert!(
            double_dash_pos.unwrap() > resume_pos.unwrap(),
            "'--' should come after '--resume'. Args: {:?}",
            args
        );
    }

    /// Test that empty prompt doesn't add "--" separator
    #[test]
    fn test_empty_prompt_no_separator() {
        let runner = ClaudeCodeRunner {
            binary_path: PathBuf::from("/usr/bin/claude"),
        };
        let config = AgentStartConfig::new("", PathBuf::from("/tmp"));

        let cmd = runner.build_command(&config);
        let args = get_command_args(&cmd);

        // Should NOT contain "--" when prompt is empty
        assert!(
            !args.contains(&"--".to_string()),
            "Command should NOT contain '--' separator when prompt is empty. Args: {:?}",
            args
        );
    }
}
