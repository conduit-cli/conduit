import { useMemo, useState, useEffect, useRef } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout, ChatView } from './components';
import { CommandPalette, type CommandPaletteItem } from './components/CommandPalette';
import { SessionImportDialog } from './components/SessionImportDialog';
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
  useWorkspaceSession,
  useCloseSession,
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
  const workspacesQuery = useWorkspaces({ enabled: !!bootstrap });
  const sessionsQuery = useSessions({ enabled: !!bootstrap });
  const { data: uiState } = useUiState({ enabled: !!bootstrap });
  const updateUiState = useUpdateUiState();
  const closeSession = useCloseSession();
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);
  const [autoCreateEnabled, setAutoCreateEnabled] = useState(true);
  const [suppressedWorkspaceIds, setSuppressedWorkspaceIds] = useState<Set<string>>(new Set());
  const [historyReady, setHistoryReady] = useState(false);
  const [isImportDialogOpen, setIsImportDialogOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const previousActiveSessionId = useRef<string | null>(null);
  const bootstrapApplied = useRef(false);

  const resolvedUiState = uiState ?? bootstrap?.ui_state;
  const resolvedWorkspaces = workspacesQuery.data ?? bootstrap?.workspaces ?? [];
  const resolvedSessions = sessionsQuery.data ?? bootstrap?.sessions ?? [];

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
  const allowAutoCreate =
    autoCreateEnabled &&
    (!selectedWorkspaceId || !suppressedWorkspaceIds.has(selectedWorkspaceId));
  const {
    data: workspaceSession,
    isLoading: isLoadingWorkspaceSession,
  } = useWorkspaceSession(selectedWorkspaceId, { enabled: allowAutoCreate });

  const wsEvents = useSessionEvents(activeSessionId);
  const { data: historyEvents = [] } = useSessionEventsFromApi(activeSessionId, {
    enabled: historyReady && !!activeSessionId,
    query: { tail: true, limit: 200 },
  });
  const latestUsage = useMemo(
    () => latestUsageFromEvents(wsEvents, historyEvents),
    [wsEvents, historyEvents]
  );
  const isLoadingSession = isLoadingWorkspaceSession && !activeSessionId;

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
    if (!bootstrap || bootstrapApplied.current) return;
    if (!activeSessionId && bootstrap.active_session) {
      setActiveSessionId(bootstrap.active_session.id);
    }
    if (!selectedWorkspaceId && bootstrap.active_workspace) {
      setSelectedWorkspaceId(bootstrap.active_workspace.id);
    }
    bootstrapApplied.current = true;
  }, [bootstrap, activeSessionId, selectedWorkspaceId]);

  useEffect(() => {
    if (!autoCreateEnabled) return;
    if (!selectedWorkspaceId || !workspaceSession) return;
    if (workspaceSession.workspace_id !== selectedWorkspaceId) return;
    if (workspaceSession.id === activeSessionId) return;
    setActiveSessionId(workspaceSession.id);
    updateUiState.mutate({
      active_session_id: workspaceSession.id,
      last_workspace_id: selectedWorkspaceId,
    });
  }, [activeSessionId, autoCreateEnabled, selectedWorkspaceId, updateUiState, workspaceSession]);

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
    setAutoCreateEnabled(true);
    setSuppressedWorkspaceIds((prev) => {
      if (!prev.has(workspace.id)) return prev;
      const next = new Set(prev);
      next.delete(workspace.id);
      return next;
    });
    setSelectedWorkspaceId(workspace.id);
  };

  const handleSelectSession = (session: Session) => {
    setAutoCreateEnabled(true);
    setActiveSessionId(session.id);
    updateUiState.mutate({
      active_session_id: session.id,
      last_workspace_id: session.workspace_id ?? null,
    });
    if (session.workspace_id) {
      setSuppressedWorkspaceIds((prev) => {
        if (!prev.has(session.workspace_id!)) return prev;
        const next = new Set(prev);
        next.delete(session.workspace_id!);
        return next;
      });
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

  const handleCloseSession = (sessionId: string) => {
    const currentIndex = orderedSessions.findIndex((s) => s.id === sessionId);
    const isActiveTab = sessionId === activeSessionId;
    const sessionToClose = orderedSessions.find((session) => session.id === sessionId) ?? null;
    if (sessionToClose?.workspace_id) {
      setSuppressedWorkspaceIds((prev) => {
        const next = new Set(prev);
        next.add(sessionToClose.workspace_id!);
        return next;
      });
    }

    if (isActiveTab) {
      if (orderedSessions.length > 1) {
        // Prefer next tab, fallback to previous
        const nextIndex =
          currentIndex < orderedSessions.length - 1 ? currentIndex + 1 : currentIndex - 1;
        const nextSession = orderedSessions[nextIndex];
        setActiveSessionId(nextSession.id);
        updateUiState.mutate({ active_session_id: nextSession.id });
        if (nextSession.workspace_id) {
          setSelectedWorkspaceId(nextSession.workspace_id);
          updateUiState.mutate({ last_workspace_id: nextSession.workspace_id });
        }
      } else {
        // Last tab
        setAutoCreateEnabled(false);
        setActiveSessionId(null);
        updateUiState.mutate({ active_session_id: null });
      }
    }

    // Remove from tab_order
    const newTabOrder = (resolvedUiState?.tab_order ?? []).filter((id) => id !== sessionId);
    updateUiState.mutate({ tab_order: newTabOrder });

    // Close the session via API
    closeSession.mutate(sessionId);
  };

  const handleNewSession = () => {
    // TODO: Open new session dialog
    console.log('New session requested');
  };

  const handleOpenImport = () => {
    setIsImportDialogOpen(true);
  };

  const handleImportedSession = (session: Session) => {
    setActiveSessionId(session.id);
    updateUiState.mutate({ active_session_id: session.id });

    if (session.workspace_id) {
      setSelectedWorkspaceId(session.workspace_id);
      updateUiState.mutate({ last_workspace_id: session.workspace_id });
    }

    const currentOrder = resolvedUiState?.tab_order ?? [];
    if (!currentOrder.includes(session.id)) {
      updateUiState.mutate({ tab_order: [...currentOrder, session.id] });
    }
  };

  const commands = useMemo<CommandPaletteItem[]>(
    () => [
      {
        id: 'toggle-sidebar',
        label: isSidebarOpen ? 'Hide Sidebar' : 'Show Sidebar',
        shortcut: 'Ctrl+B',
        onSelect: handleToggleSidebar,
      },
      {
        id: 'import-session',
        label: 'Import Session',
        shortcut: 'Ctrl+I',
        onSelect: handleOpenImport,
      },
      {
        id: 'new-session',
        label: 'Start New Session',
        onSelect: handleNewSession,
        disabled: true,
      },
    ],
    [handleNewSession, handleOpenImport, handleToggleSidebar, isSidebarOpen]
  );

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setIsCommandPaletteOpen((prev) => !prev);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <>
      <Layout
        selectedWorkspaceId={selectedWorkspaceId}
        onSelectWorkspace={handleSelectWorkspace}
        sessions={orderedSessions}
        activeSessionId={activeSessionId}
        onSelectSession={handleSelectSession}
        onReorderSessions={handleReorderSessions}
        onCloseSession={handleCloseSession}
        workspaces={resolvedWorkspaces}
        activeWorkspace={activeWorkspace ?? null}
        workspaceStatus={workspaceStatus ?? null}
        latestUsage={latestUsage}
        isSidebarOpen={isSidebarOpen}
        onToggleSidebar={handleToggleSidebar}
        onImportSession={handleOpenImport}
        isBootstrapping={isBootstrapping}
      >
        <ChatView
          session={activeSession}
          onNewSession={handleNewSession}
          isLoadingSession={isLoadingSession}
          onForkedSession={handleImportedSession}
        />
      </Layout>
      <SessionImportDialog
        isOpen={isImportDialogOpen}
        onClose={() => setIsImportDialogOpen(false)}
        onImported={handleImportedSession}
      />
      <CommandPalette
        isOpen={isCommandPaletteOpen}
        onClose={() => setIsCommandPaletteOpen(false)}
        commands={commands}
      />
    </>
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
