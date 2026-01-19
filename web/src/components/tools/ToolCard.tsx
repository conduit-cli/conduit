import { useState, type ReactNode } from 'react';
import { ChevronRight, Loader2 } from 'lucide-react';
import { cn } from '../../lib/cn';

export type ToolStatus = 'running' | 'success' | 'error';

interface ToolCardProps {
  icon: ReactNode;
  title: string;
  status: ToolStatus;
  subtitle?: ReactNode;
  children?: ReactNode;
  defaultOpen?: boolean;
  className?: string;
}

const statusStyles: Record<ToolStatus, { bg: string; border: string; icon: string }> = {
  running: {
    bg: 'bg-amber-500/10',
    border: 'border-amber-500/30',
    icon: 'text-amber-400',
  },
  success: {
    bg: 'bg-emerald-500/10',
    border: 'border-emerald-500/30',
    icon: 'text-emerald-400',
  },
  error: {
    bg: 'bg-red-500/10',
    border: 'border-red-500/30',
    icon: 'text-red-400',
  },
};

export function ToolCard({
  icon,
  title,
  status,
  subtitle,
  children,
  defaultOpen = true,
  className,
}: ToolCardProps) {
  const [isOpen, setIsOpen] = useState(defaultOpen);
  const styles = statusStyles[status];

  return (
    <div
      className={cn(
        'rounded-lg border overflow-hidden transition-colors duration-200',
        styles.border,
        styles.bg,
        className
      )}
    >
      {/* Header */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'flex w-full items-center gap-3 px-3 py-2',
          'hover:bg-black/10 transition-colors duration-150'
        )}
      >
        {/* Status indicator */}
        <div className={cn('flex items-center justify-center', styles.icon)}>
          {status === 'running' ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            icon
          )}
        </div>

        {/* Title and subtitle */}
        <div className="flex-1 min-w-0 text-left">
          <p className={cn('text-sm font-medium', styles.icon)}>{title}</p>
          {subtitle && (
            <p className="text-xs text-text-muted truncate">{subtitle}</p>
          )}
        </div>

        {/* Chevron */}
        {children && (
          <ChevronRight
            className={cn(
              'h-4 w-4 text-text-muted transition-transform duration-200',
              isOpen && 'rotate-90'
            )}
          />
        )}
      </button>

      {/* Content */}
      {children && isOpen && (
        <div className="border-t border-border/50">{children}</div>
      )}
    </div>
  );
}
