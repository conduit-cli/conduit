import { useMemo, useState, useEffect } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout, ChatView } from './components';
import { WebSocketProvider, ThemeProvider } from './hooks';
import {
  useWorkspaces,
  useWorkspaceSession,
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
  const { data: workspaces = [] } = useWorkspaces();
  const { data: sessions = [] } = useSessions();
  const { data: uiState } = useUiState();
  const updateUiState = useUpdateUiState();
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);

  const sortedSessions = useMemo(
    () => [...sessions].sort((a, b) => a.tab_index - b.tab_index),
    [sessions]
  );
  const orderedSessions = useMemo(
    () => applyTabOrder(sortedSessions, uiState?.tab_order ?? []),
    [sortedSessions, uiState?.tab_order]
  );

  const activeSession = orderedSessions.find((session) => session.id === activeSessionId) ?? null;
  const { data: activeWorkspace } = useWorkspace(activeSession?.workspace_id ?? '');
  const { data: workspaceStatus } = useWorkspaceStatus(activeSession?.workspace_id ?? null);

  const wsEvents = useSessionEvents(activeSessionId);
  const { data: historyEvents = [] } = useSessionEventsFromApi(activeSessionId);
  const latestUsage = useMemo(
    () => latestUsageFromEvents(wsEvents, historyEvents),
    [wsEvents, historyEvents]
  );

  // Get or create session for selected workspace (auto-creates if needed)
  const { data: workspaceSession, isLoading: isLoadingSession } =
    useWorkspaceSession(selectedWorkspaceId);

  useEffect(() => {
    if (!uiState) return;
    setIsSidebarOpen(uiState.sidebar_open);
  }, [uiState]);

  useEffect(() => {
    if (!uiState) return;
    const mergedOrder = mergeTabOrder(uiState.tab_order ?? [], sortedSessions);
    if (mergedOrder.join(',') !== (uiState.tab_order ?? []).join(',')) {
      updateUiState.mutate({ tab_order: mergedOrder });
    }
  }, [sortedSessions, uiState, updateUiState]);

  useEffect(() => {
    if (activeSessionId || orderedSessions.length === 0 || !uiState) return;
    const preferred =
      uiState.active_session_id &&
      orderedSessions.some((session) => session.id === uiState.active_session_id)
        ? uiState.active_session_id
        : orderedSessions[0].id;
    setActiveSessionId(preferred);
  }, [activeSessionId, orderedSessions, uiState]);

  useEffect(() => {
    if (selectedWorkspaceId || workspaces.length === 0) return;
    const lastWorkspace =
      uiState?.last_workspace_id &&
      workspaces.some((workspace) => workspace.id === uiState.last_workspace_id)
        ? uiState.last_workspace_id
        : null;
    const nextWorkspace = lastWorkspace ?? activeSession?.workspace_id ?? workspaces[0].id;
    if (nextWorkspace) {
      setSelectedWorkspaceId(nextWorkspace);
    }
  }, [selectedWorkspaceId, workspaces, uiState?.last_workspace_id, activeSession?.workspace_id]);

  useEffect(() => {
    if (!workspaceSession) return;
    if (workspaceSession.id !== activeSessionId) {
      setActiveSessionId(workspaceSession.id);
      updateUiState.mutate({
        active_session_id: workspaceSession.id,
        last_workspace_id: workspaceSession.workspace_id ?? null,
      });
    }
  }, [workspaceSession, activeSessionId, updateUiState]);

  useEffect(() => {
    if (!activeSessionId || uiState?.active_session_id === activeSessionId) return;
    updateUiState.mutate({ active_session_id: activeSessionId });
  }, [activeSessionId, uiState?.active_session_id, updateUiState]);

  useEffect(() => {
    if (!activeSession?.workspace_id) return;
    if (activeSession.workspace_id === selectedWorkspaceId) return;
    setSelectedWorkspaceId(activeSession.workspace_id);
    updateUiState.mutate({ last_workspace_id: activeSession.workspace_id });
  }, [activeSession?.workspace_id, selectedWorkspaceId, updateUiState]);

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
      workspaces={workspaces}
      activeWorkspace={activeWorkspace ?? null}
      workspaceStatus={workspaceStatus ?? null}
      latestUsage={latestUsage}
      isSidebarOpen={isSidebarOpen}
      onToggleSidebar={handleToggleSidebar}
    >
      <ChatView
        session={activeSession}
        onNewSession={handleNewSession}
        isLoadingSession={isLoadingSession}
      />
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
