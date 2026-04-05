use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};

use crate::agent::error::AgentError;
use crate::agent::events::{
    AgentEvent, AssistantMessageEvent, CommandOutputEvent, ErrorEvent, ReasoningEvent,
    SessionInitEvent, TokenUsage, ToolCompletedEvent, ToolStartedEvent, TurnCompletedEvent,
    TurnFailedEvent,
};
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use crate::agent::session::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiModelCatalogEntry {
    pub provider: String,
    pub model_id: String,
    pub context_window: i64,
}

impl PiModelCatalogEntry {
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider, self.model_id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PiModelCatalog {
    pub current_model_id: Option<String>,
    pub models: Vec<PiModelCatalogEntry>,
}

pub struct PiRunner {
    binary_path: PathBuf,
}

impl Default for PiRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl PiRunner {
    const DEFAULT_MODEL_ID: &str = "default";
    const SUPPORTED_TOOLS: [&str; 7] = ["read", "bash", "edit", "write", "grep", "find", "ls"];

    pub fn new() -> Self {
        Self {
            binary_path: Self::find_binary().unwrap_or_else(|| PathBuf::from("pi")),
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { binary_path: path }
    }

    fn find_binary() -> Option<PathBuf> {
        which::which("pi").ok()
    }

    pub fn load_model_catalog(binary_path: Option<PathBuf>) -> PiModelCatalog {
        let binary = binary_path
            .filter(|path| path.exists())
            .or_else(Self::find_binary);
        let Some(binary) = binary else {
            return PiModelCatalog::default();
        };

        let mut catalog = match Self::discover_model_catalog(&binary) {
            Ok(catalog) => catalog,
            Err(err) => {
                tracing::debug!(error = %err, binary = %binary.display(), "Failed to discover Pi models");
                PiModelCatalog::default()
            }
        };

        if let Some(current_model_id) = catalog.current_model_id.clone() {
            let already_present = catalog
                .models
                .iter()
                .any(|model| model.full_id() == current_model_id);
            if !already_present {
                if let Some((provider, model_id)) = current_model_id.split_once('/') {
                    catalog.models.push(PiModelCatalogEntry {
                        provider: provider.to_string(),
                        model_id: model_id.to_string(),
                        context_window: 200_000,
                    });
                }
            }
        }

        catalog.models.sort_by_key(PiModelCatalogEntry::full_id);
        catalog.models.dedup_by(|left, right| {
            left.provider == right.provider && left.model_id == right.model_id
        });
        catalog
    }

    fn discover_model_catalog(binary: &Path) -> std::io::Result<PiModelCatalog> {
        Ok(PiModelCatalog {
            current_model_id: Self::discover_current_model(binary)?,
            models: Self::discover_available_models(binary)?,
        })
    }

    fn discover_available_models(binary: &Path) -> std::io::Result<Vec<PiModelCatalogEntry>> {
        let output = StdCommand::new(binary).arg("--list-models").output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "pi --list-models exited with status {:?}",
                output.status.code()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let text = if stdout.trim().is_empty() {
            stderr.as_ref()
        } else {
            stdout.as_ref()
        };
        Ok(Self::parse_list_models_output(text))
    }

    fn discover_current_model(binary: &Path) -> std::io::Result<Option<String>> {
        let mut child = StdCommand::new(binary)
            .args(["--mode", "rpc", "--no-session"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(
                br#"{"id":"state-1","type":"get_state"}
"#,
            )?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "pi --mode rpc --no-session exited with status {:?}",
                output.status.code()
            )));
        }

        Ok(Self::parse_current_model_output(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    fn parse_list_models_output(output: &str) -> Vec<PiModelCatalogEntry> {
        output
            .lines()
            .skip_while(|line| line.trim().is_empty())
            .skip(1)
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let provider = parts.next()?;
                let model_id = parts.next()?;
                let context_window = parts
                    .next()
                    .and_then(Self::parse_token_count)
                    .unwrap_or(200_000);
                Some(PiModelCatalogEntry {
                    provider: provider.to_string(),
                    model_id: model_id.to_string(),
                    context_window,
                })
            })
            .collect()
    }

    fn parse_current_model_output(output: &str) -> Option<String> {
        output.lines().find_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            if value.get("type").and_then(Value::as_str) != Some("response")
                || value.get("command").and_then(Value::as_str) != Some("get_state")
                || value.get("success").and_then(Value::as_bool) != Some(true)
            {
                return None;
            }

            let model = value.get("data")?.get("model")?;
            let provider = model.get("provider")?.as_str()?;
            let model_id = model.get("id")?.as_str()?;
            Some(format!("{provider}/{model_id}"))
        })
    }

    fn parse_token_count(value: &str) -> Option<i64> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        let suffix = trimmed.chars().last()?;
        let multiplier = match suffix {
            'K' | 'k' => 1_000_f64,
            'M' | 'm' => 1_000_000_f64,
            _ => return trimmed.parse::<i64>().ok(),
        };
        let number = trimmed[..trimmed.len().saturating_sub(1)]
            .parse::<f64>()
            .ok()?;
        Some((number * multiplier).round() as i64)
    }

    fn build_command(&self, config: &AgentStartConfig) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("--mode").arg("rpc");
        cmd.current_dir(&config.working_dir);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        if let Some(session_id) = &config.resume_session {
            cmd.arg("--session").arg(session_id.as_str());
        }
        if let Some(model) = &config.model {
            if !model.trim().eq_ignore_ascii_case(Self::DEFAULT_MODEL_ID) {
                cmd.arg("--model").arg(model);
            }
        }
        if let Some(effort) = config.reasoning_effort {
            cmd.arg("--thinking").arg(effort.as_str());
        }
        let tools = config
            .allowed_tools
            .iter()
            .map(|tool| tool.trim().to_ascii_lowercase())
            .filter(|tool| Self::SUPPORTED_TOOLS.contains(&tool.as_str()))
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            cmd.arg("--tools").arg(tools.join(","));
        }
        for arg in &config.additional_args {
            cmd.arg(arg);
        }

        cmd
    }

    fn parse_event_line(line: &str) -> Result<Vec<AgentEvent>, AgentError> {
        let value: Value = serde_json::from_str(line)?;
        Ok(Self::parse_event_value(&value))
    }

    fn parse_event_value(value: &Value) -> Vec<AgentEvent> {
        match value.get("type").and_then(Value::as_str) {
            Some("response") => Self::parse_response(value),
            Some("turn_start") => vec![AgentEvent::TurnStarted],
            Some("turn_end") => Self::parse_turn_end(value),
            Some("message_update") => Self::parse_message_update(value),
            Some("message_end") => Self::parse_message_end(value),
            Some("tool_execution_start") => Self::parse_tool_execution_start(value),
            Some("tool_execution_update") => Self::parse_tool_execution_update(value),
            Some("tool_execution_end") => Self::parse_tool_execution_end(value),
            Some("extension_error") => vec![AgentEvent::Error(ErrorEvent {
                message: value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Pi extension error")
                    .to_string(),
                is_fatal: false,
                code: Some("extension_error".to_string()),
                details: Some(value.clone()),
            })],
            Some("agent_end") => Vec::new(),
            _ => Vec::new(),
        }
    }

    fn parse_response(value: &Value) -> Vec<AgentEvent> {
        let success = value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let command = value.get("command").and_then(Value::as_str).unwrap_or("");
        if !success {
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Pi RPC command failed")
                .to_string();
            return vec![
                AgentEvent::Error(ErrorEvent {
                    message: message.clone(),
                    is_fatal: true,
                    code: Some(format!("pi_response_{command}")),
                    details: Some(value.clone()),
                }),
                AgentEvent::TurnFailed(TurnFailedEvent { error: message }),
            ];
        }

        if command != "get_state" {
            return Vec::new();
        }

        let data = match value.get("data") {
            Some(data) => data,
            None => return Vec::new(),
        };
        let session_ref = data
            .get("sessionFile")
            .and_then(Value::as_str)
            .or_else(|| data.get("sessionId").and_then(Value::as_str));
        let Some(session_ref) = session_ref else {
            return Vec::new();
        };
        let model = data
            .get("model")
            .and_then(|model| model.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        vec![AgentEvent::SessionInit(SessionInitEvent {
            session_id: SessionId::from_string(session_ref),
            model,
        })]
    }

    fn parse_turn_end(value: &Value) -> Vec<AgentEvent> {
        let message = value.get("message");
        let usage = message.map(Self::extract_usage).unwrap_or_default();
        let mut events = vec![AgentEvent::TurnCompleted(TurnCompletedEvent { usage })];

        if let Some(message) = message {
            let stop_reason = message.get("stopReason").and_then(Value::as_str);
            if matches!(stop_reason, Some("error") | Some("aborted")) {
                let error = message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .unwrap_or("Pi turn failed")
                    .to_string();
                events.push(AgentEvent::TurnFailed(TurnFailedEvent { error }));
            }
        }

        events
    }

    fn parse_message_update(value: &Value) -> Vec<AgentEvent> {
        let Some(delta) = value.get("assistantMessageEvent") else {
            return Vec::new();
        };
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => delta
                .get("delta")
                .and_then(Value::as_str)
                .map(|text| {
                    vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
                        text: text.to_string(),
                        is_final: false,
                    })]
                })
                .unwrap_or_default(),
            Some("thinking_delta") => delta
                .get("delta")
                .and_then(Value::as_str)
                .map(|text| {
                    vec![AgentEvent::AssistantReasoning(ReasoningEvent {
                        text: text.to_string(),
                    })]
                })
                .unwrap_or_default(),
            Some("error") => {
                let message = delta
                    .get("error")
                    .and_then(Value::as_str)
                    .or_else(|| delta.get("reason").and_then(Value::as_str))
                    .unwrap_or("Pi message error")
                    .to_string();
                vec![AgentEvent::Error(ErrorEvent {
                    message,
                    is_fatal: true,
                    code: Some("message_update_error".to_string()),
                    details: Some(value.clone()),
                })]
            }
            _ => Vec::new(),
        }
    }

    fn parse_message_end(value: &Value) -> Vec<AgentEvent> {
        let Some(message) = value.get("message") else {
            return Vec::new();
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return Vec::new();
        }

        let text = message
            .get("content")
            .map(Self::extract_text_from_message_content)
            .filter(|text| !text.is_empty())
            .or_else(|| {
                message
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|text| !text.is_empty())
            })
            .unwrap_or_default();

        vec![AgentEvent::AssistantMessage(AssistantMessageEvent {
            text,
            is_final: true,
        })]
    }

    fn extract_text_from_message_content(content: &Value) -> String {
        match content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }

    fn parse_tool_execution_start(value: &Value) -> Vec<AgentEvent> {
        let tool_name = value
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let tool_id = value
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let arguments = value.get("args").cloned().unwrap_or(Value::Null);
        vec![AgentEvent::ToolStarted(ToolStartedEvent {
            tool_name,
            tool_id,
            arguments,
        })]
    }

    fn parse_tool_execution_update(value: &Value) -> Vec<AgentEvent> {
        let tool_name = value.get("toolName").and_then(Value::as_str).unwrap_or("");
        if tool_name != "bash" {
            return Vec::new();
        }
        let output = value
            .get("partialResult")
            .and_then(Self::extract_text_from_tool_result)
            .unwrap_or_default();
        let command = value
            .get("args")
            .and_then(|args| args.get("command"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        vec![AgentEvent::CommandOutput(CommandOutputEvent {
            command,
            output,
            exit_code: None,
            is_streaming: true,
        })]
    }

    fn parse_tool_execution_end(value: &Value) -> Vec<AgentEvent> {
        let tool_name = value
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let tool_id = value
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_error = value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let result_text = value
            .get("result")
            .and_then(Self::extract_text_from_tool_result);
        let mut events = vec![AgentEvent::ToolCompleted(ToolCompletedEvent {
            tool_id,
            success: !is_error,
            result: if is_error { None } else { result_text.clone() },
            error: if is_error { result_text.clone() } else { None },
        })];

        if tool_name == "bash" {
            let command = value
                .get("args")
                .and_then(|args| args.get("command"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            events.push(AgentEvent::CommandOutput(CommandOutputEvent {
                command,
                output: result_text.unwrap_or_default(),
                exit_code: None,
                is_streaming: false,
            }));
        }

        events
    }

    fn extract_usage(message: &Value) -> TokenUsage {
        let Some(usage) = message.get("usage") else {
            return TokenUsage::default();
        };
        TokenUsage {
            input_tokens: usage.get("input").and_then(Value::as_i64).unwrap_or(0),
            output_tokens: usage.get("output").and_then(Value::as_i64).unwrap_or(0),
            cached_tokens: usage.get("cacheRead").and_then(Value::as_i64).unwrap_or(0),
            total_tokens: usage
                .get("totalTokens")
                .and_then(Value::as_i64)
                .unwrap_or_else(|| {
                    usage.get("input").and_then(Value::as_i64).unwrap_or(0)
                        + usage.get("output").and_then(Value::as_i64).unwrap_or(0)
                }),
        }
    }

    fn extract_text_from_tool_result(value: &Value) -> Option<String> {
        let content = value.get("content")?.as_array()?;
        let text = content
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        Some(text)
    }

    async fn write_command_line(
        stdin: &Arc<Mutex<tokio::process::ChildStdin>>,
        payload: Value,
    ) -> Result<(), AgentError> {
        let mut guard = stdin.lock().await;
        let line = serde_json::to_string(&payload)?;
        guard.write_all(line.as_bytes()).await?;
        guard.write_all(b"\n").await?;
        guard.flush().await?;
        Ok(())
    }

    fn image_payload(image: &PathBuf) -> Result<Value, AgentError> {
        let bytes = std::fs::read(image)?;
        let mime_type = mime_guess::from_path(image)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        Ok(json!({
            "type": "image",
            "data": BASE64_STANDARD.encode(bytes),
            "mimeType": mime_type,
        }))
    }

    fn build_message_payload(
        command_type: &str,
        text: String,
        images: &[PathBuf],
    ) -> Result<Value, AgentError> {
        if images.is_empty() {
            return Ok(json!({ "type": command_type, "message": text }));
        }

        let images = images
            .iter()
            .map(Self::image_payload)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({
            "type": command_type,
            "message": text,
            "images": images,
        }))
    }

    fn build_prompt_payload(
        text: String,
        images: &[PathBuf],
        busy: bool,
    ) -> Result<Value, AgentError> {
        let command_type = if busy { "steer" } else { "prompt" };
        Self::build_message_payload(command_type, text, images)
    }

    async fn send_prompt(
        stdin: &Arc<Mutex<tokio::process::ChildStdin>>,
        text: String,
        images: &[PathBuf],
        busy: &Arc<AtomicBool>,
    ) -> Result<(), AgentError> {
        let payload = Self::build_prompt_payload(text, images, busy.load(Ordering::SeqCst))?;
        Self::write_command_line(stdin, payload).await
    }

    async fn send_follow_up(
        stdin: &Arc<Mutex<tokio::process::ChildStdin>>,
        text: String,
        images: &[PathBuf],
    ) -> Result<(), AgentError> {
        let payload = Self::build_message_payload("follow_up", text, images)?;
        Self::write_command_line(stdin, payload).await
    }

    #[cfg(unix)]
    fn signal_pid(pid: u32, signal: i32) -> Result<(), AgentError> {
        let result = unsafe { libc::kill(pid as i32, signal) };
        if result == -1 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(AgentError::Io(err));
        }
        Ok(())
    }
}

#[async_trait]
impl AgentRunner for PiRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Pi
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        let mut child = self.build_command(&config).spawn()?;
        let pid = child.id().ok_or(AgentError::ProcessSpawnFailed)?;
        let stdin = child.stdin.take().ok_or(AgentError::ProcessSpawnFailed)?;
        let stdout = child.stdout.take().ok_or(AgentError::StdoutCaptureFailed)?;
        let stderr = child.stderr.take().ok_or(AgentError::StdoutCaptureFailed)?;

        let stdin = Arc::new(Mutex::new(stdin));
        let busy = Arc::new(AtomicBool::new(false));
        let (event_tx, event_rx) = mpsc::channel(512);
        let (input_tx, mut input_rx) = mpsc::channel(32);

        Self::write_command_line(&stdin, json!({ "id": "state-1", "type": "get_state" })).await?;
        if !config.prompt.trim().is_empty() || !config.images.is_empty() {
            Self::send_prompt(&stdin, config.prompt.clone(), &config.images, &busy).await?;
        }

        let stdout_busy = busy.clone();
        let stdout_tx = event_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        if let Ok(value) = serde_json::from_str::<Value>(line) {
                            match value.get("type").and_then(Value::as_str) {
                                Some("agent_start") | Some("turn_start") => {
                                    stdout_busy.store(true, Ordering::SeqCst);
                                }
                                Some("agent_end") => {
                                    stdout_busy.store(false, Ordering::SeqCst);
                                }
                                _ => {}
                            }
                        }
                        match Self::parse_event_line(line) {
                            Ok(events) => {
                                for event in events {
                                    if stdout_tx.send(event).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(_) => {
                                if stdout_tx
                                    .send(AgentEvent::Error(ErrorEvent {
                                        message: format!("Failed to parse Pi event: {line}"),
                                        is_fatal: false,
                                        code: Some("pi_parse_error".to_string()),
                                        details: None,
                                    }))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(err) => {
                        let _ = stdout_tx
                            .send(AgentEvent::Error(ErrorEvent {
                                message: format!("Failed reading Pi output: {err}"),
                                is_fatal: true,
                                code: Some("pi_stdout_error".to_string()),
                                details: None,
                            }))
                            .await;
                        break;
                    }
                }
            }
        });

        let stderr_tx = event_tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if stderr_tx
                    .send(AgentEvent::Error(ErrorEvent {
                        message: format!("Pi stderr: {trimmed}"),
                        is_fatal: false,
                        code: Some("pi_stderr".to_string()),
                        details: None,
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let input_stdin = stdin.clone();
        let input_busy = busy.clone();
        let input_event_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                match input {
                    AgentInput::CodexPrompt { text, images, .. } => {
                        if let Err(err) =
                            Self::send_prompt(&input_stdin, text, &images, &input_busy).await
                        {
                            if input_event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: err.to_string(),
                                    is_fatal: true,
                                    code: Some("pi_send_input".to_string()),
                                    details: None,
                                }))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    AgentInput::PiSetThinkingLevel { level } => {
                        if let Err(err) = Self::write_command_line(
                            &input_stdin,
                            json!({ "type": "set_thinking_level", "level": level.as_str() }),
                        )
                        .await
                        {
                            if input_event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: err.to_string(),
                                    is_fatal: true,
                                    code: Some("pi_set_thinking_level".to_string()),
                                    details: None,
                                }))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    AgentInput::PiFollowUp { text, images } => {
                        if let Err(err) = Self::send_follow_up(&input_stdin, text, &images).await {
                            if input_event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: err.to_string(),
                                    is_fatal: true,
                                    code: Some("pi_follow_up".to_string()),
                                    details: None,
                                }))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    AgentInput::PiSetFollowUpMode { mode } => {
                        if let Err(err) = Self::write_command_line(
                            &input_stdin,
                            json!({ "type": "set_follow_up_mode", "mode": mode.as_str() }),
                        )
                        .await
                        {
                            if input_event_tx
                                .send(AgentEvent::Error(ErrorEvent {
                                    message: err.to_string(),
                                    is_fatal: true,
                                    code: Some("pi_set_follow_up_mode".to_string()),
                                    details: None,
                                }))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    AgentInput::ClaudeJsonl(_) | AgentInput::OpencodeQuestion { .. } => {
                        if input_event_tx
                            .send(AgentEvent::Error(ErrorEvent {
                                message: "Unsupported input for Pi runner".to_string(),
                                is_fatal: false,
                                code: Some("pi_input_unsupported".to_string()),
                                details: None,
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            match child.wait().await {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    let code = status.code();
                    let _ = event_tx
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("Pi exited with status {:?}", code),
                            is_fatal: true,
                            code: Some("pi_exit".to_string()),
                            details: None,
                        }))
                        .await;
                    if let Some(code) = code {
                        let _ = event_tx
                            .send(AgentEvent::TurnFailed(TurnFailedEvent {
                                error: format!("Pi exited with status {code}"),
                            }))
                            .await;
                    }
                }
                Err(err) => {
                    let _ = event_tx
                        .send(AgentEvent::Error(ErrorEvent {
                            message: format!("Failed waiting for Pi process: {err}"),
                            is_fatal: true,
                            code: Some("pi_wait_error".to_string()),
                            details: None,
                        }))
                        .await;
                }
            }
        });

        Ok(AgentHandle::new(event_rx, pid, Some(input_tx)))
    }

    async fn send_input(&self, handle: &AgentHandle, input: AgentInput) -> Result<(), AgentError> {
        let sender = handle.input_tx.as_ref().ok_or(AgentError::ChannelClosed)?;
        sender
            .send(input)
            .await
            .map_err(|_| AgentError::ChannelClosed)
    }

    async fn stop(&self, handle: &AgentHandle) -> Result<(), AgentError> {
        #[cfg(unix)]
        {
            return Self::signal_pid(handle.pid, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = handle;
            Ok(())
        }
    }

    async fn kill(&self, handle: &AgentHandle) -> Result<(), AgentError> {
        #[cfg(unix)]
        {
            return Self::signal_pid(handle.pid, libc::SIGKILL);
        }
        #[cfg(not(unix))]
        {
            let _ = handle;
            Ok(())
        }
    }

    fn is_available(&self) -> bool {
        self.binary_path.exists() || which::which(&self.binary_path).is_ok()
    }

    fn binary_path(&self) -> Option<PathBuf> {
        Some(self.binary_path.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::PiRunner;
    use crate::agent::events::AgentEvent;
    use crate::agent::{AgentStartConfig, PiFollowUpMode, ReasoningEffort, SessionId};

    fn command_args(command: &tokio::process::Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn build_command_includes_resume_model_thinking_and_tools() {
        let runner = PiRunner::with_path(PathBuf::from("/usr/bin/pi"));
        let config = AgentStartConfig::new("hello", PathBuf::from("/tmp"))
            .with_resume(SessionId::from_string("/tmp/pi-session.jsonl"))
            .with_model("anthropic/claude-sonnet-4")
            .with_reasoning_effort(ReasoningEffort::XHigh)
            .with_tools(vec!["Bash".to_string(), "Read".to_string()]);

        let command = runner.build_command(&config);
        let args = command_args(&command);

        assert_eq!(
            args,
            vec![
                "--mode",
                "rpc",
                "--session",
                "/tmp/pi-session.jsonl",
                "--model",
                "anthropic/claude-sonnet-4",
                "--thinking",
                "xhigh",
                "--tools",
                "bash,read",
            ]
        );
    }

    #[test]
    fn build_command_omits_pi_default_model_argument() {
        let runner = PiRunner::with_path(PathBuf::from("/usr/bin/pi"));
        let config = AgentStartConfig::new("hello", PathBuf::from("/tmp"))
            .with_model("default")
            .with_tools(vec!["Read".to_string()]);

        let command = runner.build_command(&config);
        let args = command_args(&command);

        assert_eq!(args, vec!["--mode", "rpc", "--tools", "read"]);
    }

    #[test]
    fn build_command_filters_unsupported_tools() {
        let runner = PiRunner::with_path(PathBuf::from("/usr/bin/pi"));
        let config = AgentStartConfig::new("hello", PathBuf::from("/tmp")).with_tools(vec![
            "Bash".to_string(),
            "Glob".to_string(),
            "Read".to_string(),
            "Unknown".to_string(),
            "Find".to_string(),
        ]);

        let command = runner.build_command(&config);
        let args = command_args(&command);

        assert_eq!(args, vec!["--mode", "rpc", "--tools", "bash,read,find"]);
    }

    #[test]
    fn build_command_supports_off_thinking() {
        let runner = PiRunner::with_path(PathBuf::from("/usr/bin/pi"));
        let config = AgentStartConfig::new("hello", PathBuf::from("/tmp"))
            .with_reasoning_effort(ReasoningEffort::Off);

        let command = runner.build_command(&config);
        let args = command_args(&command);

        assert!(args.windows(2).any(|pair| pair == ["--thinking", "off"]));
    }

    #[test]
    fn build_prompt_payload_includes_images() {
        let tmp = tempfile::Builder::new()
            .prefix("conduit-pi-image-")
            .suffix(".png")
            .tempfile()
            .expect("failed to create temp image");
        let path = tmp.path().to_path_buf();
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .save(&path)
            .expect("failed to write temp image");

        let payload = PiRunner::build_prompt_payload("describe".to_string(), &[path], false)
            .expect("payload should build");

        assert_eq!(payload["type"], "prompt");
        assert_eq!(payload["message"], "describe");
        assert_eq!(payload["images"].as_array().map(Vec::len), Some(1));
        assert_eq!(payload["images"][0]["type"], "image");
        assert_eq!(payload["images"][0]["mimeType"], "image/png");
        assert!(payload["images"][0]["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()));
    }

    #[test]
    fn build_message_payload_supports_follow_up() {
        let payload = PiRunner::build_message_payload("follow_up", "later".to_string(), &[])
            .expect("payload should build");

        assert_eq!(payload["type"], "follow_up");
        assert_eq!(payload["message"], "later");
        assert!(payload.get("images").is_none());
    }

    #[test]
    fn build_set_follow_up_mode_payload() {
        let payload = json!({
            "type": "set_follow_up_mode",
            "mode": PiFollowUpMode::OneAtATime.as_str(),
        });

        assert_eq!(payload["type"], "set_follow_up_mode");
        assert_eq!(payload["mode"], "one-at-a-time");
    }

    #[test]
    fn response_failure_emits_error_and_turn_failed() {
        let events = PiRunner::parse_event_value(&json!({
            "type": "response",
            "command": "set_thinking_level",
            "success": false,
            "error": "thinking unsupported"
        }));

        assert!(matches!(
            &events[0],
            AgentEvent::Error(error)
                if error.message == "thinking unsupported"
                    && error.is_fatal
                    && error.code.as_deref() == Some("pi_response_set_thinking_level")
        ));
        assert!(matches!(
            &events[1],
            AgentEvent::TurnFailed(failed) if failed.error == "thinking unsupported"
        ));
    }

    #[test]
    fn parses_get_state_response_into_session_init() {
        let events = PiRunner::parse_event_value(&json!({
            "type": "response",
            "command": "get_state",
            "success": true,
            "data": {
                "sessionFile": "/tmp/pi-session.jsonl",
                "sessionId": "session-123",
                "model": { "id": "anthropic/claude-sonnet-4-20250514" }
            }
        }));

        assert!(matches!(
            &events[0],
            AgentEvent::SessionInit(init)
                if init.session_id.as_str() == "/tmp/pi-session.jsonl"
                    && init.model.as_deref() == Some("anthropic/claude-sonnet-4-20250514")
        ));
    }

    #[test]
    fn parses_message_update_deltas() {
        let text_events = PiRunner::parse_event_value(&json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "type": "text_delta",
                "delta": "Hello"
            }
        }));
        let thinking_events = PiRunner::parse_event_value(&json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "type": "thinking_delta",
                "delta": "Analyzing"
            }
        }));

        assert!(matches!(
            &text_events[0],
            AgentEvent::AssistantMessage(message) if message.text == "Hello" && !message.is_final
        ));
        assert!(matches!(
            &thinking_events[0],
            AgentEvent::AssistantReasoning(reasoning) if reasoning.text == "Analyzing"
        ));
    }

    #[test]
    fn parses_tool_events_and_turn_completion() {
        let started = PiRunner::parse_event_value(&json!({
            "type": "tool_execution_start",
            "toolCallId": "call-1",
            "toolName": "bash",
            "args": { "command": "ls" }
        }));
        let completed = PiRunner::parse_event_value(&json!({
            "type": "tool_execution_end",
            "toolCallId": "call-1",
            "toolName": "bash",
            "args": { "command": "ls" },
            "result": {
                "content": [{ "type": "text", "text": "file.txt\n" }]
            },
            "isError": false
        }));
        let turn = PiRunner::parse_event_value(&json!({
            "type": "turn_end",
            "message": {
                "usage": {
                    "input": 12,
                    "output": 8,
                    "cacheRead": 3,
                    "totalTokens": 20
                },
                "stopReason": "stop"
            }
        }));

        assert!(matches!(
            &started[0],
            AgentEvent::ToolStarted(tool)
                if tool.tool_name == "bash"
                    && tool.tool_id == "call-1"
                    && tool.arguments["command"] == "ls"
        ));
        assert!(completed
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCompleted(tool) if tool.tool_id == "call-1" && tool.success)));
        assert!(completed.iter().any(|event| matches!(
            event,
            AgentEvent::CommandOutput(output) if output.command == "ls" && output.output == "file.txt\n" && !output.is_streaming
        )));
        assert!(matches!(
            &turn[0],
            AgentEvent::TurnCompleted(done)
                if done.usage.input_tokens == 12
                    && done.usage.output_tokens == 8
                    && done.usage.cached_tokens == 3
                    && done.usage.total_tokens == 20
        ));
    }

    #[test]
    fn parses_jsonl_lines() {
        let events = PiRunner::parse_event_line(
            r#"{"type":"message_end","message":{"role":"assistant","content":[]}}"#,
        )
        .expect("line should parse");

        assert!(matches!(
            &events[0],
            AgentEvent::AssistantMessage(message) if message.is_final && message.text.is_empty()
        ));
    }

    #[test]
    fn parses_message_end_text_from_content_blocks() {
        let events = PiRunner::parse_event_value(&json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "Yes — everything appears committed." }
                ]
            }
        }));

        assert!(matches!(
            &events[0],
            AgentEvent::AssistantMessage(message)
                if message.is_final && message.text == "Yes — everything appears committed."
        ));
    }

    #[test]
    fn parses_message_end_text_field_when_content_missing() {
        let events = PiRunner::parse_event_value(&json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "text": "Final answer"
            }
        }));

        assert!(matches!(
            &events[0],
            AgentEvent::AssistantMessage(message)
                if message.is_final && message.text == "Final answer"
        ));
    }
}
