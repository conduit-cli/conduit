import { FolderPlus, Settings, UploadCloud } from 'lucide-react';
import { Logo } from '../Logo';

interface OnboardingEmptyStateProps {
  onAddProject: () => void;
  onSetBaseDir: () => void;
  onImportSession?: () => void;
}

export function OnboardingEmptyState({
  onAddProject,
  onSetBaseDir,
  onImportSession,
}: OnboardingEmptyStateProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 text-center text-text-muted">
      <div className="mb-5 flex size-16 items-center justify-center rounded-2xl bg-accent/15">
        <Logo className="h-9 w-9" />
      </div>
      <h2 className="mb-2 text-2xl font-semibold text-text">Welcome to Conduit</h2>
      <p className="mb-6 max-w-md text-sm">
        Add a project to get started. Conduit can scan a base directory for Git repos or you can
        add one manually.
      </p>
      <div className="flex flex-col items-center gap-3">
        <button
          onClick={onAddProject}
          className="flex items-center gap-2 rounded-lg bg-accent px-5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-accent-hover"
        >
          <FolderPlus className="h-4 w-4" />
          Add Project
        </button>
        <button
          onClick={onSetBaseDir}
          className="flex items-center gap-2 rounded-lg border border-border px-4 py-2 text-sm text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
        >
          <Settings className="h-4 w-4" />
          Set Projects Directory
        </button>
        {onImportSession && (
          <button
            onClick={onImportSession}
            className="flex items-center gap-2 rounded-lg px-4 py-2 text-sm text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          >
            <UploadCloud className="h-4 w-4" />
            Import Session
          </button>
        )}
      </div>
    </div>
  );
}
