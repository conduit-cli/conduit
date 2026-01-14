import { useHealth } from '../hooks';
import { Circle, Settings } from 'lucide-react';
import { cn } from '../lib/cn';
import { ThemeSwitcher } from './ThemeSwitcher';

export function Header() {
  const { data: health, isLoading, isError } = useHealth();

  const statusColor = isLoading
    ? 'text-yellow-400'
    : isError
    ? 'text-red-400'
    : health?.status === 'ok'
    ? 'text-green-400'
    : 'text-red-400';

  return (
    <header className="flex h-14 items-center justify-between border-b border-border bg-surface px-6">
      <div className="flex items-center gap-4">
        <h1 className="text-sm font-medium text-text-muted">Dashboard</h1>
      </div>
      <div className="flex items-center gap-4">
        {/* Status indicator */}
        <div className="flex items-center gap-2 text-xs text-text-muted">
          <Circle className={cn('h-2 w-2 fill-current', statusColor)} />
          <span>
            {isLoading
              ? 'Connecting...'
              : isError
              ? 'Disconnected'
              : `v${health?.version}`}
          </span>
        </div>
        {/* Theme switcher */}
        <ThemeSwitcher />
        {/* Settings button */}
        <button
          aria-label="Settings"
          className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
        >
          <Settings className="h-4 w-4" />
        </button>
      </div>
    </header>
  );
}
