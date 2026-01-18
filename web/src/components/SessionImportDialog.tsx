import { useEffect, useMemo, useRef, useState } from 'react';
import { Loader2, Search, X } from 'lucide-react';
import { useExternalSessions, useImportExternalSession } from '../hooks';
import type { ExternalSession, Session } from '../types';
import { cn } from '../lib/cn';

type AgentFilter = 'all' | 'claude' | 'codex' | 'gemini';

interface SessionImportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onImported: (session: Session) => void;
}

const FILTER_LABELS: Record<AgentFilter, string> = {
  all: 'All',
  claude: 'Claude',
  codex: 'Codex',
  gemini: 'Gemini',
};

export function SessionImportDialog({ isOpen, onClose, onImported }: SessionImportDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [filter, setFilter] = useState<AgentFilter>('all');
  const [query, setQuery] = useState('');

  const { data: sessions = [], isLoading } = useExternalSessions(
    filter === 'all' ? null : filter,
    { enabled: isOpen }
  );
  const importMutation = useImportExternalSession();

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    if (isOpen) {
      dialog.showModal();
    } else {
      dialog.close();
      setQuery('');
      setFilter('all');
    }
  }, [isOpen]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const handleCancel = (e: Event) => {
      e.preventDefault();
      if (!importMutation.isPending) {
        onClose();
      }
    };

    dialog.addEventListener('cancel', handleCancel);
    return () => dialog.removeEventListener('cancel', handleCancel);
  }, [onClose, importMutation.isPending]);

  const handleBackdropClick = (e: React.MouseEvent<HTMLDialogElement>) => {
    if (e.target === dialogRef.current && !importMutation.isPending) {
      onClose();
    }
  };

  const filtered = useMemo(() => {
    if (!query.trim()) return sessions;
    const q = query.trim().toLowerCase();
    return sessions.filter((session) => {
      return (
        session.display.toLowerCase().includes(q) ||
        (session.project_name ?? '').toLowerCase().includes(q) ||
        (session.project ?? '').toLowerCase().includes(q)
      );
    });
  }, [sessions, query]);

  const handleImport = (session: ExternalSession) => {
    importMutation.mutate(session.id, {
      onSuccess: (response) => {
        onImported(response.session);
        onClose();
      },
    });
  };

  return (
    <dialog
      ref={dialogRef}
      onClick={handleBackdropClick}
      className="m-auto w-[720px] max-w-[90vw] rounded-xl border border-border bg-surface p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex flex-col">
        <div className="flex items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold text-text">Import Session</h2>
          <button
            onClick={onClose}
            disabled={importMutation.isPending}
            className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="px-6 py-4">
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search sessions..."
                className="w-full rounded-lg border border-border bg-surface-elevated py-2 pl-9 pr-3 text-sm text-text placeholder-text-muted focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              />
            </div>
            <div className="flex items-center gap-1 rounded-lg bg-surface-elevated p-1">
              {(['all', 'claude', 'codex', 'gemini'] as AgentFilter[]).map((mode) => (
                <button
                  key={mode}
                  onClick={() => setFilter(mode)}
                  className={cn(
                    'rounded-md px-2.5 py-1 text-xs font-medium transition-colors',
                    filter === mode
                      ? 'bg-accent text-white'
                      : 'text-text-muted hover:bg-surface hover:text-text'
                  )}
                >
                  {FILTER_LABELS[mode]}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="max-h-[420px] overflow-y-auto px-6 pb-6">
          {isLoading ? (
            <div className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading sessions...
            </div>
          ) : filtered.length === 0 ? (
            <div className="text-sm text-text-muted">No sessions found.</div>
          ) : (
            <div className="space-y-2">
              {filtered.map((session) => (
                <div
                  key={session.id}
                  className="flex items-center justify-between rounded-lg border border-border/60 bg-surface-elevated px-4 py-3"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm text-text">{session.display}</div>
                    <div className="mt-1 flex items-center gap-2 text-xs text-text-muted">
                      <span className="capitalize">{session.agent_type}</span>
                      <span>·</span>
                      <span>{session.relative_time}</span>
                      {session.project_name && (
                        <>
                          <span>·</span>
                          <span className="truncate">{session.project_name}</span>
                        </>
                      )}
                    </div>
                  </div>
                  <button
                    onClick={() => handleImport(session)}
                    disabled={importMutation.isPending}
                    className={cn(
                      'rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hover',
                      importMutation.isPending && 'cursor-wait opacity-70'
                    )}
                  >
                    Import
                  </button>
                </div>
              ))}
            </div>
          )}

          {importMutation.error && (
            <div className="mt-4 rounded-lg bg-red-500/10 px-3 py-2.5 text-sm text-red-400">
              {importMutation.error instanceof Error
                ? importMutation.error.message
                : 'Failed to import session'}
            </div>
          )}
        </div>
      </div>
    </dialog>
  );
}
