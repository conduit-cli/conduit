import { useEffect } from 'react';
import { cn } from '../lib/cn';
import type { Session, Workspace } from '../types';

interface SessionTabsProps {
  sessions: Session[];
  activeSessionId: string | null;
  workspaces: Workspace[];
  onSelectSession: (session: Session) => void;
  onReorderSessions: (sessionIds: string[]) => void;
}

function sessionLabel(session: Session, workspaces: Workspace[]): string {
  if (session.title) return session.title;
  const workspace = workspaces.find((w) => w.id === session.workspace_id);
  if (workspace) return workspace.name;
  return `Session ${session.tab_index + 1}`;
}

export function SessionTabs({
  sessions,
  activeSessionId,
  workspaces,
  onSelectSession,
  onReorderSessions,
}: SessionTabsProps) {
  useEffect(() => {
    if (sessions.length === 0) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.altKey) return;

      const activeIndex = sessions.findIndex((session) => session.id === activeSessionId);
      if (activeIndex === -1) return;

      if (event.shiftKey && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        event.preventDefault();
        const direction = event.key === 'ArrowLeft' ? -1 : 1;
        const targetIndex = activeIndex + direction;
        if (targetIndex < 0 || targetIndex >= sessions.length) return;

        const nextOrder = sessions.map((session) => session.id);
        [nextOrder[activeIndex], nextOrder[targetIndex]] = [
          nextOrder[targetIndex],
          nextOrder[activeIndex],
        ];
        onReorderSessions(nextOrder);
        return;
      }

      if (!event.shiftKey && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        event.preventDefault();
        const direction = event.key === 'ArrowLeft' ? -1 : 1;
        const nextIndex = (activeIndex + direction + sessions.length) % sessions.length;
        onSelectSession(sessions[nextIndex]);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [sessions, activeSessionId, onSelectSession, onReorderSessions]);

  if (sessions.length === 0) {
    return null;
  }

  return (
    <div className="flex items-center gap-2 overflow-x-auto border-b border-border bg-surface px-4 py-2">
      {sessions.map((session) => {
        const label = sessionLabel(session, workspaces);
        const isActive = session.id === activeSessionId;
        return (
          <button
            key={session.id}
            onClick={() => onSelectSession(session)}
            aria-selected={isActive}
            className={cn(
              'flex shrink-0 items-center gap-2 rounded-full px-3 py-1 text-xs transition-colors',
              isActive
                ? 'bg-accent/20 text-text'
                : 'text-text-muted hover:bg-surface-elevated hover:text-text'
            )}
          >
            <span
              className={cn(
                'h-2 w-2 rounded-full',
                session.agent_type === 'claude'
                  ? 'bg-orange-400'
                  : session.agent_type === 'codex'
                  ? 'bg-green-400'
                  : 'bg-blue-400'
              )}
            />
            <span className="max-w-36 truncate">{label}</span>
          </button>
        );
      })}
    </div>
  );
}
