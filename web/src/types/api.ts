// API types matching Rust backend

export type WorkspaceMode = 'worktree' | 'checkout';

export interface Repository {
  id: string;
  name: string;
  base_path: string | null;
  repository_url: string | null;
  workspace_mode: WorkspaceMode | null;
  workspace_mode_effective: WorkspaceMode;
  archive_delete_branch: boolean | null;
  archive_delete_branch_effective: boolean;
  archive_remote_prompt: boolean | null;
  archive_remote_prompt_effective: boolean;
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

export interface ArchivePreflightResponse {
  branch_name: string;
  is_dirty: boolean;
  is_merged: boolean;
  commits_ahead: number;
  commits_behind: number;
  warnings: string[];
  severity: 'info' | 'warning' | 'danger';
  error: string | null;
  remote_branch_exists: boolean | null;
}

export interface RepositoryRemovePreflightResponse {
  repository_name: string;
  workspace_count: number;
  warnings: string[];
  severity: 'info' | 'warning' | 'danger';
}

export interface RepositoryRemoveResponse {
  success: boolean;
  errors: string[];
}

export interface Session {
  id: string;
  tab_index: number;
  workspace_id: string | null;
  agent_type: 'claude' | 'codex' | 'gemini' | 'opencode';
  agent_mode: string | null;
  agent_session_id: string | null;
  model: string | null;
  model_display_name: string | null;
  model_invalid: boolean;
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

export interface UpdateRepositorySettingsRequest {
  workspace_mode?: WorkspaceMode;
  archive_delete_branch?: boolean;
  archive_remote_prompt?: boolean;
}

export interface CreateWorkspaceRequest {
  name: string;
  branch: string;
  path: string;
  is_default?: boolean;
}

export interface ArchiveWorkspaceRequest {
  delete_remote?: boolean;
}

export interface CreateSessionRequest {
  workspace_id?: string;
  agent_type: 'claude' | 'codex' | 'gemini' | 'opencode';
  model?: string;
}

export interface TurnSummary {
  duration_secs: number;
  input_tokens: number;
  output_tokens: number;
}

export interface SessionEvent {
  role: 'user' | 'assistant' | 'reasoning' | 'tool' | 'system' | 'error' | 'summary';
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

export interface InputHistoryResponse {
  history: string[];
}

export interface BootstrapResponse {
  ui_state: UiState;
  sessions: Session[];
  workspaces: Workspace[];
  active_session: Session | null;
  active_workspace: Workspace | null;
}

export interface ExternalSession {
  id: string;
  agent_type: 'claude' | 'codex' | 'gemini' | 'opencode';
  display: string;
  project?: string | null;
  project_name?: string | null;
  timestamp: string;
  relative_time: string;
  message_count: number;
  file_path: string;
}

export interface ListExternalSessionsResponse {
  sessions: ExternalSession[];
}

export interface ImportExternalSessionResponse {
  session: Session;
  workspace?: Workspace;
  repository?: Repository;
}

export interface ForkSessionResponse {
  session: Session;
  workspace: Workspace;
  warnings: string[];
  token_estimate: number;
  context_window: number;
  usage_percent: number;
  seed_prompt: string;
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
  merge_readiness?: 'ready' | 'blocked' | 'has_conflicts' | 'unknown';
  checks_total?: number;
  checks_passed?: number;
  checks_failed?: number;
  checks_pending?: number;
  checks_skipped?: number;
  mergeable?: 'mergeable' | 'conflicting' | 'unknown';
  review_decision?: 'approved' | 'review_required' | 'changes_requested' | 'none';
}

export interface WorkspaceStatus {
  git_stats?: GitDiffStats;
  pr_status?: PrStatus;
  updated_at?: string;
}

export interface PrPreflightResponse {
  gh_installed: boolean;
  gh_authenticated: boolean;
  on_main_branch: boolean;
  branch_name: string;
  target_branch: string;
  uncommitted_count: number;
  has_upstream: boolean;
  existing_pr?: PrStatus;
}

export interface PrCreateResponse {
  preflight: PrPreflightResponse;
  prompt: string;
}

export interface QueuedImageAttachment {
  path: string;
  placeholder: string;
}

export interface QueuedMessage {
  id: string;
  mode: 'steer' | 'follow-up';
  text: string;
  images: QueuedImageAttachment[];
  created_at: string;
}

export interface SessionQueueResponse {
  messages: QueuedMessage[];
}

export interface AddQueueMessageRequest {
  mode: 'steer' | 'follow-up';
  text: string;
  images?: QueuedImageAttachment[];
}

export interface UpdateQueueMessageRequest {
  text?: string;
  mode?: 'steer' | 'follow-up';
  position?: number;
}

export interface UiState {
  active_session_id: string | null;
  tab_order: string[];
  sidebar_open: boolean;
  last_workspace_id: string | null;
}

export interface OnboardingBaseDirResponse {
  base_dir: string | null;
}

export interface OnboardingProjectEntry {
  name: string;
  path: string;
  modified_at: string;
}

export interface OnboardingProjectsResponse {
  projects: OnboardingProjectEntry[];
}

export interface AddOnboardingProjectRequest {
  path: string;
}

export interface AddOnboardingProjectResponse {
  repository: Repository;
}

// Model types
export interface ModelInfo {
  id: string;
  display_name: string;
  description: string;
  is_default: boolean;
  agent_type: 'claude' | 'codex' | 'gemini' | 'opencode';
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
  agent_type?: 'claude' | 'codex' | 'gemini' | 'opencode';
  agent_mode?: 'build' | 'plan';
}

export interface SetDefaultModelRequest {
  agent_type: 'claude' | 'codex' | 'gemini' | 'opencode';
  model_id: string;
}

// File viewer types
export interface FileViewerTab {
  id: string;
  type: 'file-viewer';
  filePath: string;
  workspaceId: string;
}

export interface FileContentRequest {
  path: string;
}

export interface FileContentResponse {
  content: string;
  encoding: 'utf-8' | 'base64';
  size: number;
  media_type: string;
  exists: boolean;
}
