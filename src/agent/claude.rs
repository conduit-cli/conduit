use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, SessionInitEvent, TokenUsage, ToolCompletedEvent,
    ToolStartedEvent, TurnCompletedEvent,
};
use crate::agent::runner::{AgentHandle, AgentRunner, AgentStartConfig, AgentType};
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

    fn find_binary() -> Option<PathBuf> {
        which::which("claude").ok()
    }

    fn build_command(&self, config: &AgentStartConfig) -> Command {
        let mut cmd = Command::new(&self.binary_path);

        // Core headless mode flags
        cmd.arg("-p").arg(&config.prompt);
        cmd.arg("--output-format").arg("stream-json");

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

        // Additional args
        for arg in &config.additional_args {
            cmd.arg(arg);
        }

        // Stdio setup for JSONL capture
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        cmd
    }

    /// Convert Claude-specific event to unified AgentEvent(s)
    /// Returns a Vec because Assistant events can contain both text and tool_use blocks
    fn convert_event(raw: ClaudeRawEvent) -> Vec<AgentEvent> {
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
                let tool_id = result.tool_use_id.unwrap_or_default();
                let is_error = result.is_error.unwrap_or(false);
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
            ClaudeRawEvent::Unknown => vec![],
        }
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

        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;
        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;

        let (tx, rx) = mpsc::channel::<AgentEvent>(256);

        // Spawn JSONL parser task
        tokio::spawn(async move {
            let (raw_tx, mut raw_rx) = mpsc::channel::<ClaudeRawEvent>(256);

            // Parse raw events
            let parse_handle = tokio::spawn(async move {
                let _ = JsonlStreamParser::parse_stream(stdout, raw_tx).await;
            });

            // Convert and forward events
            'outer: while let Some(raw_event) = raw_rx.recv().await {
                for event in Self::convert_event(raw_event) {
                    if tx.send(event).await.is_err() {
                        break 'outer;
                    }
                }
            }

            let _ = parse_handle.await;
        });

        // Monitor process exit
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        Ok(AgentHandle::new(rx, pid))
    }

    async fn send_input(&self, _handle: &AgentHandle, _input: &str) -> Result<(), AgentError> {
        // Claude Code headless mode doesn't support interactive input
        Err(AgentError::NotSupported(
            "Claude headless mode doesn't support interactive input".into(),
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
