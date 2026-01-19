import { useCallback } from 'react';
import type { Session, Workspace } from '../types';
import { useWebSocket } from './useWebSocket';
import { useCreateWorkspacePr, useForkSession } from './useApi';

interface UseWorkspaceActionsOptions {
  session: Session | null;
  workspace: Workspace | null;
  onForkedSession?: (session: Session, workspace: Workspace) => void;
}

export function useWorkspaceActions({
  session,
  workspace,
  onForkedSession,
}: UseWorkspaceActionsOptions) {
  const { sendPrompt } = useWebSocket();
  const forkSessionMutation = useForkSession();
  const createPrMutation = useCreateWorkspacePr();

  const handleForkSession = useCallback(() => {
    if (!session) return;
    forkSessionMutation.mutate(session.id, {
      onSuccess: (response) => {
        if (onForkedSession) {
          onForkedSession(response.session, response.workspace);
        }
        if (response.warnings.length > 0) {
          window.alert(`Fork warnings:\n${response.warnings.join('\n')}`);
        }
        sendPrompt(
          response.session.id,
          response.seed_prompt,
          response.workspace.path,
          response.session.model ?? undefined,
          true
        );
      },
    });
  }, [forkSessionMutation, onForkedSession, sendPrompt, session]);

  const handleCreatePr = useCallback(() => {
    if (!session || !workspace) return;
    createPrMutation.mutate(workspace.id, {
      onSuccess: (response) => {
        const preflight = response.preflight;
        if (!preflight.gh_installed) {
          window.alert('GitHub CLI (gh) is required to create PRs.');
          return;
        }
        if (!preflight.gh_authenticated) {
          window.alert('GitHub CLI is not authenticated. Run: gh auth login');
          return;
        }
        if (preflight.on_main_branch) {
          window.alert(
            `You are on ${preflight.branch_name}. Create a feature branch before opening a PR.`
          );
          return;
        }
        if (preflight.existing_pr?.url) {
          window.alert(`PR already exists: ${preflight.existing_pr.url}`);
          return;
        }

        const warnings: string[] = [];
        if (preflight.uncommitted_count > 0) {
          warnings.push(`${preflight.uncommitted_count} file(s) will be auto-committed`);
        }
        if (!preflight.has_upstream) {
          warnings.push('Branch will be pushed to remote');
        }
        if (warnings.length > 0) {
          const proceed = window.confirm(`Create PR?\n\n${warnings.join('\n')}`);
          if (!proceed) return;
        }
        sendPrompt(session.id, response.prompt, workspace.path, session.model ?? undefined);
      },
    });
  }, [createPrMutation, sendPrompt, session, workspace]);

  return {
    handleForkSession,
    handleCreatePr,
    isForking: forkSessionMutation.isPending,
    isCreatingPr: createPrMutation.isPending,
  };
}
