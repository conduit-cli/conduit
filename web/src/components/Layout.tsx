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
  workspaces: Workspace[];
  activeWorkspace?: Workspace | null;
  workspaceStatus?: WorkspaceStatus | null;
  latestUsage?: { input_tokens: number; output_tokens: number } | null;
  isSidebarOpen: boolean;
  onToggleSidebar: () => void;
}

export function Layout({
  children,
  selectedWorkspaceId,
  onSelectWorkspace,
  sessions,
  activeSessionId,
  onSelectSession,
  onReorderSessions,
  workspaces,
  activeWorkspace,
  workspaceStatus,
  latestUsage,
  isSidebarOpen,
  onToggleSidebar,
}: LayoutProps) {
  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? null;

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
        />
        <SessionTabs
          sessions={sessions}
          activeSessionId={activeSessionId}
          workspaces={workspaces}
          onSelectSession={onSelectSession}
          onReorderSessions={onReorderSessions}
        />
        <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      </div>
    </div>
  );
}
