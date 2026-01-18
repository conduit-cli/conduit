import { useEffect, useMemo, useRef, useState } from 'react';
import { Loader2, Search, X } from 'lucide-react';
import {
  useAddOnboardingProject,
  useOnboardingProjects,
  useOnboardingBaseDir,
} from '../../hooks';
import { cn } from '../../lib/cn';

interface ProjectPickerDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onAdded: () => void;
}

export function ProjectPickerDialog({
  isOpen,
  onClose,
  onAdded,
}: ProjectPickerDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const { data: baseDir } = useOnboardingBaseDir({ enabled: isOpen });
  const {
    data,
    isLoading,
    error,
    refetch,
  } = useOnboardingProjects({ enabled: isOpen });
  const { mutate, isPending, error: addError, reset } = useAddOnboardingProject();
  const [query, setQuery] = useState('');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

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
    setQuery('');
    setSelectedPath(null);
    refetch();
  }, [isOpen, refetch]);

  const filteredProjects = useMemo(() => {
    const projects = data?.projects ?? [];
    if (!query.trim()) return projects;
    const needle = query.trim().toLowerCase();
    return projects.filter((project) => project.name.toLowerCase().includes(needle));
  }, [data?.projects, query]);

  const handleAdd = (path: string) => {
    setSelectedPath(path);
    mutate(
      { path },
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
      className="m-auto w-full max-w-2xl rounded-xl border border-border bg-surface p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex flex-col">
        <div className="flex items-center justify-between border-b border-border px-6 py-4">
          <div>
            <h2 className="text-lg font-semibold text-text">Add a project</h2>
            <p className="text-xs text-text-muted">
              {baseDir?.base_dir ? `Scanning ${baseDir.base_dir}` : 'Set a projects directory first.'}
            </p>
          </div>
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="px-6 py-4">
          <div className="flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-3 py-2">
            <Search className="h-4 w-4 text-text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search projects"
              className="flex-1 bg-transparent text-sm text-text outline-none"
            />
          </div>

          <div className="mt-4 max-h-[360px] overflow-y-auto rounded-lg border border-border">
            {isLoading ? (
              <div className="flex items-center justify-center gap-2 py-10 text-sm text-text-muted">
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading projects…
              </div>
            ) : error ? (
              <div className="px-4 py-6 text-sm text-red-400">
                {(error as Error).message}
              </div>
            ) : filteredProjects.length === 0 ? (
              <div className="px-4 py-6 text-sm text-text-muted">
                No git repositories found in this directory.
              </div>
            ) : (
              <ul className="divide-y divide-border">
                {filteredProjects.map((project) => (
                  <li key={project.path} className="flex items-center justify-between px-4 py-3">
                    <div>
                      <p className="text-sm font-medium text-text">{project.name}</p>
                      <p className="text-xs text-text-muted">{project.path}</p>
                    </div>
                    <button
                      onClick={() => handleAdd(project.path)}
                      disabled={isPending && selectedPath === project.path}
                      className={cn(
                        'rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text',
                        isPending && selectedPath === project.path && 'opacity-70'
                      )}
                    >
                      {isPending && selectedPath === project.path ? 'Adding…' : 'Add'}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {addError && (
            <div className="mt-3 rounded-lg bg-red-500/10 px-3 py-2 text-sm text-red-400">
              {addError instanceof Error ? addError.message : 'Failed to add project'}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-3 border-t border-border px-6 py-4">
          <button
            onClick={onClose}
            disabled={isPending}
            className="rounded-lg px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
          >
            Close
          </button>
        </div>
      </div>
    </dialog>
  );
}
