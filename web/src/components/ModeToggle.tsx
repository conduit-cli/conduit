import { cn } from '../lib/cn';

interface ModeToggleProps {
  mode: 'build' | 'plan';
  onToggle: () => void;
  disabled?: boolean;
  size?: 'sm' | 'md';
}

export function ModeToggle({ mode, onToggle, disabled = false, size = 'sm' }: ModeToggleProps) {
  const isSmall = size === 'sm';

  return (
    <div
      className={cn(
        'inline-flex rounded-md border border-border/50 bg-surface-elevated/50',
        disabled && 'opacity-50'
      )}
      role="group"
      aria-label="Agent mode toggle"
    >
      <button
        type="button"
        onClick={() => mode !== 'build' && !disabled && onToggle()}
        disabled={disabled}
        className={cn(
          'rounded-l-[5px] transition-colors',
          isSmall ? 'px-2 py-0.5 text-xs' : 'px-3 py-1 text-sm',
          mode === 'build'
            ? 'bg-accent/20 text-accent font-medium'
            : 'text-text-muted hover:text-text',
          disabled ? 'cursor-not-allowed' : 'cursor-pointer'
        )}
        aria-pressed={mode === 'build'}
        title="Build mode"
      >
        Build
      </button>
      <button
        type="button"
        onClick={() => mode !== 'plan' && !disabled && onToggle()}
        disabled={disabled}
        className={cn(
          'rounded-r-[5px] transition-colors',
          isSmall ? 'px-2 py-0.5 text-xs' : 'px-3 py-1 text-sm',
          mode === 'plan'
            ? 'bg-accent/20 text-accent font-medium'
            : 'text-text-muted hover:text-text',
          disabled ? 'cursor-not-allowed' : 'cursor-pointer'
        )}
        aria-pressed={mode === 'plan'}
        title="Plan mode (Ctrl+Shift+P)"
      >
        Plan
      </button>
    </div>
  );
}
