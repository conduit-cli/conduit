import { Loader2 } from 'lucide-react';

interface SessionStatusIndicatorProps {
  label: string;
  elapsedSeconds?: number;
}

export function SessionStatusIndicator({ label, elapsedSeconds }: SessionStatusIndicatorProps) {
  return (
    <div className="flex items-center gap-2 text-xs text-text-muted">
      <Loader2 className="h-3.5 w-3.5 animate-spin text-text-muted" />
      <span>{label}</span>
      {typeof elapsedSeconds === 'number' && elapsedSeconds > 0 && (
        <span className="text-[10px] text-text-muted">• {elapsedSeconds}s</span>
      )}
    </div>
  );
}
