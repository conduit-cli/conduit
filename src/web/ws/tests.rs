//! Tests for WebSocket message types and serialization.

use super::messages::{ClientMessage, ServerMessage};
use crate::agent::events::{AgentEvent, AssistantMessageEvent, SessionInitEvent};
use crate::agent::session::SessionId;
use uuid::Uuid;

#[test]
fn test_client_message_ping_serialization() {
    let msg = ClientMessage::Ping;
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, r#"{"type":"ping"}"#);

    let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, ClientMessage::Ping));
}

#[test]
fn test_client_message_subscribe_serialization() {
    let session_id = Uuid::nil();
    let msg = ClientMessage::Subscribe { session_id };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"subscribe""#));
    assert!(json.contains(&session_id.to_string()));

    let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
    if let ClientMessage::Subscribe {
        session_id: parsed_id,
    } = parsed
    {
        assert_eq!(parsed_id, session_id);
    } else {
        panic!("Expected Subscribe message");
    }
}

#[test]
fn test_client_message_start_session_serialization() {
    let session_id = Uuid::nil();
    let msg = ClientMessage::StartSession {
        session_id,
        prompt: "Hello, world!".to_string(),
        working_dir: "/home/user/project".to_string(),
        model: Some("claude-sonnet-4-20250514".to_string()),
        hidden: false,
        images: Vec::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"start_session""#));
    assert!(json.contains("Hello, world!"));
    assert!(json.contains("claude-sonnet-4-20250514"));

    let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
    if let ClientMessage::StartSession {
        session_id: parsed_id,
        prompt,
        working_dir,
        model,
        hidden,
        images,
    } = parsed
    {
        assert_eq!(parsed_id, session_id);
        assert_eq!(prompt, "Hello, world!");
        assert_eq!(working_dir, "/home/user/project");
        assert_eq!(model, Some("claude-sonnet-4-20250514".to_string()));
        assert!(!hidden);
        assert!(images.is_empty());
    } else {
        panic!("Expected StartSession message");
    }
}

#[test]
fn test_server_message_pong_serialization() {
    let msg = ServerMessage::Pong;
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, r#"{"type":"pong"}"#);

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, ServerMessage::Pong));
}

#[test]
fn test_server_message_error_serialization() {
    let msg = ServerMessage::error("Something went wrong");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"error""#));
    assert!(json.contains("Something went wrong"));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::Error {
        message,
        session_id,
    } = parsed
    {
        assert_eq!(message, "Something went wrong");
        assert!(session_id.is_none());
    } else {
        panic!("Expected Error message");
    }
}

#[test]
fn test_server_message_session_error_serialization() {
    let session_id = Uuid::nil();
    let msg = ServerMessage::session_error(session_id, "Session failed");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"error""#));
    assert!(json.contains("Session failed"));
    assert!(json.contains(&session_id.to_string()));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::Error {
        message,
        session_id: sid,
    } = parsed
    {
        assert_eq!(message, "Session failed");
        assert_eq!(sid, Some(session_id));
    } else {
        panic!("Expected Error message");
    }
}

#[test]
fn test_server_message_agent_event_serialization() {
    let session_id = Uuid::nil();
    let event = AgentEvent::AssistantMessage(AssistantMessageEvent {
        text: "Hello from Claude!".to_string(),
        is_final: true,
    });
    let msg = ServerMessage::agent_event(session_id, event);
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"agent_event""#));
    assert!(json.contains("Hello from Claude!"));
    assert!(json.contains(&session_id.to_string()));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::AgentEvent {
        session_id: sid,
        event: parsed_event,
    } = parsed
    {
        assert_eq!(sid, session_id);
        if let AgentEvent::AssistantMessage(msg) = parsed_event {
            assert_eq!(msg.text, "Hello from Claude!");
            assert!(msg.is_final);
        } else {
            panic!("Expected AssistantMessage event");
        }
    } else {
        panic!("Expected AgentEvent message");
    }
}

#[test]
fn test_server_message_session_init_event_serialization() {
    let session_id = Uuid::nil();
    let event = AgentEvent::SessionInit(SessionInitEvent {
        session_id: SessionId::from_string("claude-session-123"),
        model: Some("claude-sonnet-4-20250514".to_string()),
    });
    let msg = ServerMessage::agent_event(session_id, event);
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("claude-session-123"));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::AgentEvent {
        event: parsed_event,
        ..
    } = parsed
    {
        if let AgentEvent::SessionInit(init) = parsed_event {
            assert_eq!(init.session_id.as_str(), "claude-session-123");
            assert_eq!(init.model, Some("claude-sonnet-4-20250514".to_string()));
        } else {
            panic!("Expected SessionInit event");
        }
    } else {
        panic!("Expected AgentEvent message");
    }
}

#[test]
fn test_server_message_session_started_serialization() {
    use crate::agent::runner::AgentType;

    let session_id = Uuid::nil();
    let msg = ServerMessage::session_started(
        session_id,
        AgentType::Claude,
        Some("claude-123".to_string()),
    );
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"session_started""#));
    assert!(json.contains("claude"));
    assert!(json.contains("claude-123"));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::SessionStarted {
        session_id: sid,
        agent_type,
        agent_session_id,
    } = parsed
    {
        assert_eq!(sid, session_id);
        assert_eq!(agent_type, "claude");
        assert_eq!(agent_session_id, Some("claude-123".to_string()));
    } else {
        panic!("Expected SessionStarted message");
    }
}

#[test]
fn test_server_message_session_metadata_serialization() {
    let session_id = Uuid::nil();
    let workspace_id = Uuid::new_v4();
    let msg = ServerMessage::SessionMetadata {
        session_id,
        title: Some("Fix session naming".to_string()),
        workspace_id: Some(workspace_id),
        workspace_branch: Some("user/fix-session-naming".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"session_metadata""#));
    assert!(json.contains("Fix session naming"));
    assert!(json.contains("user/fix-session-naming"));
    assert!(json.contains(&workspace_id.to_string()));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::SessionMetadata {
        session_id: parsed_session_id,
        title,
        workspace_id: parsed_workspace_id,
        workspace_branch,
    } = parsed
    {
        assert_eq!(parsed_session_id, session_id);
        assert_eq!(title, Some("Fix session naming".to_string()));
        assert_eq!(parsed_workspace_id, Some(workspace_id));
        assert_eq!(
            workspace_branch,
            Some("user/fix-session-naming".to_string())
        );
    } else {
        panic!("Expected SessionMetadata message");
    }
}

#[test]
fn test_server_message_session_ended_serialization() {
    let session_id = Uuid::nil();
    let msg = ServerMessage::SessionEnded {
        session_id,
        reason: "completed".to_string(),
        error: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""type":"session_ended""#));
    assert!(json.contains("completed"));

    let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
    if let ServerMessage::SessionEnded {
        session_id: sid,
        reason,
        error,
    } = parsed
    {
        assert_eq!(sid, session_id);
        assert_eq!(reason, "completed");
        assert!(error.is_none());
    } else {
        panic!("Expected SessionEnded message");
    }
}
