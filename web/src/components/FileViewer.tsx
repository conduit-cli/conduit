import { X, Copy, FileText, Loader2, AlertTriangle } from 'lucide-react';
import { cn } from '../lib/cn';
import { MarkdownBody } from './markdown';
import { CodeBlock } from './markdown/CodeBlock';
import { useFileContent } from '../hooks/useApi';

interface FileViewerProps {
  filePath: string;
  workspaceId: string;
  onClose: () => void;
}

function getFileExtension(path: string): string {
  const parts = path.split('.');
  return parts.length > 1 ? parts.pop()!.toLowerCase() : '';
}

function getLanguageFromPath(path: string): string | undefined {
  const ext = getFileExtension(path);
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
    scss: 'scss',
    sql: 'sql',
    sh: 'bash',
    bash: 'bash',
    zsh: 'bash',
    c: 'c',
    cpp: 'cpp',
    h: 'c',
    hpp: 'cpp',
    java: 'java',
    rb: 'ruby',
    php: 'php',
    swift: 'swift',
    kt: 'kotlin',
    lua: 'lua',
    r: 'r',
    xml: 'xml',
    svg: 'xml',
  };
  return extMap[ext];
}

function isImageFile(path: string): boolean {
  const ext = getFileExtension(path);
  return ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp'].includes(ext);
}

function isMarkdownFile(path: string): boolean {
  const ext = getFileExtension(path);
  return ['md', 'markdown', 'mdx'].includes(ext);
}

function getFileName(path: string): string {
  const parts = path.split('/');
  return parts[parts.length - 1] || path;
}

export function FileViewer({ filePath, workspaceId, onClose }: FileViewerProps) {
  const { data, isLoading, error } = useFileContent(workspaceId, filePath);

  const handleCopyPath = async () => {
    try {
      await navigator.clipboard.writeText(filePath);
    } catch (err) {
      console.error('Failed to copy path', err);
    }
  };

  const fileName = getFileName(filePath);
  const isImage = isImageFile(filePath);
  const isMarkdown = isMarkdownFile(filePath);
  const language = getLanguageFromPath(filePath);

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border bg-surface px-4 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-text-muted" />
          <span className="truncate text-sm font-medium text-text" title={filePath}>
            {fileName}
          </span>
          <span className="hidden truncate text-xs text-text-muted sm:block" title={filePath}>
            {filePath}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={handleCopyPath}
            className={cn(
              'rounded p-1.5 text-text-muted transition-colors',
              'hover:bg-surface-elevated hover:text-text'
            )}
            title="Copy path"
          >
            <Copy className="h-4 w-4" />
          </button>
          <button
            onClick={onClose}
            className={cn(
              'rounded p-1.5 text-text-muted transition-colors',
              'hover:bg-surface-elevated hover:text-text'
            )}
            title="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto">
        {isLoading && (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
          </div>
        )}

        {error && (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-4">
            <AlertTriangle className="h-8 w-8 text-error" />
            <p className="text-sm text-error">Failed to load file</p>
            <p className="text-xs text-text-muted">{String(error)}</p>
          </div>
        )}

        {data && !data.exists && (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-4">
            <AlertTriangle className="h-8 w-8 text-warning" />
            <p className="text-sm text-text-muted">File not found</p>
          </div>
        )}

        {data?.exists && data.content && (
          <>
            {isImage && data.encoding === 'base64' ? (
              <div className="flex items-center justify-center p-4">
                <img
                  src={`data:${data.media_type};base64,${data.content}`}
                  alt={fileName}
                  className="max-h-[80vh] max-w-full object-contain"
                />
              </div>
            ) : isMarkdown ? (
              <div className="p-4">
                <MarkdownBody content={data.content} />
              </div>
            ) : (
              <div className="p-2">
                <CodeBlock
                  code={data.content}
                  language={language}
                  showLineNumbers
                  maxHeight={undefined}
                />
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
