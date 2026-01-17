import { Search } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';

interface GrepToolCardProps {
  status: ToolStatus;
  pattern: string;
  path?: string;
  content?: string;
  error?: string;
}

interface GrepMatch {
  file: string;
  line?: number;
  text: string;
}

// Strip ANSI escape codes from terminal output
function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g, '');
}

function parseGrepOutput(content: string): GrepMatch[] {
  // Strip ANSI codes first
  const cleanContent = stripAnsi(content);
  const lines = cleanContent.split('\n').filter(Boolean);
  return lines.map(line => {
    // Format: file:line:text or file:text
    const colonIndex = line.indexOf(':');
    if (colonIndex === -1) {
      return { file: '', text: line };
    }

    const file = line.slice(0, colonIndex);
    const rest = line.slice(colonIndex + 1);

    // Try to parse line number
    const lineMatch = rest.match(/^(\d+):(.*)$/);
    if (lineMatch) {
      return {
        file,
        line: parseInt(lineMatch[1], 10),
        text: lineMatch[2],
      };
    }

    return { file, text: rest };
  });
}

function GrepMatchItem({ match, pattern }: { match: GrepMatch; pattern: string }) {
  // Highlight the pattern in the text
  const highlightText = (text: string, searchPattern: string) => {
    try {
      // Escape special regex characters in pattern for safe matching
      const escaped = searchPattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const regex = new RegExp(`(${escaped})`, 'gi');
      const parts = text.split(regex);

      // When splitting with a capturing group, matches are at odd indices
      // e.g., "a foo b".split(/(foo)/) => ["a ", "foo", " b"]
      //        indices:                      0      1      2
      // This avoids the global regex lastIndex bug entirely
      return parts.map((part, i) =>
        i % 2 === 1 ? (
          <mark key={i} className="bg-accent/30 text-accent rounded px-0.5">
            {part}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        )
      );
    } catch {
      return text;
    }
  };

  return (
    <div className="flex items-start gap-2 px-3 py-0.5 hover:bg-surface-elevated/50 font-mono text-xs">
      {match.line && (
        <span className="text-text-muted shrink-0 w-12 text-right">{match.line}</span>
      )}
      <span className="text-text-muted whitespace-pre-wrap break-all flex-1">
        {highlightText(match.text.trim(), pattern)}
      </span>
    </div>
  );
}

export function GrepToolCard({ status, pattern, path, content, error }: GrepToolCardProps) {
  const matches = content ? parseGrepOutput(content) : [];
  const matchCount = matches.length;

  return (
    <ToolCard
      icon={<Search className="h-4 w-4" />}
      title="Grep"
      subtitle={`"${pattern}"${path ? ` in ${path}` : ''} (${matchCount} ${matchCount === 1 ? 'match' : 'matches'})`}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : matches.length > 0 ? (
        <div className="max-h-[300px] overflow-auto py-1.5">
          {matches.slice(0, 50).map((match, idx) => (
            <GrepMatchItem key={idx} match={match} pattern={pattern} />
          ))}
          {matches.length > 50 && (
            <p className="px-3 py-1.5 text-xs text-text-muted italic">
              ... and {matches.length - 50} more matches
            </p>
          )}
        </div>
      ) : (
        <div className="p-3 text-xs text-text-muted">No matches found</div>
      )}
    </ToolCard>
  );
}
