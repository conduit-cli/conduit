import { useHealth } from '../hooks';
import { Circle, Settings, PanelLeft, GitBranch, GitPullRequest, Activity, Download } from 'lucide-react';
import { cn } from '../lib/cn';
import { ThemeSwitcher } from './ThemeSwitcher';
import type { Session, Workspace, WorkspaceStatus } from '../types';

interface HeaderProps {
  activeSession?: Session | null;
  activeWorkspace?: Workspace | null;
  workspaceStatus?: WorkspaceStatus | null;
  latestUsage?: { input_tokens: number; output_tokens: number } | null;
  isSidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  onImportSession?: () => void;
}

export function Header({
  activeSession,
  activeWorkspace,
  workspaceStatus,
  latestUsage,
  isSidebarOpen = true,
  onToggleSidebar,
  onImportSession,
}: HeaderProps) {
  const { data: health, isLoading, isError } = useHealth();

  const statusColor = isLoading
    ? 'text-yellow-400'
    : isError
    ? 'text-red-400'
    : health?.status === 'ok'
    ? 'text-green-400'
    : 'text-red-400';

  const gitStats = workspaceStatus?.git_stats;
  const prStatus = workspaceStatus?.pr_status;

  return (
    <header className="flex h-14 items-center justify-between border-b border-border bg-surface px-6">
      <div className="flex items-center gap-3">
        <button
          aria-label={isSidebarOpen ? 'Hide sidebar' : 'Show sidebar'}
          onClick={onToggleSidebar}
          className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
        >
          <PanelLeft className="h-4 w-4" />
        </button>
        <h1 className="text-sm font-medium text-text-muted">Dashboard</h1>
        {activeSession && (
          <div className="flex items-center gap-2 rounded-full bg-surface-elevated px-2.5 py-1 text-xs text-text">
            <span
              className={cn(
                'h-2 w-2 rounded-full',
                activeSession.agent_type === 'claude'
                  ? 'bg-orange-400'
                  : activeSession.agent_type === 'codex'
                  ? 'bg-green-400'
                  : 'bg-blue-400'
              )}
            />
            <span className="capitalize">
              {activeSession.agent_type === 'claude'
                ? 'Claude'
                : activeSession.agent_type === 'codex'
                ? 'Codex'
                : 'Gemini'}
            </span>
            {activeSession.agent_mode && <span className="text-text-muted">· {activeSession.agent_mode}</span>}
            {activeSession.model_display_name && (
              <span className="text-text-muted">· {activeSession.model_display_name}</span>
            )}
          </div>
        )}
      </div>

      <div className="flex items-center gap-4">
        <div className="flex items-center gap-3 text-xs text-text-muted">
          {latestUsage && (
            <div className="flex items-center gap-1">
              <Activity className="h-3.5 w-3.5" />
              <span>
                {latestUsage.input_tokens} in / {latestUsage.output_tokens} out
              </span>
            </div>
          )}
          {activeWorkspace?.branch && (
            <div className="flex items-center gap-1">
              <GitBranch className="h-3.5 w-3.5" />
              <span className="max-w-40 truncate">{activeWorkspace.branch}</span>
            </div>
          )}
          {gitStats && (gitStats.additions > 0 || gitStats.deletions > 0) && (
            <span className="tabular-nums text-xs text-text-muted">
              <span className="text-green-400">+{gitStats.additions}</span>
              <span className="mx-1 text-text-muted">/</span>
              <span className="text-red-400">-{gitStats.deletions}</span>
            </span>
          )}
          {prStatus && (
            <div className="flex items-center gap-1">
              <GitPullRequest className="h-3.5 w-3.5" />
              <span>#{prStatus.number}</span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2 text-xs text-text-muted">
            <Circle className={cn('h-2 w-2 fill-current', statusColor)} />
            <span>
              {isLoading
                ? 'Connecting...'
                : isError
                ? 'Disconnected'
                : `v${health?.version}`}
            </span>
          </div>
          <ThemeSwitcher />
          {onImportSession && (
            <button
              aria-label="Import session"
              onClick={onImportSession}
              className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
            >
              <Download className="h-4 w-4" />
            </button>
          )}
          <button
            aria-label="Settings"
            className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          >
            <Settings className="h-4 w-4" />
          </button>
        </div>
      </div>
    </header>
  );
}
