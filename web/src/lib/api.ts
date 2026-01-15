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
  AgentsResponse,
  CreateRepositoryRequest,
  CreateWorkspaceRequest,
  CreateSessionRequest,
  WorkspaceStatus,
  UiState,
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

  return response.json();
}

// Health
export async function getHealth(): Promise<HealthResponse> {
  return request('/health');
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

export async function getSessionEvents(id: string): Promise<SessionEvent[]> {
  const response = await request<ListSessionEventsResponse>(`/sessions/${id}/events`);
  return response.events;
}

// Workspace status
export async function getWorkspaceStatus(id: string): Promise<WorkspaceStatus> {
  return request(`/workspaces/${id}/status`);
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
