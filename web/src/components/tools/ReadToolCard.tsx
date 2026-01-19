import { FileText } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { CodeBlock } from '../markdown/CodeBlock';
import { FilePathLink } from '../FilePathLink';

interface ReadToolCardProps {
  status: ToolStatus;
  filePath: string;
  content?: string;
  error?: string;
}

// Strip cat -n style line numbers from content
// Format: "     1→\tcontent" (spaces, number, arrow, tab, content)
function stripCatLineNumbers(content: string): { stripped: string; hasLineNumbers: boolean } {
  const lines = content.split('\n');
  // Check if content has cat -n format (line starts with spaces + number + → or tab)
  const catLineRegex = /^\s*\d+[→\t]\s*/;
  const hasCatFormat = lines.length > 0 && lines.slice(0, 5).some(line => catLineRegex.test(line));

  if (!hasCatFormat) {
    return { stripped: content, hasLineNumbers: false };
  }

  // Strip the line number prefix from each line
  const stripped = lines.map(line => line.replace(/^\s*\d+[→\t]\s?/, '')).join('\n');
  return { stripped, hasLineNumbers: true };
}

function getLanguageFromPath(path: string): string | undefined {
  const ext = path.split('.').pop()?.toLowerCase();
  const extMap: Record<string, string> = {
    ts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    jsx: 'jsx',
    py: 'python',
    rs: 'rust',
    go: 'go',
    json: 'json',
    yaml: 'yaml',
    yml: 'yaml',
    toml: 'toml',
    md: 'markdown',
    html: 'html',
    css: 'css',
    sql: 'sql',
    sh: 'bash',
    bash: 'bash',
  };
  return ext ? extMap[ext] : undefined;
}

export function ReadToolCard({ status, filePath, content, error }: ReadToolCardProps) {
  const language = getLanguageFromPath(filePath);

  // Strip cat -n line numbers if present
  const { stripped: cleanContent, hasLineNumbers } = content
    ? stripCatLineNumbers(content)
    : { stripped: '', hasLineNumbers: false };

  return (
    <ToolCard
      icon={<FileText className="h-4 w-4" />}
      title="Read"
      subtitle={<FilePathLink path={filePath} className="text-xs" />}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : cleanContent ? (
        <div className="p-2">
          <CodeBlock code={cleanContent} language={language} showLineNumbers={hasLineNumbers} />
        </div>
      ) : null}
    </ToolCard>
  );
}
