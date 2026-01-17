import { cn } from '../../lib/cn';

interface InlineCodeProps {
  children: React.ReactNode;
  className?: string;
}

export function InlineCode({ children, className }: InlineCodeProps) {
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
