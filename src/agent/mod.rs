pub mod claude;
pub mod codex;
pub mod display;
pub mod error;
pub mod events;
pub mod gemini;
pub mod history;
pub mod mock;
pub mod models;
pub mod runner;
pub mod session;
pub mod stream;

pub use claude::ClaudeCodeRunner;
pub use codex::CodexCliRunner;
pub use display::MessageDisplay;
pub use error::AgentError;
pub use events::*;
pub use gemini::GeminiCliRunner;
pub use history::{
    load_claude_history_with_debug, load_codex_history_with_debug, HistoryDebugEntry, HistoryError,
};
pub use mock::{MockAgentRunner, MockConfig, MockEventBuilder, MockStartError};
pub use models::{ModelInfo, ModelRegistry};
pub use runner::{AgentHandle, AgentMode, AgentRunner, AgentStartConfig, AgentType};
pub use session::{SessionId, SessionMetadata, SessionStatus};
