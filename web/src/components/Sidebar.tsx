import { useEffect, useState } from 'react';
import { useRepositories, useWorkspaces, useAgents, useWorkspaceStatus } from '../hooks';
import {
  FolderGit2,
  Plus,
  ChevronDown,
  ChevronRight,
  GitBranch,
  GitPullRequest,
  MoreHorizontal,
  Archive,
} from 'lucide-react';
import { cn } from '../lib/cn';
import type { Repository, Workspace } from '../types';
import { CreateWorkspaceDialog } from './CreateWorkspaceDialog';
import { Logo } from './Logo';

interface WorkspaceItemProps {
  repository: Repository;
  workspace: Workspace;
  isSelected?: boolean;
  onSelect?: () => void;
  onArchive?: () => void;
}

function parseGitHubRepo(repoUrl: string | null | undefined): string | null {
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

function WorkspaceItem({
  repository,
  workspace,
  isSelected,
  onSelect,
  onArchive,
}: WorkspaceItemProps) {
  const [hasInitialStatus, setHasInitialStatus] = useState(false);
  const shouldPoll = !!isSelected || !hasInitialStatus;
  const { data: status } = useWorkspaceStatus(workspace.id, {
    enabled: true,
    refetchInterval: shouldPoll ? 5000 : false,
    staleTime: shouldPoll ? 2000 : Number.POSITIVE_INFINITY,
  });

  useEffect(() => {
    if (!hasInitialStatus && status?.updated_at) {
      setHasInitialStatus(true);
    }
  }, [hasInitialStatus, status?.updated_at]);

  // Extract branch display name (show last part after / with ellipsis prefix)
  const branchDisplay = workspace.branch.includes('/')
    ? '…/' + workspace.branch.split('/').pop()
    : workspace.branch;

  const gitStats = status?.git_stats;
  const prStatus = status?.pr_status;
  const repoSlug = parseGitHubRepo(repository.repository_url);
  const prUrl = prStatus
    ? prStatus.url ?? (repoSlug ? `https://github.com/${repoSlug}/pull/${prStatus.number}` : null)
    : null;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect?.();
        }
      }}
      className={cn(
        'group flex w-full flex-col gap-0.5 rounded-md px-3 py-2 text-left transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60',
        isSelected
          ? 'bg-accent/10 text-text'
          : 'text-text-muted hover:bg-surface-elevated hover:text-text'
      )}
    >
      {/* Branch name first */}
      <div className="flex items-center gap-2">
        <GitBranch className="h-3.5 w-3.5 shrink-0 text-text-muted" />
        <span className="truncate text-sm text-text-muted">{branchDisplay}</span>
      </div>

      {/* Workspace name + status indicators second */}
      <div className="ml-5 flex items-center justify-between gap-2">
        <span className="truncate text-xs font-medium">{workspace.name}</span>

        <div className="flex shrink-0 items-center gap-2 text-xs">
          {/* Git stats */}
          {gitStats && (gitStats.additions > 0 || gitStats.deletions > 0) && (
            <span className="flex items-center gap-1 tabular-nums">
              <span className="text-green-400">+{gitStats.additions}</span>
              <span className="text-red-400">-{gitStats.deletions}</span>
            </span>
          )}

          {/* PR badge - clickable link to open PR */}
          {prStatus && prUrl ? (
            <a
              href={prUrl}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(e) => e.stopPropagation()}
              className={cn(
                'flex items-center gap-1 rounded px-1.5 py-0.5 transition-opacity hover:opacity-80',
                prStatus.state === 'open' && 'bg-green-500/10 text-green-400',
                prStatus.state === 'merged' && 'bg-purple-500/10 text-purple-400',
                prStatus.state === 'closed' && 'bg-red-500/10 text-red-400',
                prStatus.state === 'draft' && 'bg-orange-500/10 text-orange-400'
              )}
              aria-label={`Open PR #${prStatus.number}`}
            >
              <GitPullRequest className="h-3 w-3" />
              #{prStatus.number}
              {prStatus.checks_passing && ' ✓'}
            </a>
          ) : prStatus ? (
            <span
              className={cn(
                'flex items-center gap-1 rounded px-1.5 py-0.5',
                prStatus.state === 'open' && 'bg-green-500/10 text-green-400',
                prStatus.state === 'merged' && 'bg-purple-500/10 text-purple-400',
                prStatus.state === 'closed' && 'bg-red-500/10 text-red-400',
                prStatus.state === 'draft' && 'bg-orange-500/10 text-orange-400'
              )}
              aria-label={`PR #${prStatus.number}`}
            >
              <GitPullRequest className="h-3 w-3" />
              #{prStatus.number}
              {prStatus.checks_passing && ' ✓'}
            </span>
          ) : null}

          {onArchive && (
            <button
              onClick={(event) => {
                event.stopPropagation();
                onArchive();
              }}
              className={cn(
                'flex items-center justify-center rounded p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text',
                'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'
              )}
              aria-label={`Archive workspace ${workspace.name}`}
            >
              <Archive className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

interface RepositorySectionProps {
  repository: Repository;
  workspaces: Workspace[];
  selectedWorkspaceId?: string | null;
  onSelectWorkspace?: (workspace: Workspace) => void;
  onArchiveWorkspace?: (workspace: Workspace) => void;
  onNewWorkspace?: () => void;
}

function RepositorySection({
  repository,
  workspaces,
  selectedWorkspaceId,
  onSelectWorkspace,
  onArchiveWorkspace,
  onNewWorkspace,
}: RepositorySectionProps) {
  const [expanded, setExpanded] = useState(true);

  return (
    <div className="mb-2">
      {/* Repository header */}
      <button
        onClick={() => setExpanded(!expanded)}
        aria-label={expanded ? 'Collapse repository' : 'Expand repository'}
        aria-expanded={expanded}
        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm font-medium text-text transition-colors hover:bg-surface-elevated"
      >
        <span className="text-text-muted">
          {expanded ? (
            <ChevronDown className="h-4 w-4" />
          ) : (
            <ChevronRight className="h-4 w-4" />
          )}
        </span>
        <span className="truncate">{repository.name}</span>
      </button>

      {/* Workspaces under repository */}
      {expanded && (
        <div className="ml-2 mt-1 space-y-0.5">
          {/* New workspace button */}
          <button
            onClick={onNewWorkspace}
            className="group flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-sm text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          >
            <Plus className="h-3.5 w-3.5" />
            <span>New workspace</span>
            <MoreHorizontal className="ml-auto h-4 w-4 opacity-0 group-hover:opacity-100" />
          </button>

          {/* Workspace list */}
          {workspaces.map((workspace) => (
            <WorkspaceItem
              key={workspace.id}
              repository={repository}
              workspace={workspace}
              isSelected={workspace.id === selectedWorkspaceId}
              onSelect={() => onSelectWorkspace?.(workspace)}
              onArchive={onArchiveWorkspace ? () => onArchiveWorkspace(workspace) : undefined}
            />
          ))}
        </div>
      )}
    </div>
  );
}

interface SidebarProps {
  selectedWorkspaceId?: string | null;
  onSelectWorkspace?: (workspace: Workspace) => void;
  onCreateWorkspace?: (repository: Repository) => void;
  onArchiveWorkspace?: (workspace: Workspace) => void;
}

export function Sidebar({
  selectedWorkspaceId,
  onSelectWorkspace,
  onCreateWorkspace,
  onArchiveWorkspace,
}: SidebarProps) {
  const { data: repositories = [] } = useRepositories();
  const { data: workspaces = [] } = useWorkspaces();
  const { data: agents = [] } = useAgents();
  const [workspacesExpanded, setWorkspacesExpanded] = useState(true);
  const [createWorkspaceRepo, setCreateWorkspaceRepo] = useState<Repository | null>(null);

  const availableAgents = agents.filter((a) => a.available);

  // Group workspaces by repository
  const workspacesByRepo = repositories.reduce((acc, repo) => {
    acc[repo.id] = workspaces.filter((w) => w.repository_id === repo.id);
    return acc;
  }, {} as Record<string, Workspace[]>);

  const handleNewWorkspace = (repository: Repository) => {
    if (onCreateWorkspace) {
      onCreateWorkspace(repository);
      return;
    }
    setCreateWorkspaceRepo(repository);
  };

  const handleWorkspaceCreated = (workspace: Workspace) => {
    setCreateWorkspaceRepo(null);
    onSelectWorkspace?.(workspace);
  };

  return (
    <aside className="flex w-72 flex-col border-r border-border bg-surface">
      {/* Logo */}
      <div className="flex items-center gap-3 border-b border-border px-4 py-4">
        <div className="flex size-8 items-center justify-center rounded-lg bg-accent/20">
          <Logo className="h-6 w-6" />
        </div>
        <span className="text-lg font-semibold">Conduit</span>
      </div>

      {/* Workspaces section */}
      <nav className="flex-1 overflow-y-auto p-3">
        {/* Section header */}
        <button
          onClick={() => setWorkspacesExpanded(!workspacesExpanded)}
          aria-label={workspacesExpanded ? 'Collapse workspaces' : 'Expand workspaces'}
          aria-expanded={workspacesExpanded}
          className="mb-2 flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm font-semibold text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
        >
          <FolderGit2 className="h-4 w-4" />
          <span>Workspaces</span>
        </button>

        {workspacesExpanded && (
          <div className="space-y-1">
            {repositories.length === 0 ? (
              <p className="px-3 py-2 text-xs text-text-muted">No repositories</p>
            ) : (
              repositories.map((repo) => (
                <RepositorySection
                  key={repo.id}
                  repository={repo}
                  workspaces={workspacesByRepo[repo.id] || []}
                  selectedWorkspaceId={selectedWorkspaceId}
                  onSelectWorkspace={onSelectWorkspace}
                  onArchiveWorkspace={onArchiveWorkspace}
                  onNewWorkspace={() => handleNewWorkspace(repo)}
                />
              ))
            )}
          </div>
        )}
      </nav>

      {/* Available agents indicator */}
      <div className="border-t border-border p-3">
        <div className="flex items-center justify-center gap-2 text-xs">
          {availableAgents.map((agent) => (
            <span
              key={agent.id}
              className={cn(
                'rounded px-1.5 py-0.5',
                agent.id === 'claude'
                  ? 'bg-orange-400/10 text-orange-400'
                  : agent.id === 'codex'
                  ? 'bg-green-400/10 text-green-400'
                  : 'bg-blue-400/10 text-blue-400'
              )}
            >
              {agent.name}
            </span>
          ))}
        </div>
      </div>

      {/* Create Workspace Dialog */}
      {createWorkspaceRepo && (
        <CreateWorkspaceDialog
          repositoryId={createWorkspaceRepo.id}
          repositoryName={createWorkspaceRepo.name}
          isOpen={!!createWorkspaceRepo}
          onClose={() => setCreateWorkspaceRepo(null)}
          onSuccess={handleWorkspaceCreated}
        />
      )}
    </aside>
  );
}
