import { cn } from '../lib/cn';
import { useFileViewer } from '../contexts/FileViewerContext';

interface FilePathLinkProps {
  path: string;
  workspaceId?: string | null;
  className?: string;
  children?: React.ReactNode;
}

export function FilePathLink({ path, workspaceId, className, children }: FilePathLinkProps) {
  const fileViewer = useFileViewer();

  // Use provided workspaceId or fall back to context
  const resolvedWorkspaceId = workspaceId ?? fileViewer?.currentWorkspaceId;

  // If no workspace context or file viewer context, render as plain text
  if (!resolvedWorkspaceId || !fileViewer) {
    return (
      <span className={cn('font-mono', className)}>
        {children ?? path}
      </span>
    );
  }

  const handleClick = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    fileViewer.openFile(path, resolvedWorkspaceId);
  };

  return (
    <button
      onClick={handleClick}
      className={cn(
        'font-mono text-accent hover:underline cursor-pointer text-left',
        'focus:outline-none focus:ring-1 focus:ring-accent/50 rounded',
        className
      )}
      title={`Open ${path}`}
    >
      {children ?? path}
    </button>
  );
}
