import type { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import { SessionTabs } from './SessionTabs';
import type { Session, Workspace, WorkspaceStatus } from '../types';

interface LayoutProps {
  children: ReactNode;
  selectedWorkspaceId?: string | null;
  onSelectWorkspace?: (workspace: Workspace) => void;
  sessions: Session[];
  activeSessionId: string | null;
  onSelectSession: (session: Session) => void;
  onReorderSessions: (sessionIds: string[]) => void;
  onCloseSession: (sessionId: string) => void;
  workspaces: Workspace[];
  activeWorkspace?: Workspace | null;
  workspaceStatus?: WorkspaceStatus | null;
  latestUsage?: { input_tokens: number; output_tokens: number } | null;
  isSidebarOpen: boolean;
  onToggleSidebar: () => void;
  isBootstrapping?: boolean;
  onImportSession?: () => void;
}

export function Layout({
  children,
  selectedWorkspaceId,
  onSelectWorkspace,
  sessions,
  activeSessionId,
  onSelectSession,
  onReorderSessions,
  onCloseSession,
  workspaces,
  activeWorkspace,
  workspaceStatus,
  latestUsage,
  isSidebarOpen,
  onToggleSidebar,
  isBootstrapping = false,
  onImportSession,
}: LayoutProps) {
  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? null;

  if (isBootstrapping) {
    return (
      <div className="flex h-dvh animate-pulse bg-background text-text">
        <div className="flex w-72 flex-col border-r border-border bg-surface">
          <div className="flex items-center gap-3 border-b border-border px-4 py-4">
            <div className="h-8 w-8 rounded-lg bg-surface-elevated" />
            <div className="h-4 w-24 rounded bg-surface-elevated" />
          </div>
          <div className="flex-1 space-y-3 overflow-y-auto p-3">
            <div className="h-4 w-20 rounded bg-surface-elevated" />
            <div className="space-y-2">
              {Array.from({ length: 6 }).map((_, index) => (
                <div key={`sidebar-skeleton-${index}`} className="h-10 rounded bg-surface-elevated" />
              ))}
            </div>
          </div>
          <div className="border-t border-border p-3">
            <div className="h-5 w-32 rounded bg-surface-elevated" />
          </div>
        </div>
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="border-b border-border px-6 py-4">
            <div className="h-4 w-64 rounded bg-surface-elevated" />
          </div>
          <div className="border-b border-border px-6 py-3">
            <div className="flex gap-2">
              {Array.from({ length: 4 }).map((_, index) => (
                <div key={`tab-skeleton-${index}`} className="h-7 w-24 rounded-full bg-surface-elevated" />
              ))}
            </div>
          </div>
          <main className="min-h-0 flex-1 overflow-hidden">
            <div className="flex h-full flex-col justify-between p-6">
              <div className="space-y-4">
                {Array.from({ length: 3 }).map((_, index) => (
                  <div key={`chat-skeleton-${index}`} className="h-14 rounded-lg bg-surface-elevated" />
                ))}
              </div>
              <div className="h-12 rounded-lg bg-surface-elevated" />
            </div>
          </main>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-dvh bg-background text-text">
      {isSidebarOpen && (
        <Sidebar
          selectedWorkspaceId={selectedWorkspaceId}
          onSelectWorkspace={onSelectWorkspace}
        />
      )}
      <div className="flex flex-1 flex-col overflow-hidden">
        <Header
          activeSession={activeSession}
          activeWorkspace={activeWorkspace}
          workspaceStatus={workspaceStatus}
          latestUsage={latestUsage}
          isSidebarOpen={isSidebarOpen}
          onToggleSidebar={onToggleSidebar}
          onImportSession={onImportSession}
        />
        <SessionTabs
          sessions={sessions}
          activeSessionId={activeSessionId}
          workspaces={workspaces}
          onSelectSession={onSelectSession}
          onReorderSessions={onReorderSessions}
          onCloseSession={onCloseSession}
        />
        <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      </div>
    </div>
  );
}
