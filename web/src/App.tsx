import { useMemo, useState, useEffect, useRef } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout, ChatView } from './components';
import { WebSocketProvider, ThemeProvider } from './hooks';
import {
  useBootstrap,
  useWorkspaces,
  useSessions,
  useUiState,
  useUpdateUiState,
  useWorkspace,
  useWorkspaceStatus,
  useSessionEventsFromApi,
  useSessionEvents,
} from './hooks';
import type { Workspace, Session, SessionEvent, AgentEvent } from './types';

// Create a client
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5000,
      refetchOnWindowFocus: false,
    },
  },
});

function mergeTabOrder(order: string[], sessions: Session[]): string[] {
  const sessionIds = sessions.map((session) => session.id);
  const ordered = order.filter((id) => sessionIds.includes(id));
  const missing = sessionIds.filter((id) => !ordered.includes(id));
  return [...ordered, ...missing];
}

function applyTabOrder(sessions: Session[], order: string[]): Session[] {
  if (order.length === 0) return sessions;
  const sessionMap = new Map(sessions.map((session) => [session.id, session]));
  const ordered = order
    .map((id) => sessionMap.get(id))
    .filter((session): session is Session => Boolean(session));
  const missing = sessions.filter((session) => !order.includes(session.id));
  return [...ordered, ...missing];
}

function latestUsageFromEvents(wsEvents: AgentEvent[], historyEvents: SessionEvent[]) {
  for (let index = wsEvents.length - 1; index >= 0; index -= 1) {
    const event = wsEvents[index];
    if (event.type === 'TurnCompleted') {
      return {
        input_tokens: event.usage.input_tokens,
        output_tokens: event.usage.output_tokens,
      };
    }
  }

  for (let index = historyEvents.length - 1; index >= 0; index -= 1) {
    const event = historyEvents[index];
    if (event.role === 'summary' && event.summary) {
      return {
        input_tokens: event.summary.input_tokens,
        output_tokens: event.summary.output_tokens,
      };
    }
  }

  return null;
}

function AppContent() {
  const { data: bootstrap, isLoading: isBootstrapping } = useBootstrap();
  const { data: workspaces = [] } = useWorkspaces({ enabled: !!bootstrap });
  const { data: sessions = [] } = useSessions({ enabled: !!bootstrap });
  const { data: uiState } = useUiState({ enabled: !!bootstrap });
  const updateUiState = useUpdateUiState();
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);
  const [historyReady, setHistoryReady] = useState(false);
  const previousActiveSessionId = useRef<string | null>(null);

  const resolvedUiState = uiState ?? bootstrap?.ui_state;
  const resolvedWorkspaces = workspaces.length > 0 ? workspaces : bootstrap?.workspaces ?? [];
  const resolvedSessions = sessions.length > 0 ? sessions : bootstrap?.sessions ?? [];

  const sortedSessions = useMemo(
    () => [...resolvedSessions].sort((a, b) => a.tab_index - b.tab_index),
    [resolvedSessions]
  );
  const orderedSessions = useMemo(
    () => applyTabOrder(sortedSessions, resolvedUiState?.tab_order ?? []),
    [sortedSessions, resolvedUiState?.tab_order]
  );

  const activeSession = orderedSessions.find((session) => session.id === activeSessionId) ?? null;
  const { data: activeWorkspace } = useWorkspace(activeSession?.workspace_id ?? '');
  const { data: workspaceStatus } = useWorkspaceStatus(activeSession?.workspace_id ?? null);

  const wsEvents = useSessionEvents(activeSessionId);
  const { data: historyEvents = [] } = useSessionEventsFromApi(activeSessionId, {
    enabled: historyReady && !!activeSessionId,
    query: { tail: true, limit: 200 },
  });
  const latestUsage = useMemo(
    () => latestUsageFromEvents(wsEvents, historyEvents),
    [wsEvents, historyEvents]
  );

  useEffect(() => {
    setHistoryReady(true);
  }, []);

  useEffect(() => {
    if (!resolvedUiState) return;
    setIsSidebarOpen(resolvedUiState.sidebar_open);
  }, [resolvedUiState]);

  useEffect(() => {
    if (!resolvedUiState) return;
    const mergedOrder = mergeTabOrder(resolvedUiState.tab_order ?? [], sortedSessions);
    if (mergedOrder.join(',') !== (resolvedUiState.tab_order ?? []).join(',')) {
      updateUiState.mutate({ tab_order: mergedOrder });
    }
  }, [sortedSessions, resolvedUiState, updateUiState]);

  useEffect(() => {
    if (!bootstrap) return;
    if (!activeSessionId && bootstrap.active_session) {
      setActiveSessionId(bootstrap.active_session.id);
    }
    if (!selectedWorkspaceId && bootstrap.active_workspace) {
      setSelectedWorkspaceId(bootstrap.active_workspace.id);
    }
  }, [bootstrap, activeSessionId, selectedWorkspaceId]);

  useEffect(() => {
    if (activeSessionId || orderedSessions.length === 0 || !resolvedUiState) return;
    const preferred =
      resolvedUiState.active_session_id &&
      orderedSessions.some((session) => session.id === resolvedUiState.active_session_id)
        ? resolvedUiState.active_session_id
        : orderedSessions[0].id;
    setActiveSessionId(preferred);
  }, [activeSessionId, orderedSessions, resolvedUiState]);

  useEffect(() => {
    if (selectedWorkspaceId || resolvedWorkspaces.length === 0) return;
    const lastWorkspace =
      resolvedUiState?.last_workspace_id &&
      resolvedWorkspaces.some((workspace) => workspace.id === resolvedUiState.last_workspace_id)
        ? resolvedUiState.last_workspace_id
        : null;
    const nextWorkspace = lastWorkspace ?? activeSession?.workspace_id ?? resolvedWorkspaces[0].id;
    if (nextWorkspace) {
      setSelectedWorkspaceId(nextWorkspace);
    }
  }, [
    selectedWorkspaceId,
    resolvedWorkspaces,
    resolvedUiState?.last_workspace_id,
    activeSession?.workspace_id,
  ]);

  useEffect(() => {
    if (!activeSessionId || resolvedUiState?.active_session_id === activeSessionId) return;
    updateUiState.mutate({ active_session_id: activeSessionId });
  }, [activeSessionId, resolvedUiState?.active_session_id, updateUiState]);

  useEffect(() => {
    if (!activeSessionId) {
      previousActiveSessionId.current = null;
      return;
    }
    if (previousActiveSessionId.current === activeSessionId) {
      return;
    }
    previousActiveSessionId.current = activeSessionId;
    if (!activeSession?.workspace_id) return;
    if (activeSession.workspace_id === selectedWorkspaceId) return;
    setSelectedWorkspaceId(activeSession.workspace_id);
    updateUiState.mutate({ last_workspace_id: activeSession.workspace_id });
  }, [activeSessionId, activeSession?.workspace_id, selectedWorkspaceId, updateUiState]);

  const handleSelectWorkspace = (workspace: Workspace) => {
    setSelectedWorkspaceId(workspace.id);
    updateUiState.mutate({ last_workspace_id: workspace.id });
  };

  const handleSelectSession = (session: Session) => {
    setActiveSessionId(session.id);
    updateUiState.mutate({
      active_session_id: session.id,
      last_workspace_id: session.workspace_id ?? null,
    });
    if (session.workspace_id) {
      setSelectedWorkspaceId(session.workspace_id);
    }
  };

  const handleReorderSessions = (sessionIds: string[]) => {
    updateUiState.mutate({ tab_order: sessionIds });
  };

  const handleToggleSidebar = () => {
    setIsSidebarOpen((prev) => {
      const next = !prev;
      updateUiState.mutate({ sidebar_open: next });
      return next;
    });
  };

  const handleNewSession = () => {
    // TODO: Open new session dialog
    console.log('New session requested');
  };

  return (
      <Layout
        selectedWorkspaceId={selectedWorkspaceId}
        onSelectWorkspace={handleSelectWorkspace}
        sessions={orderedSessions}
        activeSessionId={activeSessionId}
        onSelectSession={handleSelectSession}
        onReorderSessions={handleReorderSessions}
        workspaces={resolvedWorkspaces}
        activeWorkspace={activeWorkspace ?? null}
        workspaceStatus={workspaceStatus ?? null}
        latestUsage={latestUsage}
        isSidebarOpen={isSidebarOpen}
        onToggleSidebar={handleToggleSidebar}
        isBootstrapping={isBootstrapping}
      >
        <ChatView session={activeSession} onNewSession={handleNewSession} isLoadingSession={false} />
      </Layout>

  );
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <WebSocketProvider>
          <AppContent />
        </WebSocketProvider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}

export default App;
