import { FilePlus } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { CodeBlock } from '../markdown/CodeBlock';
import { FilePathLink } from '../FilePathLink';

interface WriteToolCardProps {
  status: ToolStatus;
  filePath: string;
  content?: string;
  error?: string;
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

export function WriteToolCard({ status, filePath, content, error }: WriteToolCardProps) {
  const language = getLanguageFromPath(filePath);

  return (
    <ToolCard
      icon={<FilePlus className="h-4 w-4" />}
      title="Write"
      subtitle={<FilePathLink path={filePath} className="text-xs" />}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : content ? (
        <div className="p-2">
          <CodeBlock code={content} language={language} showLineNumbers />
        </div>
      ) : (
        <div className="p-3 text-xs text-text-muted">File written successfully</div>
      )}
    </ToolCard>
  );
}
