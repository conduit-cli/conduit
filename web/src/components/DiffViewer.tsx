import { useMemo, useState } from 'react';
import { ChevronDown, ChevronUp, FileEdit } from 'lucide-react';
import { cn } from '../lib/cn';
import { parseDiff, type DiffFile, type DiffLine } from '../lib/diffParser';
import { CopyButton } from './ui/CopyButton';

interface DiffViewerProps {
  diff: string;
  maxHeight?: number;
  className?: string;
}

const DEFAULT_MAX_HEIGHT = 400;
const COLLAPSE_THRESHOLD = 30; // lines

function DiffLineComponent({ line, showLineNumbers }: { line: DiffLine; showLineNumbers: boolean }) {
  const lineStyles = {
    add: 'bg-diff-add-bg text-success',
    remove: 'bg-diff-remove-bg text-error',
    context: 'text-text-muted',
    header: 'text-text-muted bg-surface-elevated/50 font-medium',
    hunk: 'text-accent bg-accent/10 font-medium',
  };

  const prefix = {
    add: '+',
    remove: '-',
    context: ' ',
    header: '',
    hunk: '',
  };

  return (
    <div className={cn('flex font-mono text-xs', lineStyles[line.type])}>
      {showLineNumbers && line.type !== 'header' && line.type !== 'hunk' && (
        <span className="w-8 shrink-0 select-none px-2 text-right text-text-faint">
          {line.oldLineNumber ?? line.newLineNumber ?? ''}
        </span>
      )}
      <span className="w-4 shrink-0 select-none text-center">
        {prefix[line.type]}
      </span>
      <span className="flex-1 whitespace-pre overflow-x-auto px-2 py-0.5">
        {line.content}
      </span>
    </div>
  );
}

function DiffFileComponent({
  file,
  showLineNumbers,
  expanded,
}: {
  file: DiffFile;
  showLineNumbers: boolean;
  expanded: boolean;
}) {
  const [isFileExpanded, setIsFileExpanded] = useState(true);

  return (
    <div className="border-b border-border last:border-b-0">
      {/* File header */}
      <button
        onClick={() => setIsFileExpanded(!isFileExpanded)}
        className="flex w-full items-center gap-2 px-3 py-2 bg-surface-elevated hover:bg-bg-highlight transition-colors"
      >
        <FileEdit className="h-3.5 w-3.5 text-amber-400" />
        <span className="flex-1 text-left text-sm font-mono text-text truncate">
          {file.newPath || file.oldPath}
        </span>
        <span className="flex items-center gap-2 text-xs">
          {file.additions > 0 && (
            <span className="text-success">+{file.additions}</span>
          )}
          {file.deletions > 0 && (
            <span className="text-error">-{file.deletions}</span>
          )}
        </span>
        {isFileExpanded ? (
          <ChevronUp className="h-4 w-4 text-text-muted" />
        ) : (
          <ChevronDown className="h-4 w-4 text-text-muted" />
        )}
      </button>

      {/* File diff content */}
      {isFileExpanded && (
        <div className={cn('overflow-auto', !expanded && 'max-h-[300px]')}>
          {file.lines
            .filter(line => line.type !== 'header')
            .map((line, idx) => (
              <DiffLineComponent
                key={idx}
                line={line}
                showLineNumbers={showLineNumbers}
              />
            ))}
        </div>
      )}
    </div>
  );
}

export function DiffViewer({
  diff,
  maxHeight = DEFAULT_MAX_HEIGHT,
  className,
}: DiffViewerProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [showLineNumbers, setShowLineNumbers] = useState(true);

  const parsed = useMemo(() => parseDiff(diff), [diff]);

  const totalLines = parsed.files.reduce((acc, file) => acc + file.lines.length, 0);
  const shouldCollapse = totalLines > COLLAPSE_THRESHOLD;

  const totalAdditions = parsed.files.reduce((acc, file) => acc + file.additions, 0);
  const totalDeletions = parsed.files.reduce((acc, file) => acc + file.deletions, 0);

  if (parsed.files.length === 0) {
    return (
      <pre className={cn('p-3 text-xs text-text-muted overflow-auto rounded-lg border border-border bg-surface', className)}>
        {diff}
      </pre>
    );
  }

  return (
    <div className={cn('rounded-lg border border-border overflow-hidden', className)}>
      {/* Header with stats */}
      <div className="flex items-center justify-between px-3 py-2 bg-surface-elevated border-b border-border">
        <div className="flex items-center gap-3">
          <span className="text-xs font-medium text-text-muted">
            {parsed.files.length} file{parsed.files.length !== 1 ? 's' : ''}
          </span>
          <span className="flex items-center gap-2 text-xs">
            <span className="text-success">+{totalAdditions}</span>
            <span className="text-error">-{totalDeletions}</span>
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowLineNumbers(!showLineNumbers)}
            className={cn(
              'text-xs px-2 py-0.5 rounded',
              showLineNumbers
                ? 'bg-accent/20 text-accent'
                : 'text-text-muted hover:text-text'
            )}
          >
            #
          </button>
          <CopyButton text={diff} />
        </div>
      </div>

      {/* Diff content */}
      <div
        className={cn('bg-background', !isExpanded && shouldCollapse && 'overflow-hidden')}
        style={!isExpanded && shouldCollapse ? { maxHeight } : undefined}
      >
        {parsed.files.map((file, idx) => (
          <DiffFileComponent
            key={idx}
            file={file}
            showLineNumbers={showLineNumbers}
            expanded={isExpanded}
          />
        ))}
      </div>

      {/* Expand/collapse button */}
      {shouldCollapse && (
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className={cn(
            'flex w-full items-center justify-center gap-1 py-1.5',
            'bg-surface-elevated border-t border-border',
            'text-xs text-text-muted hover:text-text',
            'transition-colors duration-150'
          )}
        >
          {isExpanded ? (
            <>
              <ChevronUp className="h-3.5 w-3.5" />
              <span>Collapse</span>
            </>
          ) : (
            <>
              <ChevronDown className="h-3.5 w-3.5" />
              <span>Expand ({totalLines} lines)</span>
            </>
          )}
        </button>
      )}
    </div>
  );
}
