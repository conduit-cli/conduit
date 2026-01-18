import { ClipboardList } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { MarkdownBody } from '../markdown';

interface TaskToolCardProps {
  status: ToolStatus;
  description?: string;
  prompt?: string;
  output?: string;
  error?: string;
}

export function TaskToolCard({
  status,
  description,
  prompt,
  output,
  error,
}: TaskToolCardProps) {
  return (
    <ToolCard
      icon={<ClipboardList className="h-4 w-4" />}
      title="Task"
      subtitle={description}
      status={status}
    >
      {prompt && (
        <div className="border-b border-border/50 bg-surface-elevated/40 px-3 py-2">
          <p className="text-[11px] uppercase tracking-wide text-text-muted">Prompt</p>
          <pre className="mt-1 whitespace-pre-wrap break-words text-xs text-text-muted">
            {prompt}
          </pre>
        </div>
      )}
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : output ? (
        <div className="p-3">
          <MarkdownBody content={output} />
        </div>
      ) : (
        <div className="p-3 text-xs text-text-muted">No output yet</div>
      )}
    </ToolCard>
  );
}
