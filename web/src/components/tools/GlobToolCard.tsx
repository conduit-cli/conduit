import { FolderSearch, File, Folder } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { cn } from '../../lib/cn';
import { FilePathLink } from '../FilePathLink';

interface GlobToolCardProps {
  status: ToolStatus;
  pattern: string;
  results?: string[];
  content?: string;
  error?: string;
}

function FileItem({ path }: { path: string }) {
  const isDir = path.endsWith('/');
  const name = path.split('/').filter(Boolean).pop() || path;
  const depth = (path.match(/\//g) || []).length;

  return (
    <div
      className={cn(
        'flex items-center gap-2 py-0.5 text-xs font-mono',
        'text-text-muted hover:text-text hover:bg-surface-elevated/50',
        'transition-colors duration-100'
      )}
      style={{ paddingLeft: `${Math.min(depth, 5) * 12 + 8}px` }}
    >
      {isDir ? (
        <Folder className="h-3.5 w-3.5 text-amber-400" />
      ) : (
        <File className="h-3.5 w-3.5 text-blue-400" />
      )}
      {isDir ? (
        <span className="truncate">{name}</span>
      ) : (
        <FilePathLink path={path} className="truncate text-xs">
          {name}
        </FilePathLink>
      )}
    </div>
  );
}

export function GlobToolCard({ status, pattern, results, content, error }: GlobToolCardProps) {
  // Parse results from content if not provided directly
  const files = results ?? (content ? content.split('\n').filter(Boolean) : []);
  const fileCount = files.length;

  return (
    <ToolCard
      icon={<FolderSearch className="h-4 w-4" />}
      title="Glob"
      subtitle={`${pattern} (${fileCount} ${fileCount === 1 ? 'match' : 'matches'})`}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : files.length > 0 ? (
        <div className="max-h-[300px] overflow-auto py-1">
          {files.slice(0, 100).map((file, idx) => (
            <FileItem key={idx} path={file} />
          ))}
          {files.length > 100 && (
            <p className="px-3 py-2 text-xs text-text-muted italic">
              ... and {files.length - 100} more files
            </p>
          )}
        </div>
      ) : (
        <div className="p-3 text-xs text-text-muted">No matches found</div>
      )}
    </ToolCard>
  );
}
