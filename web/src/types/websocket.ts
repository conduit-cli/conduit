// WebSocket message types matching Rust backend

// Client -> Server messages
export type ClientMessage =
  | { type: 'ping' }
  | { type: 'subscribe'; session_id: string }
  | { type: 'unsubscribe'; session_id: string }
  | { type: 'start_session'; session_id: string; prompt: string; working_dir: string; model?: string }
  | { type: 'send_input'; session_id: string; input: string }
  | { type: 'respond_to_control'; session_id: string; request_id: string; allow: boolean }
  | { type: 'stop_session'; session_id: string };

// Server -> Client messages
export type ServerMessage =
  | { type: 'pong' }
  | { type: 'subscribed'; session_id: string }
  | { type: 'unsubscribed'; session_id: string }
  | { type: 'session_started'; session_id: string; agent_type: string; agent_session_id: string | null }
  | { type: 'agent_event'; session_id: string; event: AgentEvent }
  | { type: 'session_ended'; session_id: string; reason: string; error: string | null }
  | { type: 'error'; message: string; session_id: string | null };

// Agent events (unified across Claude, Codex, Gemini)
export type AgentEvent =
  | { type: 'SessionInit'; session_id: string; model: string | null }
  | { type: 'TurnStarted' }
  | { type: 'TurnCompleted'; usage: TokenUsage }
  | { type: 'TurnFailed'; error: string }
  | { type: 'AssistantMessage'; text: string; is_final: boolean }
  | { type: 'AssistantReasoning'; text: string }
  | { type: 'ToolStarted'; tool_name: string; tool_id: string; arguments: unknown }
  | { type: 'ToolCompleted'; tool_id: string; success: boolean; result: string | null; error: string | null }
  | { type: 'ControlRequest'; request_id: string; tool_name: string; tool_use_id: string | null; input: unknown }
  | { type: 'FileChanged'; path: string; operation: 'create' | 'update' | 'delete' }
  | { type: 'CommandOutput'; command: string; output: string; exit_code: number | null; is_streaming: boolean }
  | { type: 'TokenUsage'; usage: TokenUsage; context_window: number | null; usage_percent: number | null }
  | { type: 'ContextCompaction'; reason: string; tokens_before: number; tokens_after: number }
  | { type: 'Error'; message: string; is_fatal: boolean }
  | { type: 'Raw'; data: unknown };

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  total_tokens: number;
}
