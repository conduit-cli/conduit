// API client for Conduit REST endpoints

import type {
  Repository,
  Workspace,
  Session,
  SessionEvent,
  Agent,
  HealthResponse,
  ListRepositoriesResponse,
  ListWorkspacesResponse,
  ListSessionsResponse,
  ListSessionEventsResponse,
  ListModelsResponse,
  AgentsResponse,
  CreateRepositoryRequest,
  CreateWorkspaceRequest,
  CreateSessionRequest,
  UpdateSessionRequest,
  SetDefaultModelRequest,
  WorkspaceStatus,
  UiState,
  BootstrapResponse,
  SessionEventsQuery,
  InputHistoryResponse,
  SessionQueueResponse,
  AddQueueMessageRequest,
  UpdateQueueMessageRequest,
  QueuedMessage,
  ExternalSession,
  ListExternalSessionsResponse,
  ImportExternalSessionResponse,
  ForkSessionResponse,
  PrPreflightResponse,
  PrCreateResponse,
  ArchivePreflightResponse,
  OnboardingBaseDirResponse,
  OnboardingProjectsResponse,
  AddOnboardingProjectRequest,
  AddOnboardingProjectResponse,
} from '../types';
import type { Theme, ThemeListResponse } from './themes';

const API_BASE = '/api';

class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
    this.name = 'ApiError';
  }
}

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });

  if (!response.ok) {
    const error = await response.text();
    throw new ApiError(response.status, error);
  }

  if (response.status === 204 || response.status === 205) {
    return undefined as T;
  }

  const contentLength = response.headers.get('content-length');
  if (contentLength === '0') {
    return undefined as T;
  }

  const text = await response.text();
  if (!text) {
    return undefined as T;
  }

  return JSON.parse(text) as T;
}

// Health
export async function getHealth(): Promise<HealthResponse> {
  return request('/health');
}

// Bootstrap
export async function getBootstrap(): Promise<BootstrapResponse> {
  return request('/bootstrap');
}

// Agents
export async function getAgents(): Promise<Agent[]> {
  const response = await request<AgentsResponse>('/agents');
  return response.agents;
}

// Repositories
export async function getRepositories(): Promise<Repository[]> {
  const response = await request<ListRepositoriesResponse>('/repositories');
  return response.repositories;
}

export async function getRepository(id: string): Promise<Repository> {
  return request(`/repositories/${id}`);
}

export async function createRepository(data: CreateRepositoryRequest): Promise<Repository> {
  return request('/repositories', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function deleteRepository(id: string): Promise<void> {
  await request(`/repositories/${id}`, { method: 'DELETE' });
}

// Workspaces
export async function getWorkspaces(): Promise<Workspace[]> {
  const response = await request<ListWorkspacesResponse>('/workspaces');
  return response.workspaces;
}

export async function getRepositoryWorkspaces(repositoryId: string): Promise<Workspace[]> {
  const response = await request<ListWorkspacesResponse>(`/repositories/${repositoryId}/workspaces`);
  return response.workspaces;
}

export async function getWorkspace(id: string): Promise<Workspace> {
  return request(`/workspaces/${id}`);
}

export async function createWorkspace(repositoryId: string, data: CreateWorkspaceRequest): Promise<Workspace> {
  return request(`/repositories/${repositoryId}/workspaces`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function archiveWorkspace(id: string): Promise<void> {
  await request(`/workspaces/${id}/archive`, { method: 'POST' });
}

export async function getWorkspaceArchivePreflight(id: string): Promise<ArchivePreflightResponse> {
  return request(`/workspaces/${id}/archive/preflight`);
}

export async function deleteWorkspace(id: string): Promise<void> {
  await request(`/workspaces/${id}`, { method: 'DELETE' });
}

// Sessions
export async function getSessions(): Promise<Session[]> {
  const response = await request<ListSessionsResponse>('/sessions');
  return response.sessions;
}

export async function getSession(id: string): Promise<Session> {
  return request(`/sessions/${id}`);
}

export async function createSession(data: CreateSessionRequest): Promise<Session> {
  return request('/sessions', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function closeSession(id: string): Promise<void> {
  await request(`/sessions/${id}`, { method: 'DELETE' });
}

export async function updateSession(id: string, data: UpdateSessionRequest): Promise<Session> {
  return request(`/sessions/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
  });
}

// Models
export async function getModels(): Promise<ListModelsResponse> {
  return request('/models');
}

export async function setDefaultModel(payload: SetDefaultModelRequest): Promise<void> {
  await request('/models/default', {
    method: 'PATCH',
    body: JSON.stringify(payload),
  });
}

function buildSessionEventsQuery(query?: SessionEventsQuery) {
  const params = new URLSearchParams();
  if (query?.limit !== undefined) params.set('limit', query.limit.toString());
  if (query?.offset !== undefined) params.set('offset', query.offset.toString());
  if (query?.tail) params.set('tail', 'true');
  return params.toString();
}

export async function getSessionEvents(
  id: string,
  query?: SessionEventsQuery
): Promise<SessionEvent[]> {
  const queryString = buildSessionEventsQuery(query);
  const response = await request<ListSessionEventsResponse>(
    `/sessions/${id}/events${queryString ? `?${queryString}` : ''}`
  );
  return response.events;
}

export async function getSessionEventsPage(
  id: string,
  query?: SessionEventsQuery
): Promise<ListSessionEventsResponse> {
  const queryString = buildSessionEventsQuery(query);
  return request<ListSessionEventsResponse>(
    `/sessions/${id}/events${queryString ? `?${queryString}` : ''}`
  );
}

export async function getSessionHistory(id: string): Promise<InputHistoryResponse> {
  return request(`/sessions/${id}/history`);
}

export async function getSessionQueue(id: string): Promise<SessionQueueResponse> {
  return request(`/sessions/${id}/queue`);
}

export async function addSessionQueueMessage(
  id: string,
  data: AddQueueMessageRequest
): Promise<QueuedMessage> {
  return request(`/sessions/${id}/queue`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateSessionQueueMessage(
  id: string,
  messageId: string,
  data: UpdateQueueMessageRequest
): Promise<QueuedMessage> {
  return request(`/sessions/${id}/queue/${messageId}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
  });
}

export async function deleteSessionQueueMessage(id: string, messageId: string): Promise<void> {
  await request(`/sessions/${id}/queue/${messageId}`, { method: 'DELETE' });
}

// Workspace status
export async function getWorkspaceStatus(id: string): Promise<WorkspaceStatus> {
  return request(`/workspaces/${id}/status`);
}

export async function getWorkspacePrPreflight(id: string): Promise<PrPreflightResponse> {
  return request(`/workspaces/${id}/pr/preflight`);
}

export async function createWorkspacePr(id: string): Promise<PrCreateResponse> {
  return request(`/workspaces/${id}/pr`, { method: 'POST' });
}

// Auto-create workspace (generates name/branch automatically)
export async function autoCreateWorkspace(repositoryId: string): Promise<Workspace> {
  return request(`/repositories/${repositoryId}/workspaces/auto`, {
    method: 'POST',
  });
}

// Get or create session for a workspace
export async function getOrCreateWorkspaceSession(workspaceId: string): Promise<Session> {
  return request(`/workspaces/${workspaceId}/session`, {
    method: 'POST',
  });
}

// External sessions
export async function listExternalSessions(agentType?: string): Promise<ExternalSession[]> {
  const params = agentType ? `?agent_type=${encodeURIComponent(agentType)}` : '';
  const response = await request<ListExternalSessionsResponse>(`/external-sessions${params}`);
  return response.sessions;
}

export async function importExternalSession(id: string): Promise<ImportExternalSessionResponse> {
  return request(`/external-sessions/${id}/import`, {
    method: 'POST',
  });
}

// Fork session
export async function forkSession(id: string): Promise<ForkSessionResponse> {
  return request(`/sessions/${id}/fork`, {
    method: 'POST',
  });
}

// Themes
export async function getThemes(): Promise<ThemeListResponse> {
  return request('/themes');
}

export async function getCurrentTheme(): Promise<Theme> {
  return request('/themes/current');
}

export async function setTheme(name: string): Promise<Theme> {
  return request('/themes/current', {
    method: 'POST',
    body: JSON.stringify({ name }),
  });
}

// UI state
export async function getUiState(): Promise<UiState> {
  return request('/ui/state');
}

export async function updateUiState(data: Partial<UiState>): Promise<UiState> {
  return request('/ui/state', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

// Onboarding
export async function getOnboardingBaseDir(): Promise<OnboardingBaseDirResponse> {
  return request('/onboarding/base-dir');
}

export async function setOnboardingBaseDir(base_dir: string): Promise<OnboardingBaseDirResponse> {
  return request('/onboarding/base-dir', {
    method: 'POST',
    body: JSON.stringify({ base_dir }),
  });
}

export async function listOnboardingProjects(): Promise<OnboardingProjectsResponse> {
  return request('/onboarding/projects');
}

export async function addOnboardingProject(
  data: AddOnboardingProjectRequest
): Promise<AddOnboardingProjectResponse> {
  return request('/onboarding/add-project', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}
