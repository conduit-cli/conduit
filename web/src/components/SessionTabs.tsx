import { useEffect } from 'react';
import { X, FileText } from 'lucide-react';
import { cn } from '../lib/cn';
import { useProcessingSessions } from '../hooks';
import type { Session, Workspace, FileViewerTab } from '../types';

interface SessionTabsProps {
  sessions: Session[];
  activeSessionId: string | null;
  workspaces: Workspace[];
  onSelectSession: (session: Session) => void;
  onReorderSessions: (sessionIds: string[]) => void;
  onCloseSession: (sessionId: string) => void;
  // File viewer tabs
  fileViewerTabs?: FileViewerTab[];
  activeFileViewerId?: string | null;
  onSelectFileViewer?: (tabId: string) => void;
  onCloseFileViewer?: (tabId: string) => void;
}

function sessionLabel(session: Session, workspaces: Workspace[]): string {
  const workspace = session.workspace_id
    ? workspaces.find((w) => w.id === session.workspace_id)
    : undefined;

  if (workspace && session.title) {
    return `${workspace.name} · ${session.title}`;
  }

  if (session.title) return session.title;
  if (workspace) return workspace.name;
  return `Session ${session.tab_index + 1}`;
}

// Platform detection for keyboard shortcut display
// On Mac we use Ctrl+N (since Cmd+N is taken by browser), on Windows/Linux we use Alt+N
const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
const tabShortcutPrefix = isMac ? '⌃' : 'Alt+';

function getFileName(path: string): string {
  const parts = path.split('/');
  return parts[parts.length - 1] || path;
}

export function SessionTabs({
  sessions,
  activeSessionId,
  workspaces,
  onSelectSession,
  onReorderSessions,
  onCloseSession,
  fileViewerTabs = [],
  activeFileViewerId,
  onSelectFileViewer,
  onCloseFileViewer,
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

  if (sessions.length === 0 && fileViewerTabs.length === 0) {
    return null;
  }

  return (
    <div className="flex items-center gap-2 overflow-x-auto border-b border-border bg-surface px-4 py-2">
      {/* Session tabs */}
      {sessions.map((session, index) => {
        const label = sessionLabel(session, workspaces);
        const isActive = session.id === activeSessionId && !activeFileViewerId;
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
            {index < 9 && (
              <span className="ml-0.5 text-[10px] text-text-muted/50 tabular-nums">
                {tabShortcutPrefix}{index + 1}
              </span>
            )}
            <span
              role="button"
              tabIndex={isProcessing ? -1 : 0}
              aria-disabled={isProcessing}
              onClick={(e) => {
                e.stopPropagation();
                if (isProcessing) return;
                onCloseSession(session.id);
              }}
              onKeyDown={(e) => {
                if (isProcessing) return;
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
                isActive && 'opacity-100',
                isProcessing && 'cursor-not-allowed opacity-30'
              )}
              aria-label={`Close ${label}`}
            >
              <X className="h-3 w-3" />
            </span>
          </button>
        );
      })}

      {/* File viewer tabs */}
      {fileViewerTabs.map((tab) => {
        const fileName = getFileName(tab.filePath);
        const isActive = tab.id === activeFileViewerId;
        return (
          <button
            key={tab.id}
            onClick={() => onSelectFileViewer?.(tab.id)}
            aria-selected={isActive}
            className={cn(
              'group flex shrink-0 items-center gap-2 rounded-full px-3 py-1 text-xs transition-colors',
              isActive
                ? 'bg-accent/20 text-text'
                : 'text-text-muted hover:bg-surface-elevated hover:text-text'
            )}
          >
            <FileText className="h-3.5 w-3.5 text-blue-400" />
            <span className="max-w-36 truncate" title={tab.filePath}>
              {fileName}
            </span>
            <span
              role="button"
              tabIndex={0}
              onClick={(e) => {
                e.stopPropagation();
                onCloseFileViewer?.(tab.id);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.stopPropagation();
                  e.preventDefault();
                  onCloseFileViewer?.(tab.id);
                }
              }}
              className={cn(
                'ml-1 rounded-full p-0.5 transition-colors',
                'opacity-0 group-hover:opacity-100',
                'hover:bg-surface hover:text-text',
                isActive && 'opacity-100'
              )}
              aria-label={`Close ${fileName}`}
            >
              <X className="h-3 w-3" />
            </span>
          </button>
        );
      })}
    </div>
  );
}
