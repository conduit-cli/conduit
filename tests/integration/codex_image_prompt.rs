//! Integration test for Codex image prompts via app-server.
//!
//! This test is ignored by default because it requires a local Codex install
//! (or npx) plus valid auth. Run with:
//! CODEX_IMAGE_TEST=1 cargo test --test integration_tests codex_image_prompt -- --ignored

use std::io;
use std::process::Stdio;
use std::time::Duration;

use codex_app_server_protocol::{
    AddConversationListenerParams, ClientInfo, ClientNotification, ClientRequest, InitializeParams,
    InputItem, JSONRPCMessage, NewConversationParams, NewConversationResponse, RequestId,
    SendUserMessageParams,
};
use codex_protocol::config_types::SandboxMode;
use codex_protocol::protocol::{AskForApproval, EventMsg};
use image::{DynamicImage, Rgba, RgbaImage};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use tempfile::Builder;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::timeout;
use which::which;

#[derive(Debug, Deserialize)]
struct CodexNotificationParams {
    #[serde(rename = "msg")]
    msg: EventMsg,
}

fn build_codex_command() -> Option<Command> {
    if which("codex").is_ok() {
        let mut cmd = Command::new("codex");
        cmd.arg("app-server");
        return Some(cmd);
    }

    if which("npx").is_ok() {
        let mut cmd = Command::new("npx");
        cmd.args(["-y", "@openai/codex", "app-server"]);
        return Some(cmd);
    }

    None
}

async fn send_message<T: Serialize>(stdin: &mut ChildStdin, message: &T) -> io::Result<()> {
    let raw = serde_json::to_string(message).map_err(|e| io::Error::other(e.to_string()))?;
    stdin.write_all(raw.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

async fn next_jsonrpc_message(
    lines: &mut tokio::io::Lines<BufReader<ChildStdout>>,
) -> io::Result<JSONRPCMessage> {
    loop {
        let Some(line) = lines.next_line().await? else {
            return Err(io::Error::other("codex app-server closed stdout"));
        };

        if let Ok(msg) = serde_json::from_str::<JSONRPCMessage>(&line) {
            return Ok(msg);
        }
    }
}

async fn wait_for_response<R: DeserializeOwned>(
    lines: &mut tokio::io::Lines<BufReader<ChildStdout>>,
    request_id: RequestId,
) -> io::Result<R> {
    loop {
        let msg = next_jsonrpc_message(lines).await?;
        match msg {
            JSONRPCMessage::Response(resp) if resp.id == request_id => {
                return serde_json::from_value(resp.result)
                    .map_err(|e| io::Error::other(e.to_string()));
            }
            JSONRPCMessage::Error(err) if err.id == request_id => {
                return Err(io::Error::other(err.error.message));
            }
            _ => {}
        }
    }
}

#[tokio::test]
#[ignore = "requires local codex app-server and CODEX_IMAGE_TEST=1"]
async fn test_codex_user_message_includes_images() {
    if std::env::var("CODEX_IMAGE_TEST").is_err() {
        eprintln!("Skipping: set CODEX_IMAGE_TEST=1 to run this test.");
        return;
    }

    let Some(mut cmd) = build_codex_command() else {
        eprintln!("Skipping: codex or npx not found in PATH.");
        return;
    };

    let cwd = std::env::current_dir().expect("current dir should exist");
    cmd.current_dir(&cwd);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.env("NODE_NO_WARNINGS", "1");
    cmd.env("NO_COLOR", "1");

    let mut child = cmd.spawn().expect("failed to spawn codex app-server");
    let mut stdin = child.stdin.take().expect("missing codex stdin");
    let stdout = child.stdout.take().expect("missing codex stdout");
    let stderr = child.stderr.take();

    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if std::env::var("CODEX_IMAGE_TEST_VERBOSE").is_ok() {
                    eprintln!("[codex stderr] {line}");
                }
            }
        });
    }

    let mut lines = BufReader::new(stdout).lines();
    let mut next_id = 1_i64;

    let init_id = RequestId::Integer(next_id);
    next_id += 1;
    let init_request = ClientRequest::Initialize {
        request_id: init_id.clone(),
        params: InitializeParams {
            client_info: ClientInfo {
                name: "conduit-tests".to_string(),
                title: Some("Conduit Tests".to_string()),
                version: "0.0.0".to_string(),
            },
        },
    };
    send_message(&mut stdin, &init_request)
        .await
        .expect("failed to send initialize request");
    let _: serde_json::Value = wait_for_response(&mut lines, init_id)
        .await
        .expect("failed to receive initialize response");
    send_message(&mut stdin, &ClientNotification::Initialized)
        .await
        .expect("failed to send initialized notification");

    let conv_id = RequestId::Integer(next_id);
    next_id += 1;
    let conv_request = ClientRequest::NewConversation {
        request_id: conv_id.clone(),
        params: NewConversationParams {
            model: None,
            model_provider: None,
            profile: None,
            cwd: Some(cwd.to_string_lossy().to_string()),
            approval_policy: Some(AskForApproval::Never),
            sandbox: Some(SandboxMode::DangerFullAccess),
            config: None,
            base_instructions: None,
            include_apply_patch_tool: None,
            compact_prompt: None,
            developer_instructions: None,
        },
    };
    send_message(&mut stdin, &conv_request)
        .await
        .expect("failed to send new conversation request");
    let conv_response: NewConversationResponse = wait_for_response(&mut lines, conv_id)
        .await
        .expect("failed to receive new conversation response");

    let listen_id = RequestId::Integer(next_id);
    next_id += 1;
    let listen_request = ClientRequest::AddConversationListener {
        request_id: listen_id.clone(),
        params: AddConversationListenerParams {
            conversation_id: conv_response.conversation_id,
            experimental_raw_events: false,
        },
    };
    send_message(&mut stdin, &listen_request)
        .await
        .expect("failed to send add listener request");
    let _: serde_json::Value = wait_for_response(&mut lines, listen_id)
        .await
        .expect("failed to receive add listener response");

    let tmp = Builder::new()
        .prefix("conduit-codex-image-")
        .suffix(".png")
        .tempfile()
        .expect("failed to create temp image");
    let image_path = tmp.path().to_path_buf();
    let image = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let dyn_img = DynamicImage::ImageRgba8(image);
    dyn_img
        .save(&image_path)
        .expect("failed to write temp image");

    let prompt = "Describe this image.";
    let send_id = RequestId::Integer(next_id);
    let send_request = ClientRequest::SendUserMessage {
        request_id: send_id.clone(),
        params: SendUserMessageParams {
            conversation_id: conv_response.conversation_id,
            items: vec![
                InputItem::Text {
                    text: prompt.to_string(),
                },
                InputItem::LocalImage { path: image_path },
            ],
        },
    };
    send_message(&mut stdin, &send_request)
        .await
        .expect("failed to send user message request");

    let mut got_response = false;
    let mut user_message: Option<codex_protocol::protocol::UserMessageEvent> = None;
    let wait_result = timeout(Duration::from_secs(20), async {
        loop {
            let msg = next_jsonrpc_message(&mut lines).await?;
            match msg {
                JSONRPCMessage::Response(resp) if resp.id == send_id => {
                    got_response = true;
                }
                JSONRPCMessage::Error(err) if err.id == send_id => {
                    return Err(io::Error::other(err.error.message));
                }
                JSONRPCMessage::Notification(note) => {
                    if note.method.starts_with("codex/event/") {
                        if let Some(params) = note.params {
                            let parsed: Result<CodexNotificationParams, _> =
                                serde_json::from_value(params);
                            if let Ok(parsed) = parsed {
                                if let EventMsg::UserMessage(event) = parsed.msg {
                                    user_message = Some(event);
                                    break;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    })
    .await;

    match wait_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => panic!("codex app-server error: {err}"),
        Err(_) => panic!("timed out waiting for user_message event"),
    }

    assert!(got_response, "did not receive SendUserMessage response");
    let user_message = user_message.expect("missing user_message event");
    assert!(
        user_message.message.contains(prompt),
        "unexpected user message text: {}",
        user_message.message
    );
    let images = user_message.images.unwrap_or_default();
    assert!(
        !images.is_empty(),
        "expected user message to include images, got none"
    );

    if let Err(err) = child.kill().await {
        eprintln!("Failed to kill codex app-server: {err}");
    }
    if let Err(err) = child.wait().await {
        eprintln!("Failed to wait for codex app-server: {err}");
    }
}
