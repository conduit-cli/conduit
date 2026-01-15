// React Query hooks for API access

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as api from '../lib/api';
import type { CreateRepositoryRequest, CreateWorkspaceRequest, CreateSessionRequest } from '../types';

// Query keys
export const queryKeys = {
  health: ['health'] as const,
  agents: ['agents'] as const,
  repositories: ['repositories'] as const,
  repository: (id: string) => ['repositories', id] as const,
  workspaces: ['workspaces'] as const,
  repositoryWorkspaces: (id: string) => ['repositories', id, 'workspaces'] as const,
  workspace: (id: string) => ['workspaces', id] as const,
  workspaceStatus: (id: string) => ['workspaces', id, 'status'] as const,
  workspaceSession: (id: string) => ['workspaces', id, 'session'] as const,
  sessions: ['sessions'] as const,
  session: (id: string) => ['sessions', id] as const,
  sessionEvents: (id: string) => ['sessions', id, 'events'] as const,
  uiState: ['ui', 'state'] as const,
};

// Health
export function useHealth() {
  return useQuery({
    queryKey: queryKeys.health,
    queryFn: api.getHealth,
    staleTime: 30000,
  });
}

// Agents
export function useAgents() {
  return useQuery({
    queryKey: queryKeys.agents,
    queryFn: api.getAgents,
    staleTime: 60000,
  });
}

// Repositories
export function useRepositories() {
  return useQuery({
    queryKey: queryKeys.repositories,
    queryFn: api.getRepositories,
  });
}

export function useRepository(id: string) {
  return useQuery({
    queryKey: queryKeys.repository(id),
    queryFn: () => api.getRepository(id),
    enabled: !!id,
  });
}

export function useCreateRepository() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateRepositoryRequest) => api.createRepository(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.repositories });
    },
  });
}

export function useDeleteRepository() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteRepository(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.repositories });
    },
  });
}

// Workspaces
export function useWorkspaces() {
  return useQuery({
    queryKey: queryKeys.workspaces,
    queryFn: api.getWorkspaces,
  });
}

export function useRepositoryWorkspaces(repositoryId: string) {
  return useQuery({
    queryKey: queryKeys.repositoryWorkspaces(repositoryId),
    queryFn: () => api.getRepositoryWorkspaces(repositoryId),
    enabled: !!repositoryId,
  });
}

export function useWorkspace(id: string) {
  return useQuery({
    queryKey: queryKeys.workspace(id),
    queryFn: () => api.getWorkspace(id),
    enabled: !!id,
  });
}

export function useCreateWorkspace(repositoryId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateWorkspaceRequest) => api.createWorkspace(repositoryId, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      queryClient.invalidateQueries({ queryKey: queryKeys.repositoryWorkspaces(repositoryId) });
    },
  });
}

export function useArchiveWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.archiveWorkspace(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
    },
  });
}

export function useDeleteWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteWorkspace(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
    },
  });
}

export function useWorkspaceStatus(workspaceId: string | null) {
  return useQuery({
    queryKey: queryKeys.workspaceStatus(workspaceId ?? ''),
    queryFn: () => api.getWorkspaceStatus(workspaceId!),
    enabled: !!workspaceId,
    refetchInterval: 5000, // Poll every 5 seconds
    staleTime: 2000,
  });
}

export function useAutoCreateWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (repositoryId: string) => api.autoCreateWorkspace(repositoryId),
    onSuccess: (_data, repositoryId) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      queryClient.invalidateQueries({ queryKey: queryKeys.repositoryWorkspaces(repositoryId) });
    },
  });
}

// Get or create session for a workspace
// This auto-creates a session if one doesn't exist, matching TUI behavior
export function useWorkspaceSession(workspaceId: string | null) {
  const queryClient = useQueryClient();

  return useQuery({
    queryKey: queryKeys.workspaceSession(workspaceId ?? ''),
    queryFn: async () => {
      const session = await api.getOrCreateWorkspaceSession(workspaceId!);
      // Invalidate sessions list since we may have created a new one
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      return session;
    },
    enabled: !!workspaceId,
    staleTime: Infinity, // Session won't change unless we create a new one
  });
}

// Sessions
export function useSessions() {
  return useQuery({
    queryKey: queryKeys.sessions,
    queryFn: api.getSessions,
  });
}

export function useSession(id: string) {
  return useQuery({
    queryKey: queryKeys.session(id),
    queryFn: () => api.getSession(id),
    enabled: !!id,
  });
}

export function useCreateSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateSessionRequest) => api.createSession(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

export function useCloseSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.closeSession(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

export function useSessionEventsFromApi(id: string | null) {
  return useQuery({
    queryKey: queryKeys.sessionEvents(id ?? ''),
    queryFn: () => api.getSessionEvents(id!),
    enabled: !!id,
    staleTime: 5000, // Cache for 5 seconds
  });
}

// UI state
export function useUiState() {
  return useQuery({
    queryKey: queryKeys.uiState,
    queryFn: api.getUiState,
    staleTime: 0,
  });
}

export function useUpdateUiState() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.updateUiState,
    onSuccess: (data) => {
      queryClient.setQueryData(queryKeys.uiState, data);
    },
  });
}
