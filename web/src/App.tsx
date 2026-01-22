import { useMemo, useState, useEffect, useRef, useCallback } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  Layout,
  ChatView,
  OnboardingEmptyState,
  BaseDirDialog,
  ProjectPickerDialog,
  AddProjectDialog,
  CreateWorkspaceDialog,
  ConfirmDialog,
} from './components';
import { CommandPalette, type CommandPaletteItem } from './components/CommandPalette';
import { SessionImportDialog } from './components/SessionImportDialog';
import { FileViewer } from './components/FileViewer';
import { FileViewerContext } from './contexts/FileViewerContext';
import type { FileViewerTab } from './types';
import { WebSocketProvider, ThemeProvider } from './hooks';
import {
  useBootstrap,
  useRepositories,
  useWorkspaces,
  useSessions,
  useUiState,
  useUpdateUiState,
  useWorkspace,
  useWorkspaceStatus,
  useSessionEventsFromApi,
  useSessionEvents,
  useGetOrCreateWorkspaceSession,
  useCloseSession,
  useOnboardingBaseDir,
  useWorkspaceArchivePreflight,
  useArchiveWorkspace,
  useAutoCreateWorkspace,
  useRepositoryRemovePreflight,
  useRemoveRepository,
  useUpdateRepositorySettings,
  useWorkspaceActions,
  useUpdateSession,
} from './hooks';
import type { Repository, Workspace, Session, SessionEvent, AgentEvent, WorkspaceMode } from './types';

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

function parseGitHubRepo(repoUrl: string): string | null {
  if (!repoUrl) return null;
  if (repoUrl.startsWith('git@')) {
    const match = repoUrl.match(/git@[^:]+:([^/]+\/[^/]+?)(?:\.git)?$/);
    return match?.[1] ?? null;
  }
  try {
    const url = new URL(repoUrl);
    if (!url.hostname.endsWith('github.com')) return null;
    const parts = url.pathname.replace(/^\//, '').replace(/\.git$/, '').split('/');
    if (parts.length < 2) return null;
    return `${parts[0]}/${parts[1]}`;
  } catch {
    return null;
  }
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
  const repositoriesQuery = useRepositories({ enabled: !!bootstrap });
  const workspacesQuery = useWorkspaces({ enabled: !!bootstrap });
  const sessionsQuery = useSessions({ enabled: !!bootstrap });
  const { data: uiState } = useUiState({ enabled: !!bootstrap });
  const updateUiState = useUpdateUiState();
  const closeSession = useCloseSession();
  const archiveWorkspace = useArchiveWorkspace();
  const autoCreateWorkspace = useAutoCreateWorkspace();
  const updateRepositorySettings = useUpdateRepositorySettings();
  const updateSession = useUpdateSession();
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);
  const [historyReady, setHistoryReady] = useState(false);
  const [isImportDialogOpen, setIsImportDialogOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isBaseDirDialogOpen, setIsBaseDirDialogOpen] = useState(false);
  const [isProjectPickerOpen, setIsProjectPickerOpen] = useState(false);
  const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);
  const [createWorkspaceRepo, setCreateWorkspaceRepo] = useState<Repository | null>(null);
  const [workspaceModeTarget, setWorkspaceModeTarget] = useState<Repository | null>(null);
  const [pendingWorkspaceRepoId, setPendingWorkspaceRepoId] = useState<string | null>(null);
  const [archiveWorkspaceTarget, setArchiveWorkspaceTarget] = useState<Workspace | null>(null);
  const [archiveRemotePromptTarget, setArchiveRemotePromptTarget] = useState<Workspace | null>(null);
  const [removeRepositoryTarget, setRemoveRepositoryTarget] = useState<Repository | null>(null);
  const [fileViewerTabs, setFileViewerTabs] = useState<FileViewerTab[]>([]);
  const [activeFileViewerId, setActiveFileViewerId] = useState<string | null>(null);
  const previousActiveSessionId = useRef<string | null>(null);
  const bootstrapApplied = useRef(false);

  const resolvedUiState = uiState ?? bootstrap?.ui_state;
  const resolvedRepositories = repositoriesQuery.data ?? [];
  const resolvedWorkspaces = workspacesQuery.data ?? bootstrap?.workspaces ?? [];
  const resolvedSessions = sessionsQuery.data ?? bootstrap?.sessions ?? [];
  const { data: onboardingBaseDir } = useOnboardingBaseDir({ enabled: !!bootstrap });

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
  const selectedWorkspace =
    activeWorkspace ??
    resolvedWorkspaces.find((workspace) => workspace.id === selectedWorkspaceId) ??
    null;
  const openWorkspaceSession = useGetOrCreateWorkspaceSession();

  const wsEvents = useSessionEvents(activeSessionId);
  const { data: historyEvents = [] } = useSessionEventsFromApi(activeSessionId, {
    enabled: historyReady && !!activeSessionId,
    query: { tail: true, limit: 200 },
  });
  const latestUsage = useMemo(
    () => latestUsageFromEvents(wsEvents, historyEvents),
    [wsEvents, historyEvents]
  );
  const isLoadingSession = openWorkspaceSession.isPending && !activeSessionId;
  const { data: archivePreflight } = useWorkspaceArchivePreflight(
    archiveWorkspaceTarget?.id ?? null,
    { enabled: !!archiveWorkspaceTarget }
  );
  const { data: removeRepositoryPreflight } = useRepositoryRemovePreflight(
    removeRepositoryTarget?.id ?? null,
    { enabled: !!removeRepositoryTarget }
  );
  const removeRepository = useRemoveRepository();
  const archiveRepo = archiveWorkspaceTarget
    ? resolvedRepositories.find((candidate) => candidate.id === archiveWorkspaceTarget.repository_id)
    : null;
  const archiveDescription = useMemo(() => {
    if (!archiveRepo) return 'This will remove the workspace and delete the branch.';
    const mode = archiveRepo.workspace_mode_effective;
    const deleteBranch = archiveRepo.archive_delete_branch_effective;
    const remotePrompt = archiveRepo.archive_remote_prompt_effective;

    let description =
      mode === 'checkout' ? 'This will remove the checkout.' : 'This will remove the worktree.';
    if (deleteBranch) {
      description += ' The local branch will be deleted.';
    }
    if (deleteBranch && remotePrompt) {
      description += ' You will be asked about deleting the remote branch.';
    }
    return description;
  }, [archiveRepo]);

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

  const handleSelectSession = (session: Session) => {
    setActiveSessionId(session.id);
    setActiveFileViewerId(null); // Deselect file viewer when selecting session
    updateUiState.mutate({
      active_session_id: session.id,
      last_workspace_id: session.workspace_id ?? null,
    });
    if (session.workspace_id) {
      setSelectedWorkspaceId(session.workspace_id);
    }
  };

  const handleSelectWorkspace = (workspace: Workspace) => {
    setSelectedWorkspaceId(workspace.id);

    // If we already have an open session tab for this workspace, focus it.
    const existing = orderedSessions
      .filter((session) => session.workspace_id === workspace.id)
      .reduce<Session | null>(
        (best, session) => (best && best.tab_index > session.tab_index ? best : session),
        null
      );

    if (existing) {
      handleSelectSession(existing);
      return;
    }

    // Otherwise explicitly open/reopen a session for this workspace.
    setActiveFileViewerId(null);
    setActiveSessionId(null);
    openWorkspaceSession.mutate(workspace.id, {
      onSuccess: (session) => {
        handleSelectSession(session);
      },
    });
  };

  const handleOpenFile = useCallback((filePath: string, workspaceId: string) => {
    // Check if file is already open
    const existing = fileViewerTabs.find(
      (tab) => tab.filePath === filePath && tab.workspaceId === workspaceId
    );
    if (existing) {
      setActiveFileViewerId(existing.id);
      // Don't set activeSessionId to null - just let the file viewer take focus
      return;
    }
    // Create new file viewer tab
    const newTab: FileViewerTab = {
      id: `file-${Date.now()}`,
      type: 'file-viewer',
      filePath,
      workspaceId,
    };
    setFileViewerTabs((prev) => [...prev, newTab]);
    setActiveFileViewerId(newTab.id);
    // Don't set activeSessionId to null - keep the underlying session selected
  }, [fileViewerTabs]);

  const handleCloseFileViewer = useCallback((tabId: string) => {
    setFileViewerTabs((prev) => {
      const filtered = prev.filter((tab) => tab.id !== tabId);
      return filtered;
    });

    // Only update activeFileViewerId if we're closing the active tab
    if (tabId === activeFileViewerId) {
      // Find another file viewer tab to switch to
      const remainingTabs = fileViewerTabs.filter((tab) => tab.id !== tabId);
      if (remainingTabs.length > 0) {
        // Switch to the last remaining file viewer tab
        setActiveFileViewerId(remainingTabs[remainingTabs.length - 1].id);
      } else {
        // No more file viewer tabs - just clear the file viewer selection
        // The existing activeSessionId will take over
        setActiveFileViewerId(null);
      }
    }
  }, [fileViewerTabs, activeFileViewerId]);

  const handleSelectFileViewer = useCallback((tabId: string) => {
    setActiveFileViewerId(tabId);
    // Don't set activeSessionId to null - keep the underlying session
  }, []);

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
        setActiveSessionId(null);
        updateUiState.mutate({ active_session_id: null });
      }
    }

    // Remove from tab_order
    const newTabOrder = (resolvedUiState?.tab_order ?? []).filter((id) => id !== sessionId);
    updateUiState.mutate({ tab_order: newTabOrder });

    // Close the session via API
    closeSession.mutate({ id: sessionId, workspaceId: sessionToClose?.workspace_id ?? null });
  };

  const handleArchiveWorkspace = (workspace: Workspace) => {
    setArchiveWorkspaceTarget(workspace);
  };

  const performArchive = (workspace: Workspace, deleteRemote: boolean) => {
    const workspaceId = workspace.id;
    const sessionIdsToRemove = orderedSessions
      .filter((session) => session.workspace_id === workspaceId)
      .map((session) => session.id);
    const remainingSessions = orderedSessions.filter(
      (session) => session.workspace_id !== workspaceId
    );

    archiveWorkspace.mutate({ id: workspaceId, delete_remote: deleteRemote }, {
      onSuccess: () => {
        if (sessionIdsToRemove.length > 0) {
          const newTabOrder = (resolvedUiState?.tab_order ?? []).filter(
            (id) => !sessionIdsToRemove.includes(id)
          );
          updateUiState.mutate({ tab_order: newTabOrder });
        }

        if (activeSessionId && sessionIdsToRemove.includes(activeSessionId)) {
          if (remainingSessions.length > 0) {
            const next = remainingSessions[0];
            setActiveSessionId(next.id);
            updateUiState.mutate({
              active_session_id: next.id,
              last_workspace_id: next.workspace_id ?? null,
            });
            setSelectedWorkspaceId(next.workspace_id ?? null);
          } else {
            setActiveSessionId(null);
            updateUiState.mutate({ active_session_id: null });
          }
        }

        if (selectedWorkspaceId === workspaceId) {
          setSelectedWorkspaceId(null);
          updateUiState.mutate({ last_workspace_id: null });
        }

        setArchiveWorkspaceTarget(null);
        setArchiveRemotePromptTarget(null);
      },
    });
  };

  const handleConfirmArchive = () => {
    if (!archiveWorkspaceTarget) return;
    const repo = resolvedRepositories.find(
      (candidate) => candidate.id === archiveWorkspaceTarget.repository_id
    );
    const deleteBranch = repo?.archive_delete_branch_effective ?? true;
    const remotePrompt = repo?.archive_remote_prompt_effective ?? true;
    const remoteExists = archivePreflight?.remote_branch_exists;

    if (deleteBranch && remotePrompt && remoteExists !== false) {
      setArchiveRemotePromptTarget(archiveWorkspaceTarget);
      setArchiveWorkspaceTarget(null);
      return;
    }

    performArchive(archiveWorkspaceTarget, false);
  };

  const handleRemoteArchiveChoice = (deleteRemote: boolean) => {
    if (!archiveRemotePromptTarget) return;
    const target = archiveRemotePromptTarget;
    setArchiveRemotePromptTarget(null);
    performArchive(target, deleteRemote);
  };

  const handleSelectWorkspaceMode = (mode: WorkspaceMode) => {
    if (!workspaceModeTarget) return;
    const repoId = workspaceModeTarget.id;
    updateRepositorySettings.mutate(
      { id: repoId, data: { workspace_mode: mode } },
      {
        onSuccess: () => {
          const pendingRepoId = pendingWorkspaceRepoId ?? repoId;
          setWorkspaceModeTarget(null);
          setPendingWorkspaceRepoId(null);
          autoCreateWorkspace.mutate(pendingRepoId, {
            onSuccess: (workspace) => {
              setSelectedWorkspaceId(workspace.id);
              updateUiState.mutate({ last_workspace_id: workspace.id });
            },
          });
        },
      }
    );
  };

  const handleRemoveRepository = (repository: Repository) => {
    setRemoveRepositoryTarget(repository);
  };

  const handleConfirmRemoveRepository = () => {
    if (!removeRepositoryTarget) return;
    const repositoryId = removeRepositoryTarget.id;

    // Find affected workspaces/sessions
    const affectedWorkspaceIds = resolvedWorkspaces
      .filter((ws) => ws.repository_id === repositoryId)
      .map((ws) => ws.id);
    const sessionIdsToRemove = orderedSessions
      .filter((s) => s.workspace_id && affectedWorkspaceIds.includes(s.workspace_id))
      .map((s) => s.id);
    const remainingSessions = orderedSessions.filter(
      (s) => !s.workspace_id || !affectedWorkspaceIds.includes(s.workspace_id)
    );

    removeRepository.mutate(repositoryId, {
      onSuccess: () => {
        // Update tab order
        if (sessionIdsToRemove.length > 0) {
          const newTabOrder = (resolvedUiState?.tab_order ?? []).filter(
            (id) => !sessionIdsToRemove.includes(id)
          );
          updateUiState.mutate({ tab_order: newTabOrder });
        }

        // Update active session
        if (activeSessionId && sessionIdsToRemove.includes(activeSessionId)) {
          if (remainingSessions.length > 0) {
            const next = remainingSessions[0];
            setActiveSessionId(next.id);
            updateUiState.mutate({
              active_session_id: next.id,
              last_workspace_id: next.workspace_id ?? null,
            });
            setSelectedWorkspaceId(next.workspace_id ?? null);
          } else {
            setActiveSessionId(null);
            updateUiState.mutate({ active_session_id: null });
          }
        }

        // Update selected workspace
        if (selectedWorkspaceId && affectedWorkspaceIds.includes(selectedWorkspaceId)) {
          setSelectedWorkspaceId(null);
          updateUiState.mutate({ last_workspace_id: null });
        }

        setRemoveRepositoryTarget(null);
      },
    });
  };

  const handleNextTab = () => {
    if (orderedSessions.length === 0) return;
    if (!activeSessionId) {
      handleSelectSession(orderedSessions[0]);
      return;
    }
    const index = orderedSessions.findIndex((session) => session.id === activeSessionId);
    const nextIndex = (index + 1) % orderedSessions.length;
    handleSelectSession(orderedSessions[nextIndex]);
  };

  const handlePrevTab = () => {
    if (orderedSessions.length === 0) return;
    if (!activeSessionId) {
      handleSelectSession(orderedSessions[0]);
      return;
    }
    const index = orderedSessions.findIndex((session) => session.id === activeSessionId);
    const prevIndex = (index - 1 + orderedSessions.length) % orderedSessions.length;
    handleSelectSession(orderedSessions[prevIndex]);
  };

  const handleCopyWorkspacePath = async () => {
    if (!activeWorkspace?.path) return;
    try {
      await navigator.clipboard.writeText(activeWorkspace.path);
    } catch (err) {
      console.error('Failed to copy workspace path', err);
    }
  };

  const handleOpenPr = () => {
    const prStatus = workspaceStatus?.pr_status;
    if (!prStatus) return;
    const repo = activeWorkspace
      ? resolvedRepositories.find((candidate) => candidate.id === activeWorkspace.repository_id)
      : null;
    const fallbackRepo = repo?.repository_url ? parseGitHubRepo(repo.repository_url) : null;
    const url =
      prStatus.url ?? (fallbackRepo ? `https://github.com/${fallbackRepo}/pull/${prStatus.number}` : null);
    if (!url) {
      console.warn('Unable to open PR: missing repository URL');
      return;
    }
    window.open(url, '_blank', 'noopener');
  };

  const handleNewWorkspace = () => {
    if (createWorkspaceRepo) return;
    const repoId =
      selectedWorkspace?.repository_id ?? resolvedRepositories[0]?.id ?? null;
    if (!repoId) return;
    const repo = resolvedRepositories.find((candidate) => candidate.id === repoId);
    if (repo) {
      setCreateWorkspaceRepo(repo);
    }
  };

  const handleNewSession = () => {
    if (resolvedRepositories.length === 0) {
      if (onboardingBaseDir?.base_dir) {
        setIsProjectPickerOpen(true);
      } else {
        setIsBaseDirDialogOpen(true);
      }
      return;
    }

    if (resolvedWorkspaces.length === 0) {
      if (resolvedRepositories.length > 0) {
        setCreateWorkspaceRepo(resolvedRepositories[0]);
      }
      return;
    }

    if (!selectedWorkspaceId) {
      if (resolvedWorkspaces.length > 0) {
        setSelectedWorkspaceId(resolvedWorkspaces[0].id);
      }
      return;
    }

    const existing = orderedSessions
      .filter((session) => session.workspace_id === selectedWorkspaceId)
      .reduce<Session | null>(
        (best, session) => (best && best.tab_index > session.tab_index ? best : session),
        null
      );

    if (existing) {
      handleSelectSession(existing);
      return;
    }

    setActiveFileViewerId(null);
    setActiveSessionId(null);
    openWorkspaceSession.mutate(selectedWorkspaceId, {
      onSuccess: (session) => {
        handleSelectSession(session);
      },
    });
  };

  const handleOpenImport = () => {
    setIsImportDialogOpen(true);
  };

  const handleAddProject = () => {
    setIsAddProjectOpen(true);
  };

  const handleBrowseProjects = () => {
    if (onboardingBaseDir?.base_dir) {
      setIsProjectPickerOpen(true);
    } else {
      setIsBaseDirDialogOpen(true);
    }
  };

  const handleOnboardingAdded = () => {
    setIsBaseDirDialogOpen(false);
    setIsProjectPickerOpen(false);
    setIsAddProjectOpen(false);
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

  const { handleForkSession, handleCreatePr } = useWorkspaceActions({
    session: activeSession,
    workspace: activeWorkspace ?? null,
    onForkedSession: (session) => handleImportedSession(session),
  });

  const canTogglePlanMode =
    activeSession?.agent_type === 'claude' &&
    !activeSession?.agent_session_id;

  const handleTogglePlanMode = () => {
    if (!activeSession || !canTogglePlanMode) return;
    const nextMode = activeSession.agent_mode === 'plan' ? 'build' : 'plan';
    updateSession.mutate({ id: activeSession.id, data: { agent_mode: nextMode } });
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
        id: 'new-workspace',
        label: 'New Workspace...',
        keywords: 'workspace create',
        disabled: resolvedRepositories.length === 0,
        onSelect: handleNewWorkspace,
      },
      {
        id: 'import-session',
        label: 'Import Session',
        shortcut: 'Ctrl+I',
        onSelect: handleOpenImport,
      },
      {
        id: 'open-pr',
        label: 'Open PR',
        disabled: !workspaceStatus?.pr_status,
        onSelect: handleOpenPr,
      },
      {
        id: 'create-pr',
        label: 'Create PR...',
        disabled: !activeSession || !activeWorkspace,
        onSelect: handleCreatePr,
      },
      {
        id: 'fork-session',
        label: 'Fork Session...',
        disabled: !activeSession,
        onSelect: handleForkSession,
      },
      {
        id: 'toggle-plan-mode',
        label: activeSession?.agent_mode === 'plan' ? 'Switch to Build Mode' : 'Switch to Plan Mode',
        shortcut: 'Ctrl+Shift+P',
        keywords: 'plan build mode toggle',
        disabled: !canTogglePlanMode,
        onSelect: handleTogglePlanMode,
      },
      {
        id: 'new-session',
        label: 'Start New Session',
        onSelect: handleNewSession,
      },
      {
        id: 'archive-workspace',
        label: 'Archive Workspace...',
        disabled: !selectedWorkspace,
        onSelect: () => selectedWorkspace && handleArchiveWorkspace(selectedWorkspace),
      },
      {
        id: 'remove-project',
        label: 'Remove Project...',
        keywords: 'delete repository',
        disabled: !selectedWorkspace,
        onSelect: () => {
          if (selectedWorkspace) {
            const repo = resolvedRepositories.find((r) => r.id === selectedWorkspace.repository_id);
            if (repo) handleRemoveRepository(repo);
          }
        },
      },
      {
        id: 'copy-workspace-path',
        label: 'Copy Workspace Path',
        disabled: !activeWorkspace?.path,
        onSelect: handleCopyWorkspacePath,
      },
      {
        id: 'add-project',
        label: 'Add Project...',
        onSelect: () => setIsAddProjectOpen(true),
      },
      {
        id: 'browse-projects',
        label: 'Browse Projects...',
        shortcut: 'Ctrl+N',
        onSelect: handleBrowseProjects,
      },
      {
        id: 'set-projects-dir',
        label: 'Set Projects Directory',
        onSelect: () => setIsBaseDirDialogOpen(true),
      },
      {
        id: 'close-tab',
        label: 'Close Tab',
        disabled: !activeSessionId,
        onSelect: () => activeSessionId && handleCloseSession(activeSessionId),
      },
      {
        id: 'next-tab',
        label: 'Next Tab',
        disabled: orderedSessions.length < 2,
        onSelect: handleNextTab,
      },
      {
        id: 'prev-tab',
        label: 'Previous Tab',
        disabled: orderedSessions.length < 2,
        onSelect: handlePrevTab,
      },
    ],
    [
      activeSession,
      activeSessionId,
      activeWorkspace,
      canTogglePlanMode,
      handleArchiveWorkspace,
      handleBrowseProjects,
      handleCreatePr,
      handleCloseSession,
      handleCopyWorkspacePath,
      handleForkSession,
      handleNewSession,
      handleNewWorkspace,
      handleNextTab,
      handleOpenImport,
      handleOpenPr,
      handlePrevTab,
      handleRemoveRepository,
      handleTogglePlanMode,
      handleToggleSidebar,
      isSidebarOpen,
      orderedSessions.length,
      resolvedRepositories,
      selectedWorkspace,
      workspaceStatus?.pr_status,
    ]
  );

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && (key === 'k' || key === 'p')) {
        event.preventDefault();
        setIsCommandPaletteOpen((prev) => !prev);
      }
      if ((event.metaKey || event.ctrlKey) && key === 'n') {
        event.preventDefault();
        handleBrowseProjects();
      }
      // Ctrl+1..9 to switch tabs (works in browsers on all platforms)
      // On Mac, Cmd+1..9 is taken by browser for tab switching, so we use Ctrl
      // On Windows/Linux, Ctrl+1..9 is taken by browser, so we use Alt
      const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);
      const useTabShortcut = isMacPlatform
        ? (event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey)
        : (event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey);

      if (useTabShortcut) {
        const num = parseInt(event.key, 10);
        if (num >= 1 && num <= 9) {
          event.preventDefault();
          const targetIndex = num - 1;
          if (targetIndex < orderedSessions.length) {
            handleSelectSession(orderedSessions[targetIndex]);
          }
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onboardingBaseDir?.base_dir, orderedSessions, handleSelectSession]);

  const showOnboarding = resolvedRepositories.length === 0;

  // Get the active file viewer tab if any
  const activeFileViewerTab = activeFileViewerId
    ? fileViewerTabs.find((tab) => tab.id === activeFileViewerId) ?? null
    : null;

  // Context value for file viewer
  const fileViewerContextValue = useMemo(
    () => ({
      openFile: handleOpenFile,
      currentWorkspaceId: activeSession?.workspace_id ?? selectedWorkspaceId,
    }),
    [handleOpenFile, activeSession?.workspace_id, selectedWorkspaceId]
  );

  return (
    <FileViewerContext.Provider value={fileViewerContextValue}>
      <Layout
        selectedWorkspaceId={selectedWorkspaceId}
        onSelectWorkspace={handleSelectWorkspace}
        onCreateWorkspace={(repository) => setCreateWorkspaceRepo(repository)}
        onArchiveWorkspace={handleArchiveWorkspace}
        onRemoveRepository={handleRemoveRepository}
        onAddProject={handleAddProject}
        onBrowseProjects={handleBrowseProjects}
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
        fileViewerTabs={fileViewerTabs}
        activeFileViewerId={activeFileViewerId}
        onSelectFileViewer={handleSelectFileViewer}
        onCloseFileViewer={handleCloseFileViewer}
      >
        {activeFileViewerTab ? (
          <FileViewer
            filePath={activeFileViewerTab.filePath}
            workspaceId={activeFileViewerTab.workspaceId}
            onClose={() => handleCloseFileViewer(activeFileViewerTab.id)}
          />
        ) : showOnboarding ? (
          <OnboardingEmptyState
            onAddProject={() => setIsAddProjectOpen(true)}
            onSetBaseDir={() => setIsBaseDirDialogOpen(true)}
            onImportSession={handleOpenImport}
          />
        ) : (
          <ChatView
            session={activeSession}
            onNewSession={handleNewSession}
            isLoadingSession={isLoadingSession}
            onForkedSession={handleImportedSession}
          />
        )}
      </Layout>
      <SessionImportDialog
        isOpen={isImportDialogOpen}
        onClose={() => setIsImportDialogOpen(false)}
        onImported={handleImportedSession}
      />
      {createWorkspaceRepo && (
        <CreateWorkspaceDialog
          repositoryId={createWorkspaceRepo.id}
          repositoryName={createWorkspaceRepo.name}
          isOpen={!!createWorkspaceRepo}
          onClose={() => setCreateWorkspaceRepo(null)}
          onModeRequired={() => {
            setPendingWorkspaceRepoId(createWorkspaceRepo.id);
            setWorkspaceModeTarget(createWorkspaceRepo);
            setCreateWorkspaceRepo(null);
          }}
          onSuccess={(workspace) => {
            setCreateWorkspaceRepo(null);
            setSelectedWorkspaceId(workspace.id);
            updateUiState.mutate({ last_workspace_id: workspace.id });
          }}
        />
      )}
      <BaseDirDialog
        isOpen={isBaseDirDialogOpen}
        onClose={() => setIsBaseDirDialogOpen(false)}
        onSaved={() => {
          setIsBaseDirDialogOpen(false);
          setIsProjectPickerOpen(true);
        }}
      />
      <ProjectPickerDialog
        isOpen={isProjectPickerOpen}
        onClose={() => setIsProjectPickerOpen(false)}
        onAdded={handleOnboardingAdded}
      />
      <AddProjectDialog
        isOpen={isAddProjectOpen}
        onClose={() => setIsAddProjectOpen(false)}
        onAdded={handleOnboardingAdded}
      />
      <CommandPalette
        isOpen={isCommandPaletteOpen}
        onClose={() => setIsCommandPaletteOpen(false)}
        commands={commands}
      />
      <ConfirmDialog
        isOpen={!!archiveWorkspaceTarget}
        onClose={() => setArchiveWorkspaceTarget(null)}
        title={`Archive "${archiveWorkspaceTarget?.name ?? ''}"?`}
        description={archiveDescription}
        confirmLabel="Archive"
        onConfirm={handleConfirmArchive}
        warnings={archivePreflight?.warnings}
        error={archivePreflight?.error}
        isPending={archiveWorkspace.isPending}
        confirmVariant={
          archivePreflight?.severity === 'danger'
            ? 'danger'
            : archivePreflight?.severity === 'warning'
              ? 'warning'
              : 'info'
        }
      />
      <ConfirmDialog
        isOpen={!!archiveRemotePromptTarget}
        onClose={() => setArchiveRemotePromptTarget(null)}
        title={`Delete remote branch for "${archiveRemotePromptTarget?.name ?? ''}"?`}
        description={`Delete branch "${archiveRemotePromptTarget?.branch ?? ''}" from the remote repository?`}
        confirmLabel="Delete Remote"
        cancelLabel="Keep Remote"
        onConfirm={() => handleRemoteArchiveChoice(true)}
        onCancel={() => handleRemoteArchiveChoice(false)}
        isPending={archiveWorkspace.isPending}
        confirmVariant="warning"
      />
      <ConfirmDialog
        isOpen={!!removeRepositoryTarget}
        onClose={() => setRemoveRepositoryTarget(null)}
        title={`Remove "${removeRepositoryTarget?.name ?? ''}"?`}
        description="This will archive all workspaces and remove the project."
        confirmLabel="Remove"
        onConfirm={handleConfirmRemoveRepository}
        warnings={removeRepositoryPreflight?.warnings}
        isPending={removeRepository.isPending}
        confirmVariant={
          removeRepositoryPreflight?.severity === 'danger'
            ? 'danger'
            : removeRepositoryPreflight?.severity === 'warning'
              ? 'warning'
              : 'info'
        }
      />
      <ConfirmDialog
        isOpen={!!workspaceModeTarget}
        onClose={() => {
          setWorkspaceModeTarget(null);
          setPendingWorkspaceRepoId(null);
        }}
        title={`Select workspace mode for "${workspaceModeTarget?.name ?? ''}"`}
        description="Worktrees are lightweight and share git metadata. Checkouts create full clones for complete isolation."
        confirmLabel="Use Worktrees"
        cancelLabel="Use Checkouts"
        onConfirm={() => handleSelectWorkspaceMode('worktree')}
        onCancel={() => handleSelectWorkspaceMode('checkout')}
        isPending={updateRepositorySettings.isPending || autoCreateWorkspace.isPending}
        confirmVariant="info"
      />
    </FileViewerContext.Provider>
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
