//! Integration tests for agent session flow
//!
//! Tests the flow: MockAgent -> AgentEvents -> Session state verification
//! These tests verify that the mock agent infrastructure works correctly
//! and can be used for higher-level integration testing.

use std::path::PathBuf;
use std::time::Duration;

use conduit::agent::events::{AgentEvent, AssistantMessageEvent, SessionInitEvent};
use conduit::agent::mock::{MockAgentRunner, MockConfig, MockEventBuilder, MockStartError};
use conduit::agent::runner::{AgentRunner, AgentStartConfig, AgentType};
use conduit::agent::session::SessionId;

/// Test that the mock agent correctly emits a session init event
#[tokio::test]
async fn test_mock_agent_emits_session_init() {
    let events = vec![AgentEvent::SessionInit(SessionInitEvent {
        session_id: SessionId::from_string("integration-test-001"),
        model: Some("claude-sonnet-4-5-20250929".to_string()),
    })];

    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().with_events(events));

    let config = AgentStartConfig::new("Hello", PathBuf::from("/test/workspace"));
    let mut handle = runner
        .start(config)
        .await
        .expect("Failed to start mock agent");

    let event = handle
        .events
        .recv()
        .await
        .expect("Should receive session init event");

    match event {
        AgentEvent::SessionInit(init) => {
            assert_eq!(init.session_id.as_str(), "integration-test-001");
            assert_eq!(init.model, Some("claude-sonnet-4-5-20250929".to_string()));
        }
        other => panic!("Expected SessionInit event, got {:?}", other),
    }
}

/// Test that the mock agent emits a complete message sequence
#[tokio::test]
async fn test_mock_agent_emits_full_conversation() {
    let events = MockEventBuilder::new("test-conversation-001")
        .session_init(Some("claude-sonnet"))
        .turn_started()
        .assistant_message("Hello! How can I help you?", true)
        .turn_completed(150, 35)
        .build();

    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().with_events(events));

    let config = AgentStartConfig::new("Hi", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let mut received = Vec::new();
    while let Some(event) = handle.events.recv().await {
        received.push(event);
    }

    assert_eq!(received.len(), 4);
    assert!(matches!(received[0], AgentEvent::SessionInit(_)));
    assert!(matches!(received[1], AgentEvent::TurnStarted));
    assert!(matches!(received[2], AgentEvent::AssistantMessage(_)));
    assert!(matches!(received[3], AgentEvent::TurnCompleted(_)));

    // Verify the assistant message content
    if let AgentEvent::AssistantMessage(msg) = &received[2] {
        assert_eq!(msg.text, "Hello! How can I help you?");
        assert!(msg.is_final);
    }

    // Verify token usage
    if let AgentEvent::TurnCompleted(turn) = &received[3] {
        assert_eq!(turn.usage.input_tokens, 150);
        assert_eq!(turn.usage.output_tokens, 35);
    }
}

/// Test that the mock agent correctly emits tool use events
#[tokio::test]
async fn test_mock_agent_with_tool_use() {
    let events = MockEventBuilder::new("test-tool-001")
        .session_init(Some("claude-sonnet"))
        .assistant_message("I'll list the files for you.", false)
        .tool_started(
            "Bash",
            "tool-bash-001",
            serde_json::json!({"command": "ls -la"}),
        )
        .tool_completed("tool-bash-001", true, Some("file1.txt\nfile2.txt"), None)
        .assistant_message("Here are the files:\n- file1.txt\n- file2.txt", true)
        .turn_completed(200, 80)
        .build();

    let runner = MockAgentRunner::new(AgentType::Claude).with_events(events);

    let config = AgentStartConfig::new("List files", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let mut received = Vec::new();
    while let Some(event) = handle.events.recv().await {
        received.push(event);
    }

    assert_eq!(received.len(), 6);

    // Verify tool started event
    if let AgentEvent::ToolStarted(tool) = &received[2] {
        assert_eq!(tool.tool_name, "Bash");
        assert_eq!(tool.tool_id, "tool-bash-001");
    }

    // Verify tool completed event
    if let AgentEvent::ToolCompleted(tool) = &received[3] {
        assert_eq!(tool.tool_id, "tool-bash-001");
        assert!(tool.success);
        assert_eq!(tool.result, Some("file1.txt\nfile2.txt".to_string()));
    }
}

/// Test that the mock agent captures the start configuration
#[tokio::test]
async fn test_mock_captures_start_config() {
    let runner = MockAgentRunner::new(AgentType::Claude);

    let config = AgentStartConfig::new(
        "Test prompt with details",
        PathBuf::from("/workspace/project"),
    )
    .with_model("opus")
    .with_tools(vec![
        "Bash".to_string(),
        "Read".to_string(),
        "Edit".to_string(),
    ]);

    let _ = runner.start(config).await.unwrap();

    let captured = runner.captured_configs();
    assert_eq!(captured.len(), 1);

    let config = &captured[0];
    assert_eq!(config.prompt, "Test prompt with details");
    assert_eq!(config.working_dir, PathBuf::from("/workspace/project"));
    assert_eq!(config.model, Some("opus".to_string()));
    assert_eq!(config.allowed_tools.len(), 3);
    assert!(config.allowed_tools.contains(&"Bash".to_string()));
}

/// Test that the mock agent captures inputs sent during the session
#[tokio::test]
async fn test_mock_captures_user_inputs() {
    let runner = MockAgentRunner::new(AgentType::Claude);
    let config = AgentStartConfig::new("Initial prompt", PathBuf::from("/tmp"));
    let handle = runner.start(config).await.unwrap();

    // Simulate user sending follow-up messages
    runner
        .send_input(&handle, "Please also check the tests")
        .await
        .unwrap();
    runner
        .send_input(&handle, "And run cargo build")
        .await
        .unwrap();

    let inputs = runner.captured_inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0], "Please also check the tests");
    assert_eq!(inputs[1], "And run cargo build");
}

/// Test that the mock agent can simulate failures
#[tokio::test]
async fn test_mock_agent_failure_modes() {
    // Test default failure
    let runner =
        MockAgentRunner::new(AgentType::Claude).with_config(MockConfig::default().failing());

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let result = runner.start(config).await;
    assert!(result.is_err());

    // Test specific failure type
    let runner =
        MockAgentRunner::new(AgentType::Claude).with_config(MockConfig::default().failing_with(
            MockStartError::Config("Invalid model specified".to_string()),
        ));

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let result = runner.start(config).await;

    // Use pattern matching since AgentHandle doesn't implement Debug for unwrap_err
    match result {
        Ok(_) => panic!("Expected error"),
        Err(err) => assert!(
            err.to_string().contains("Invalid model"),
            "Error should contain 'Invalid model': {}",
            err
        ),
    }
}

/// Test that stop and kill are tracked
#[tokio::test]
async fn test_mock_tracks_lifecycle() {
    let runner = MockAgentRunner::new(AgentType::Claude);
    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let handle = runner.start(config).await.unwrap();

    // Initially not stopped or killed
    assert!(!runner.was_stopped());
    assert!(!runner.was_killed());

    // Stop the agent
    runner.stop(&handle).await.unwrap();
    assert!(runner.was_stopped());
    assert!(!runner.was_killed());

    // Kill the agent
    runner.kill(&handle).await.unwrap();
    assert!(runner.was_stopped());
    assert!(runner.was_killed());
}

/// Test that the mock can be reset for reuse
#[tokio::test]
async fn test_mock_reset() {
    let runner = MockAgentRunner::new(AgentType::Claude);

    // Start a session and capture some data
    let config = AgentStartConfig::new("first test", PathBuf::from("/tmp"));
    let handle = runner.start(config).await.unwrap();
    runner.send_input(&handle, "some input").await.unwrap();
    runner.stop(&handle).await.unwrap();

    // Verify data was captured
    assert_eq!(runner.captured_configs().len(), 1);
    assert_eq!(runner.captured_inputs().len(), 1);
    assert!(runner.was_stopped());

    // Reset the mock
    runner.reset();

    // Verify everything is cleared
    assert!(runner.captured_configs().is_empty());
    assert!(runner.captured_inputs().is_empty());
    assert!(!runner.was_stopped());
    assert!(!runner.was_killed());
}

/// Test that events are emitted with configurable delay
#[tokio::test]
async fn test_mock_event_timing() {
    let events = vec![
        AgentEvent::TurnStarted,
        AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "First".to_string(),
            is_final: false,
        }),
        AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "Second".to_string(),
            is_final: true,
        }),
    ];

    let runner = MockAgentRunner::new(AgentType::Claude).with_config(
        MockConfig::default()
            .with_events(events)
            .with_delay(Duration::from_millis(50)),
    );

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let start = std::time::Instant::now();

    // Consume all events
    let mut count = 0;
    while handle.events.recv().await.is_some() {
        count += 1;
    }

    let elapsed = start.elapsed();

    assert_eq!(count, 3);
    // With 50ms delay between 3 events, should take at least 100ms (2 delays)
    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected at least 100ms, got {:?}",
        elapsed
    );
}

/// Test that multiple sessions can be started from the same runner
#[tokio::test]
async fn test_mock_multiple_sessions() {
    let events = vec![AgentEvent::SessionInit(SessionInitEvent {
        session_id: SessionId::from_string("reusable-session"),
        model: None,
    })];

    let runner = MockAgentRunner::new(AgentType::Claude)
        .with_config(MockConfig::default().with_events(events));

    // Start multiple sessions
    for i in 0..3 {
        let config = AgentStartConfig::new(format!("Prompt {}", i), PathBuf::from("/tmp"));
        let mut handle = runner.start(config).await.unwrap();

        // Each session should receive events
        let event = handle.events.recv().await;
        assert!(event.is_some());
    }

    // All sessions should be captured
    let configs = runner.captured_configs();
    assert_eq!(configs.len(), 3);
    assert_eq!(configs[0].prompt, "Prompt 0");
    assert_eq!(configs[1].prompt, "Prompt 1");
    assert_eq!(configs[2].prompt, "Prompt 2");
}

/// Test Codex agent type
#[tokio::test]
async fn test_mock_codex_agent() {
    let events = MockEventBuilder::new("codex-test-001")
        .session_init(Some("gpt-5-mini"))
        .assistant_message("I'm Codex, ready to help!", true)
        .turn_completed(100, 25)
        .build();

    let runner = MockAgentRunner::new(AgentType::Codex).with_events(events);

    // Verify agent type
    assert_eq!(runner.agent_type(), AgentType::Codex);
    assert!(runner.is_available());
    assert_eq!(runner.binary_path(), Some(PathBuf::from("/mock/agent")));

    let config = AgentStartConfig::new("Hello Codex", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let mut received = Vec::new();
    while let Some(event) = handle.events.recv().await {
        received.push(event);
    }

    assert_eq!(received.len(), 3);
}

/// Test error event handling
#[tokio::test]
async fn test_mock_error_events() {
    let events = MockEventBuilder::new("error-test-001")
        .session_init(None)
        .error("Rate limit exceeded", false)
        .error("Fatal: Out of memory", true)
        .build();

    let runner = MockAgentRunner::new(AgentType::Claude).with_events(events);

    let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
    let mut handle = runner.start(config).await.unwrap();

    let mut received = Vec::new();
    while let Some(event) = handle.events.recv().await {
        received.push(event);
    }

    assert_eq!(received.len(), 3);

    // Check non-fatal error
    if let AgentEvent::Error(err) = &received[1] {
        assert_eq!(err.message, "Rate limit exceeded");
        assert!(!err.is_fatal);
    }

    // Check fatal error
    if let AgentEvent::Error(err) = &received[2] {
        assert_eq!(err.message, "Fatal: Out of memory");
        assert!(err.is_fatal);
    }
}
