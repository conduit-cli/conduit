import { useEffect, useRef, useState } from 'react';
import { Loader2, X } from 'lucide-react';
import { useAddOnboardingProject } from '../../hooks';
import { cn } from '../../lib/cn';

interface AddProjectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onAdded: () => void;
}

export function AddProjectDialog({ isOpen, onClose, onAdded }: AddProjectDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const { mutate, isPending, error, reset } = useAddOnboardingProject();
  const [value, setValue] = useState('');

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
  }, [isPending, onClose]);

  useEffect(() => {
    if (!isOpen) return;
    setValue('');
  }, [isOpen]);

  const handleAdd = () => {
    if (!value.trim()) return;
    mutate(
      { path: value.trim() },
      {
        onSuccess: () => {
          onAdded();
        },
      }
    );
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
      className="m-auto w-full max-w-lg rounded-xl border border-border bg-surface p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex flex-col">
        <div className="flex items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold text-text">Add custom project</h2>
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="px-6 py-5">
          <p className="text-sm text-text-muted">Enter the path to a local git repository.</p>
          <input
            value={value}
            onChange={(event) => setValue(event.target.value)}
            className={cn(
              'mt-4 w-full rounded-lg border border-border bg-surface-elevated px-3 py-2 text-sm text-text',
              'focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/30'
            )}
            placeholder="/Users/you/projects/my-repo"
          />
          {error && (
            <div className="mt-3 rounded-lg bg-red-500/10 px-3 py-2 text-sm text-red-400">
              {error instanceof Error ? error.message : 'Failed to add project'}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-3 border-t border-border px-6 py-4">
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-lg px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={handleAdd}
            disabled={isPending || !value.trim()}
            className={cn(
              'flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-70',
              isPending && 'cursor-wait'
            )}
          >
            {isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            Add project
          </button>
        </div>
      </div>
    </dialog>
  );
}
