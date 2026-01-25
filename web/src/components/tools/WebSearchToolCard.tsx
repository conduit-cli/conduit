import { Search } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { MarkdownBody } from '../markdown/MarkdownBody';

interface WebSearchToolCardProps {
  status: ToolStatus;
  query?: string;
  content?: string;
  error?: string;
}

function looksLikeJson(text: string): boolean {
  const trimmed = text.trim();
  return trimmed.startsWith('{') || trimmed.startsWith('[');
}

export function WebSearchToolCard({ status, query, content, error }: WebSearchToolCardProps) {
  const hasContent = Boolean(content && content.trim().length > 0);
  const subtitle = query ? `Query: ${query}` : undefined;

  return (
    <ToolCard icon={<Search className="h-4 w-4" />} title="Web search" subtitle={subtitle} status={status}>
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : hasContent ? (
        <div className="p-3">
          <div className="max-h-[320px] overflow-auto rounded-md border border-border/50 bg-surface-elevated/60 p-3 text-sm">
            {content && looksLikeJson(content) ? (
              <pre className="rounded bg-surface-elevated/60 p-2 text-xs text-text-muted overflow-x-auto">{content}</pre>
            ) : (
              <MarkdownBody content={content ?? ''} />
            )}
          </div>
        </div>
      ) : (
        <div className="p-3 text-xs text-text-muted">No results</div>
      )}
    </ToolCard>
  );
}
