import { Search } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { FilePathLink } from '../FilePathLink';

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

interface GrepParseResult {
  matches: GrepMatch[];
  summaryCount?: number;
}

interface GrepGroup {
  file: string;
  matches: GrepMatch[];
}

// Strip ANSI escape codes from terminal output
function stripAnsi(text: string): string {
  return text.replace(
    new RegExp('\\x1B(?:[@-Z\\\\-_]|\\[[0-?]*[ -/]*[@-~])', 'g'),
    ''
  );
}

// NOTE: Kept pure and side-effect free for future tests.
function parseGrepOutput(content: string): GrepParseResult {
  const cleanContent = stripAnsi(content);
  const lines = cleanContent
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line) => line.length > 0);

  const matches: GrepMatch[] = [];
  let summaryCount: number | undefined;
  let currentFile = '';

  for (const rawLine of lines) {
    const line = rawLine.trim();
    const summaryMatch = line.match(/^Found\s+(\d+)\s+matches?/i);
    if (summaryMatch) {
      summaryCount = Number.parseInt(summaryMatch[1], 10);
      continue;
    }

    const lineMatch = line.match(/^Line\s+(\d+)\s*:?\s*(.*)$/i);
    if (lineMatch) {
      matches.push({
        file: currentFile,
        line: Number.parseInt(lineMatch[1], 10),
        text: lineMatch[2] ?? '',
      });
      continue;
    }

    const standardMatch = line.match(/^(.*):(\d+)(?::(\d+))?:(.*)$/);
    if (standardMatch) {
      matches.push({
        file: standardMatch[1],
        line: Number.parseInt(standardMatch[2], 10),
        text: standardMatch[4],
      });
      continue;
    }

    const looksLikePath =
      line.startsWith('/') ||
      line.startsWith('./') ||
      line.startsWith('../') ||
      (line.includes('/') && !line.includes(' '));
    if (looksLikePath) {
      currentFile = line;
      continue;
    }

    const colonIndex = line.indexOf(':');
    if (colonIndex > 0) {
      const file = line.slice(0, colonIndex).trim();
      const rest = line.slice(colonIndex + 1).trim();
      matches.push({ file, text: rest });
      continue;
    }

    matches.push({ file: currentFile, text: line });
  }

  return { matches, summaryCount };
}

function groupMatches(matches: GrepMatch[]): GrepGroup[] {
  const groups: GrepGroup[] = [];
  const index = new Map<string, number>();

  for (const match of matches) {
    const key = match.file ?? '';
    const existingIndex = index.get(key);
    if (existingIndex === undefined) {
      index.set(key, groups.length);
      groups.push({ file: key, matches: [match] });
    } else {
      groups[existingIndex].matches.push(match);
    }
  }

  return groups;
}

function GrepMatchItem({ match, pattern }: { match: GrepMatch; pattern: string }) {
  const highlightText = (text: string, searchPattern: string) => {
    if (!searchPattern || !searchPattern.trim()) {
      return [<span key="0">{text}</span>];
    }
    try {
      const escaped = searchPattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const regex = new RegExp(`(${escaped})`, 'gi');
      const parts = text.split(regex);

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
      {match.line !== undefined && (
        <span className="text-text-muted shrink-0">{match.line}</span>
      )}
      <span className="text-terminal-text whitespace-pre-wrap break-all flex-1">
        {highlightText(match.text.trim(), pattern)}
      </span>
    </div>
  );
}

export function GrepToolCard({ status, pattern, path, content, error }: GrepToolCardProps) {
  const { matches, summaryCount } = content ? parseGrepOutput(content) : { matches: [] };
  const matchCount = summaryCount ?? matches.length;
  const hasContent = Boolean(content && content.trim().length > 0);

  const maxMatches = 200;
  const visibleMatches = matches.slice(0, maxMatches);
  const groupedMatches = groupMatches(visibleMatches);
  const remaining = Math.max(0, (summaryCount ?? matches.length) - visibleMatches.length);

  return (
    <ToolCard
      icon={<Search className="h-4 w-4" />}
      title="Grep"
      subtitle={`"${pattern}"${path ? ` in ${path}` : ''} (${matchCount} ${matchCount === 1 ? 'match' : 'matches'})`}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : (
        <div className="p-2">
          <div className="rounded-lg border border-border bg-terminal-bg">
            {groupedMatches.length > 0 ? (
              <div className="max-h-[300px] overflow-auto py-1.5">
                {groupedMatches.map((group, idx) => (
                  <div key={`${group.file}-${idx}`} className="mb-2 last:mb-0">
                    <div className="px-3 py-1 text-xs text-text-muted flex items-center gap-2 border-b border-border/50">
                      {group.file ? (
                        <FilePathLink path={group.file} className="truncate text-xs" />
                      ) : (
                        <span className="text-text-muted">Matches</span>
                      )}
                      <span className="text-text-muted">({group.matches.length})</span>
                    </div>
                    <div className="py-1">
                      {group.matches.map((match, matchIndex) => (
                        <GrepMatchItem key={`${idx}-${matchIndex}`} match={match} pattern={pattern} />
                      ))}
                    </div>
                  </div>
                ))}
                {remaining > 0 && (
                  <p className="px-3 py-1.5 text-xs text-text-muted italic">
                    ... and {remaining} more matches
                  </p>
                )}
              </div>
            ) : hasContent ? (
              <pre className="max-h-[300px] overflow-auto whitespace-pre-wrap break-words p-3 text-xs text-terminal-text">
                {content}
              </pre>
            ) : (
              <div className="p-3 text-xs text-text-muted">No matches found</div>
            )}
          </div>
        </div>
      )}
    </ToolCard>
  );
}
