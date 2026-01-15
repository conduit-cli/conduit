import { useEffect, useRef } from 'react';
import { X, Loader2, Info } from 'lucide-react';
import { useAutoCreateWorkspace } from '../hooks';
import type { Workspace } from '../types';
import { cn } from '../lib/cn';

interface CreateWorkspaceDialogProps {
  repositoryId: string;
  repositoryName: string;
  isOpen: boolean;
  onClose: () => void;
  onSuccess: (workspace: Workspace) => void;
}

export function CreateWorkspaceDialog({
  repositoryId,
  repositoryName,
  isOpen,
  onClose,
  onSuccess,
}: CreateWorkspaceDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const { mutate, isPending, error, reset } = useAutoCreateWorkspace();

  // Handle dialog open/close
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    if (isOpen) {
      dialog.showModal();
    } else {
      dialog.close();
      reset();
    }
  }, [isOpen, reset]);

  // Handle escape key
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const handleCancel = (e: Event) => {
      e.preventDefault();
      if (!isPending) {
        onClose();
      }
    };

    dialog.addEventListener('cancel', handleCancel);
    return () => dialog.removeEventListener('cancel', handleCancel);
  }, [onClose, isPending]);

  const handleCreate = () => {
    mutate(repositoryId, {
      onSuccess: (workspace) => {
        onSuccess(workspace);
      },
    });
  };

  const handleBackdropClick = (e: React.MouseEvent<HTMLDialogElement>) => {
    if (e.target === dialogRef.current && !isPending) {
      onClose();
    }
  };

  return (
    <dialog
      ref={dialogRef}
      onClick={handleBackdropClick}
      className="m-auto max-w-md rounded-xl border border-border bg-surface p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold text-text">Create New Workspace</h2>
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Content */}
        <div className="px-6 py-5">
          <p className="text-text">
            Create a new workspace in <span className="font-medium">"{repositoryName}"</span>?
          </p>

          <div className="mt-4 flex items-start gap-2 rounded-lg bg-accent/10 px-3 py-2.5 text-sm text-text-muted">
            <Info className="mt-0.5 h-4 w-4 shrink-0 text-accent" />
            <span>A unique name and branch will be generated automatically.</span>
          </div>

          {error && (
            <div className="mt-4 rounded-lg bg-red-500/10 px-3 py-2.5 text-sm text-red-400">
              {error instanceof Error ? error.message : 'Failed to create workspace'}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 border-t border-border px-6 py-4">
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-lg px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={isPending}
            className={cn(
              'flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-70',
              isPending && 'cursor-wait'
            )}
          >
            {isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            {isPending ? 'Creating...' : 'Create Workspace'}
          </button>
        </div>
      </div>
    </dialog>
  );
}
