import { cn } from '../../lib/cn';
import { FilePathLink } from '../FilePathLink';

interface InlineCodeProps {
  children: React.ReactNode;
  className?: string;
}

// Check if text looks like a file path
function isFilePath(text: string): boolean {
  // Must have a file extension
  if (!/\.\w+$/.test(text)) return false;

  // Match common file path patterns:
  // - Absolute paths: /foo/bar.ts
  // - Relative paths: ./foo.ts, ../foo.ts
  // - Hidden directory paths: .opencode/foo.md
  // - Directory paths: src/foo.ts, components/Button.tsx
  return /^(\/|\.\.?\/|\.[a-zA-Z][\w.-]*\/|[a-zA-Z][\w.-]*\/)/.test(text);
}

export function InlineCode({ children, className }: InlineCodeProps) {
  // Check if children is a single string that looks like a file path
  const text = typeof children === 'string' ? children : null;
  const isPath = text && isFilePath(text);

  if (isPath) {
    return (
      <code
        className={cn(
          'rounded bg-surface-elevated px-1.5 py-0.5',
          'text-[0.9em] font-mono',
          'border border-border/50',
          className
        )}
      >
        <FilePathLink path={text} className="text-text-bright" />
      </code>
    );
  }

  return (
    <code
      className={cn(
        'rounded bg-surface-elevated px-1.5 py-0.5',
        'text-[0.9em] font-mono text-text-bright',
        'border border-border/50',
        className
      )}
    >
      {children}
    </code>
  );
}
