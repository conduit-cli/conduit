//! Unified message display formatting
//!
//! Provides a common intermediate format for all message types,
//! used by both live events and history restoration.

use crate::ui::components::ChatMessage;

/// Normalized message for display (used by both live events and history)
#[derive(Debug, Clone)]
pub enum MessageDisplay {
    User {
        content: String,
    },
    Assistant {
        content: String,
        is_streaming: bool,
    },
    Tool {
        name: String,
        args: String,
        output: String,
        exit_code: Option<i32>,
        file_size: Option<u64>,
    },
    System {
        content: String,
    },
    Error {
        content: String,
    },
}

impl MessageDisplay {
    /// Convert to ChatMessage with consistent formatting
    pub fn to_chat_message(&self) -> ChatMessage {
        match self {
            MessageDisplay::User { content } => ChatMessage::user(content),
            MessageDisplay::Assistant {
                content,
                is_streaming,
            } => {
                if *is_streaming {
                    ChatMessage::streaming(content)
                } else {
                    ChatMessage::assistant(content)
                }
            }
            MessageDisplay::Tool {
                name,
                args,
                output,
                exit_code,
                file_size,
            } => {
                let mut msg = ChatMessage::tool_with_exit(name, args, output, *exit_code);
                // Use provided file_size (from session history) if available
                msg.file_size = *file_size;
                msg
            }
            MessageDisplay::System { content } => ChatMessage::system(content),
            MessageDisplay::Error { content } => ChatMessage::error(content),
        }
    }

    /// Map raw tool names to display names
    pub fn tool_display_name(raw_name: &str) -> &'static str {
        match raw_name {
            "exec_command" | "shell" | "shell_command" | "local_shell_call"
            | "command_execution" | "Bash" => "Bash",
            "read_file" | "Read" => "Read",
            "write_file" | "Write" => "Write",
            "list_directory" | "LS" => "LS",
            "Glob" => "Glob",
            "Grep" => "Grep",
            "Edit" => "Edit",
            "TodoWrite" => "TodoWrite",
            "Task" => "Task",
            _ => "Tool", // Default for unknown names
        }
    }

    /// Map raw tool names to display names, returning owned String for unknown names
    pub fn tool_display_name_owned(raw_name: &str) -> String {
        match raw_name {
            "exec_command" | "shell" | "shell_command" | "local_shell_call"
            | "command_execution" | "Bash" => "Bash".to_string(),
            "read_file" | "Read" => "Read".to_string(),
            "write_file" | "Write" => "Write".to_string(),
            "list_directory" | "LS" => "LS".to_string(),
            "Glob" => "Glob".to_string(),
            "Grep" => "Grep".to_string(),
            "Edit" => "Edit".to_string(),
            "TodoWrite" => "TodoWrite".to_string(),
            "Task" => "Task".to_string(),
            _ => raw_name.to_string(), // Pass through unknown names
        }
    }

    /// Parse Codex metadata-wrapped output to extract actual output and exit code
    ///
    /// Codex output format:
    /// ```text
    /// Chunk ID: xxx
    /// Wall time: xxx seconds
    /// Process exited with code X
    /// Original token count: xxx
    /// Output:
    /// [actual output here]
    /// ```
    pub fn parse_codex_tool_output(raw_output: &str) -> (String, Option<i32>) {
        let mut exit_code = None;

        // Find exit code
        if let Some(pos) = raw_output.find("Process exited with code ") {
            let after = &raw_output[pos + 25..];
            if let Some(end) = after.find('\n') {
                if let Ok(code) = after[..end].trim().parse::<i32>() {
                    exit_code = Some(code);
                }
            }
        } else if let Some(pos) = raw_output.find("Exit code:") {
            let after = &raw_output[pos + 10..];
            if let Some(end) = after.find('\n') {
                if let Ok(code) = after[..end].trim().parse::<i32>() {
                    exit_code = Some(code);
                }
            }
        }

        // Find actual output after "Output:\n"
        let output = if let Some(pos) = raw_output.find("Output:\n") {
            raw_output[pos + 8..].to_string()
        } else {
            raw_output.to_string()
        };

        (output, exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_display_name() {
        assert_eq!(MessageDisplay::tool_display_name("exec_command"), "Bash");
        assert_eq!(MessageDisplay::tool_display_name("shell"), "Bash");
        assert_eq!(
            MessageDisplay::tool_display_name("local_shell_call"),
            "Bash"
        );
        assert_eq!(MessageDisplay::tool_display_name("read_file"), "Read");
        assert_eq!(MessageDisplay::tool_display_name("write_file"), "Write");
        assert_eq!(MessageDisplay::tool_display_name("unknown_tool"), "Tool");
    }

    #[test]
    fn test_tool_display_name_owned() {
        assert_eq!(
            MessageDisplay::tool_display_name_owned("exec_command"),
            "Bash"
        );
        assert_eq!(
            MessageDisplay::tool_display_name_owned("custom_tool"),
            "custom_tool"
        );
    }

    #[test]
    fn test_parse_codex_tool_output() {
        let raw = r#"Chunk ID: abc123
Wall time: 0.5 seconds
Process exited with code 0
Original token count: 100
Output:
hello world
this is the actual output"#;

        let (output, exit_code) = MessageDisplay::parse_codex_tool_output(raw);
        assert_eq!(exit_code, Some(0));
        assert_eq!(output, "hello world\nthis is the actual output");
    }

    #[test]
    fn test_parse_codex_tool_output_no_metadata() {
        let raw = "plain output without metadata";
        let (output, exit_code) = MessageDisplay::parse_codex_tool_output(raw);
        assert_eq!(exit_code, None);
        assert_eq!(output, "plain output without metadata");
    }

    #[test]
    fn test_parse_codex_tool_output_error_code() {
        let raw = r#"Chunk ID: xyz
Process exited with code 1
Output:
error: command failed"#;

        let (output, exit_code) = MessageDisplay::parse_codex_tool_output(raw);
        assert_eq!(exit_code, Some(1));
        assert_eq!(output, "error: command failed");
    }

    #[test]
    fn test_to_chat_message_user() {
        let display = MessageDisplay::User {
            content: "Hello".to_string(),
        };
        let msg = display.to_chat_message();
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_to_chat_message_tool_with_exit_code() {
        let display = MessageDisplay::Tool {
            name: "Bash".to_string(),
            args: "ls -la".to_string(),
            output: "file1.txt\nfile2.txt".to_string(),
            exit_code: Some(0),
            file_size: None,
        };
        let msg = display.to_chat_message();
        assert_eq!(msg.exit_code, Some(0));
        assert!(msg.content.contains("file1.txt"));
    }

    #[test]
    fn test_to_chat_message_tool_without_exit_code() {
        let display = MessageDisplay::Tool {
            name: "Read".to_string(),
            args: "file.txt".to_string(),
            output: "file contents".to_string(),
            exit_code: None,
            file_size: None,
        };
        let msg = display.to_chat_message();
        assert_eq!(msg.exit_code, None);
        assert_eq!(msg.content, "file contents");
    }

    #[test]
    fn test_to_chat_message_tool_with_file_size() {
        let display = MessageDisplay::Tool {
            name: "Read".to_string(),
            args: r#"{"file_path": "/tmp/image.png"}"#.to_string(),
            output: "image data".to_string(),
            exit_code: None,
            file_size: Some(12345),
        };
        let msg = display.to_chat_message();
        assert_eq!(
            msg.file_size,
            Some(12345),
            "file_size should be preserved through conversion"
        );
    }
}
