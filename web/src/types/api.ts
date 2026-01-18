// API types matching Rust backend

export interface Repository {
  id: string;
  name: string;
  base_path: string | null;
  repository_url: string | null;
  created_at: string;
  updated_at: string;
}

export interface Workspace {
  id: string;
  repository_id: string;
  name: string;
  branch: string;
  path: string;
  created_at: string;
  last_accessed: string;
  is_default: boolean;
  archived_at: string | null;
}

export interface Session {
  id: string;
  tab_index: number;
  workspace_id: string | null;
  agent_type: 'claude' | 'codex' | 'gemini';
  agent_mode: string | null;
  agent_session_id: string | null;
  model: string | null;
  model_display_name: string | null;
  pr_number: number | null;
  created_at: string;
  title: string | null;
}

export interface Agent {
  id: string;
  name: string;
  available: boolean;
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface ListRepositoriesResponse {
  repositories: Repository[];
}

export interface ListWorkspacesResponse {
  workspaces: Workspace[];
}

export interface ListSessionsResponse {
  sessions: Session[];
}

export interface AgentsResponse {
  agents: Agent[];
}

export interface CreateRepositoryRequest {
  name: string;
  base_path?: string;
  repository_url?: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  branch: string;
  path: string;
  is_default?: boolean;
}

export interface CreateSessionRequest {
  workspace_id?: string;
  agent_type: 'claude' | 'codex' | 'gemini';
  model?: string;
}

export interface TurnSummary {
  duration_secs: number;
  input_tokens: number;
  output_tokens: number;
}

export interface SessionEvent {
  role: 'user' | 'assistant' | 'tool' | 'system' | 'error' | 'summary';
  content: string;
  tool_name?: string;
  tool_args?: string;
  exit_code?: number;
  summary?: TurnSummary;
}

export interface HistoryDebugEntry {
  line: number;
  entry_type: string;
  status: string;
  reason: string;
  raw: unknown;
}

export interface ListSessionEventsResponse {
  events: SessionEvent[];
  total: number;
  offset: number;
  limit: number;
  debug_file?: string | null;
  debug_entries?: HistoryDebugEntry[];
}

export interface SessionEventsQuery {
  limit?: number;
  offset?: number;
  tail?: boolean;
}

export interface BootstrapResponse {
  ui_state: UiState;
  sessions: Session[];
  workspaces: Workspace[];
  active_session: Session | null;
  active_workspace: Workspace | null;
}

export interface GitDiffStats {
  additions: number;
  deletions: number;
  files_changed: number;
}

export interface PrStatus {
  number: number;
  state: 'open' | 'merged' | 'closed' | 'draft' | 'unknown';
  checks_passing: boolean;
  url?: string;
}

export interface WorkspaceStatus {
  git_stats?: GitDiffStats;
  pr_status?: PrStatus;
  updated_at?: string;
}

export interface UiState {
  active_session_id: string | null;
  tab_order: string[];
  sidebar_open: boolean;
  last_workspace_id: string | null;
}

// Model types
export interface ModelInfo {
  id: string;
  display_name: string;
  description: string;
  is_default: boolean;
  agent_type: 'claude' | 'codex' | 'gemini';
  context_window: number;
}

export interface ModelGroup {
  agent_type: string;
  section_title: string;
  icon: string;
  models: ModelInfo[];
}

export interface ListModelsResponse {
  groups: ModelGroup[];
}

export interface UpdateSessionRequest {
  model?: string;
  agent_type?: 'claude' | 'codex' | 'gemini';
  agent_mode?: 'build' | 'plan';
}

export interface SetDefaultModelRequest {
  agent_type: 'claude' | 'codex' | 'gemini';
  model_id: string;
}
