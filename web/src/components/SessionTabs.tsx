import { useEffect } from 'react';
import { X, Loader2 } from 'lucide-react';
import { cn } from '../lib/cn';
import { useProcessingSessions } from '../hooks';
import type { Session, Workspace } from '../types';

interface SessionTabsProps {
  sessions: Session[];
  activeSessionId: string | null;
  workspaces: Workspace[];
  onSelectSession: (session: Session) => void;
  onReorderSessions: (sessionIds: string[]) => void;
  onCloseSession: (sessionId: string) => void;
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
  onCloseSession,
}: SessionTabsProps) {
  const processingSessionIds = useProcessingSessions();

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

      if (event.key === 'w' || event.key === 'W') {
        event.preventDefault();
        const activeSession = sessions[activeIndex];
        if (!processingSessionIds.has(activeSession.id)) {
          onCloseSession(activeSession.id);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [sessions, activeSessionId, onSelectSession, onReorderSessions, onCloseSession, processingSessionIds]);

  if (sessions.length === 0) {
    return null;
  }

  return (
    <div className="flex items-center gap-2 overflow-x-auto border-b border-border bg-surface px-4 py-2">
      {sessions.map((session) => {
        const label = sessionLabel(session, workspaces);
        const isActive = session.id === activeSessionId;
        const isProcessing = processingSessionIds.has(session.id);
        return (
          <button
            key={session.id}
            onClick={() => onSelectSession(session)}
            aria-selected={isActive}
            className={cn(
              'group flex shrink-0 items-center gap-2 rounded-full px-3 py-1 text-xs transition-colors',
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
            {isProcessing ? (
              <span className="ml-1 p-0.5" aria-label="Processing">
                <Loader2 className="h-3 w-3 animate-spin text-text-muted" />
              </span>
            ) : (
              <span
                role="button"
                tabIndex={0}
                onClick={(e) => {
                  e.stopPropagation();
                  onCloseSession(session.id);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.stopPropagation();
                    e.preventDefault();
                    onCloseSession(session.id);
                  }
                }}
                className={cn(
                  'ml-1 rounded-full p-0.5 transition-colors',
                  'opacity-0 group-hover:opacity-100',
                  'hover:bg-surface hover:text-text',
                  isActive && 'opacity-100'
                )}
                aria-label={`Close ${label}`}
              >
                <X className="h-3 w-3" />
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
