// React Query hooks for API access

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as api from '../lib/api';
import type {
  CreateRepositoryRequest,
  CreateWorkspaceRequest,
  CreateSessionRequest,
  UpdateSessionRequest,
  Session,
  SessionEventsQuery,
  SetDefaultModelRequest,
  AddQueueMessageRequest,
  UpdateQueueMessageRequest,
} from '../types';

// Query keys
export const queryKeys = {
  health: ['health'] as const,
  agents: ['agents'] as const,
  models: ['models'] as const,
  repositories: ['repositories'] as const,
  repository: (id: string) => ['repositories', id] as const,
  workspaces: ['workspaces'] as const,
  repositoryWorkspaces: (id: string) => ['repositories', id, 'workspaces'] as const,
  workspace: (id: string) => ['workspaces', id] as const,
  workspaceStatus: (id: string) => ['workspaces', id, 'status'] as const,
  workspacePrPreflight: (id: string) => ['workspaces', id, 'pr-preflight'] as const,
  workspaceSession: (id: string) => ['workspaces', id, 'session'] as const,
  sessions: ['sessions'] as const,
  session: (id: string) => ['sessions', id] as const,
  sessionEvents: (id: string, query?: SessionEventsQuery) =>
    ['sessions', id, 'events', query ?? {}] as const,
  sessionHistory: (id: string) => ['sessions', id, 'history'] as const,
  sessionQueue: (id: string) => ['sessions', id, 'queue'] as const,
  externalSessions: (agentType?: string | null) =>
    ['external-sessions', agentType ?? 'all'] as const,
  uiState: ['ui', 'state'] as const,
  bootstrap: ['bootstrap'] as const,
};

// Health
export function useHealth() {
  return useQuery({
    queryKey: queryKeys.health,
    queryFn: api.getHealth,
    staleTime: 30000,
  });
}

// Bootstrap
export function useBootstrap() {
  const queryClient = useQueryClient();
  return useQuery({
    queryKey: queryKeys.bootstrap,
    queryFn: async () => {
      const data = await api.getBootstrap();
      queryClient.setQueryData(queryKeys.uiState, data.ui_state);
      queryClient.setQueryData(queryKeys.sessions, data.sessions);
      queryClient.setQueryData(queryKeys.workspaces, data.workspaces);
      if (data.active_session) {
        queryClient.setQueryData(queryKeys.session(data.active_session.id), data.active_session);
      }
      if (data.active_workspace) {
        queryClient.setQueryData(queryKeys.workspace(data.active_workspace.id), data.active_workspace);
      }
      return data;
    },
    staleTime: 0,
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
export function useWorkspaces(options?: { enabled?: boolean; staleTime?: number }) {
  return useQuery({
    queryKey: queryKeys.workspaces,
    queryFn: api.getWorkspaces,
    enabled: options?.enabled,
    staleTime: options?.staleTime,
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

export function useWorkspaceStatus(
  workspaceId: string | null,
  options?: { enabled?: boolean; refetchInterval?: number | false; staleTime?: number }
) {
  return useQuery({
    queryKey: queryKeys.workspaceStatus(workspaceId ?? ''),
    queryFn: () => api.getWorkspaceStatus(workspaceId!),
    enabled: options?.enabled ?? !!workspaceId,
    refetchInterval: options?.refetchInterval ?? 5000,
    staleTime: options?.staleTime ?? 2000,
  });
}

export function useWorkspacePrPreflight(workspaceId: string | null, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: queryKeys.workspacePrPreflight(workspaceId ?? ''),
    queryFn: () => api.getWorkspacePrPreflight(workspaceId!),
    enabled: (options?.enabled ?? true) && !!workspaceId,
    staleTime: 0,
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
export function useWorkspaceSession(
  workspaceId: string | null,
  options?: { enabled?: boolean }
) {
  const queryClient = useQueryClient();

  return useQuery({
    queryKey: queryKeys.workspaceSession(workspaceId ?? ''),
    queryFn: async () => {
      const session = await api.getOrCreateWorkspaceSession(workspaceId!);
      // Invalidate sessions list since we may have created a new one
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      return session;
    },
    enabled: (options?.enabled ?? true) && !!workspaceId,
    staleTime: Infinity, // Session won't change unless we create a new one
  });
}

// Sessions
export function useSessions(options?: { enabled?: boolean; staleTime?: number }) {
  return useQuery({
    queryKey: queryKeys.sessions,
    queryFn: api.getSessions,
    enabled: options?.enabled,
    staleTime: options?.staleTime,
  });
}

export function useExternalSessions(agentType?: string | null, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: queryKeys.externalSessions(agentType),
    queryFn: () => api.listExternalSessions(agentType ?? undefined),
    enabled: options?.enabled ?? true,
    staleTime: 5000,
  });
}

export function useSession(id: string) {
  return useQuery({
    queryKey: queryKeys.session(id),
    queryFn: () => api.getSession(id),
    enabled: !!id,
  });
}

export function useSessionHistory(sessionId: string | null, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: queryKeys.sessionHistory(sessionId ?? ''),
    queryFn: () => api.getSessionHistory(sessionId!),
    enabled: (options?.enabled ?? true) && !!sessionId,
    staleTime: 5000,
  });
}

export function useSessionQueue(sessionId: string | null, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: queryKeys.sessionQueue(sessionId ?? ''),
    queryFn: () => api.getSessionQueue(sessionId!),
    enabled: (options?.enabled ?? true) && !!sessionId,
    staleTime: 2000,
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

export function useAddQueueMessage() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: AddQueueMessageRequest }) =>
      api.addSessionQueueMessage(id, data),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessionQueue(variables.id) });
      queryClient.invalidateQueries({ queryKey: queryKeys.sessionHistory(variables.id) });
    },
  });
}

export function useUpdateQueueMessage() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      messageId,
      data,
    }: {
      id: string;
      messageId: string;
      data: UpdateQueueMessageRequest;
    }) => api.updateSessionQueueMessage(id, messageId, data),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessionQueue(variables.id) });
    },
  });
}

export function useDeleteQueueMessage() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, messageId }: { id: string; messageId: string }) =>
      api.deleteSessionQueueMessage(id, messageId),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessionQueue(variables.id) });
    },
  });
}

export function useCloseSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.closeSession(id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.sessions });
      const previous = queryClient.getQueryData<Session[]>(queryKeys.sessions);
      if (previous) {
        queryClient.setQueryData(
          queryKeys.sessions,
          previous.filter((session) => session.id !== id)
        );
      }
      return { previous };
    },
    onError: (_error, _id, context) => {
      if (context?.previous) {
        queryClient.setQueryData(queryKeys.sessions, context.previous);
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

export function useImportExternalSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.importExternalSession(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      queryClient.invalidateQueries({ queryKey: queryKeys.repositories });
    },
  });
}

export function useUpdateSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateSessionRequest }) =>
      api.updateSession(id, data),
    onSuccess: (updatedSession) => {
      queryClient.setQueryData(queryKeys.session(updatedSession.id), updatedSession);
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

export function useForkSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.forkSession(id),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaceStatus(response.workspace.id) });
    },
  });
}

export function useCreateWorkspacePr() {
  return useMutation({
    mutationFn: (workspaceId: string) => api.createWorkspacePr(workspaceId),
  });
}

// Models
export function useModels() {
  return useQuery({
    queryKey: queryKeys.models,
    queryFn: api.getModels,
    staleTime: 60000, // Models don't change often
  });
}

export function useSetDefaultModel() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: SetDefaultModelRequest) => api.setDefaultModel(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.models });
    },
  });
}

export function useSessionEventsFromApi(
  id: string | null,
  options?: { enabled?: boolean; staleTime?: number; query?: SessionEventsQuery }
) {
  return useQuery({
    queryKey: queryKeys.sessionEvents(id ?? '', options?.query),
    queryFn: () => api.getSessionEvents(id!, options?.query),
    enabled: options?.enabled ?? !!id,
    staleTime: options?.staleTime ?? 5000,
  });
}

// UI state
export function useUiState(options?: { enabled?: boolean; staleTime?: number }) {
  return useQuery({
    queryKey: queryKeys.uiState,
    queryFn: api.getUiState,
    staleTime: options?.staleTime ?? 0,
    enabled: options?.enabled,
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
