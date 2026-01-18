import type { ReactNode } from 'react';
import { cn } from '../lib/cn';
import {
  ReadToolCard,
  EditToolCard,
  WriteToolCard,
  BashToolCard,
  GlobToolCard,
  GrepToolCard,
  TodoWriteToolCard,
  type ToolStatus,
} from './tools';

interface ToolRunMessageProps {
  toolName?: string;
  toolArgs?: unknown;
  status: ToolStatus;
  output?: string | null;
  exitCode?: number | null;
}

function parseToolArgs(args: unknown): Record<string, unknown> {
  if (!args) return {};
  if (typeof args === 'string') {
    try {
      return JSON.parse(args);
    } catch {
      return { raw: args };
    }
  }
  if (typeof args === 'object') {
    return args as Record<string, unknown>;
  }
  return { raw: String(args) };
}

function stringifyArgs(args: unknown): string {
  if (args == null) return '';
  return typeof args === 'string' ? args : JSON.stringify(args, null, 2);
}

export function ToolRunMessage({
  toolName,
  toolArgs,
  status,
  output,
  exitCode,
}: ToolRunMessageProps) {
  const parsedArgs = parseToolArgs(toolArgs);
  const rawArg = parsedArgs.raw ? String(parsedArgs.raw) : '';
  const content = output ?? '';
  const isSuccess = status !== 'error';
  let body: ReactNode = null;

  if (!toolName) {
    body = content ? (
      <pre className="rounded bg-surface-elevated p-2 text-xs text-text-muted overflow-x-auto max-h-64 overflow-y-auto">
        {content}
      </pre>
    ) : null;
  } else {
    switch (toolName) {
      case 'Read': {
        const readPath = parsedArgs.file_path || parsedArgs.path || rawArg;
        body = (
          <ReadToolCard
            status={status}
            filePath={String(readPath || '')}
            content={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'Edit': {
        const editPath = parsedArgs.file_path || parsedArgs.path || rawArg;
        body = (
          <EditToolCard
            status={status}
            filePath={String(editPath || '')}
            content={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'Write': {
        const writePath = parsedArgs.file_path || parsedArgs.path || rawArg;
        body = (
          <WriteToolCard
            status={status}
            filePath={String(writePath || '')}
            content={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'Bash': {
        const bashCommand = parsedArgs.command
          ? String(parsedArgs.command)
          : (rawArg || stringifyArgs(toolArgs));
        body = (
          <BashToolCard
            status={status}
            command={bashCommand}
            output={isSuccess ? content : undefined}
            exitCode={exitCode ?? undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'Glob': {
        const globPattern = parsedArgs.pattern || rawArg;
        body = (
          <GlobToolCard
            status={status}
            pattern={String(globPattern || '')}
            content={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'Grep': {
        let grepPattern = parsedArgs.pattern ? String(parsedArgs.pattern) : '';
        let grepPath = parsedArgs.path ? String(parsedArgs.path) : undefined;

        if (!grepPattern && rawArg) {
          const inMatch = rawArg.match(/^(.+?)\s+in\s+(.+)$/);
          if (inMatch) {
            grepPattern = inMatch[1];
            grepPath = inMatch[2];
          } else {
            grepPattern = rawArg;
          }
        }

        body = (
          <GrepToolCard
            status={status}
            pattern={grepPattern}
            path={grepPath}
            content={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      case 'TodoWrite': {
        body = (
          <TodoWriteToolCard
            status={status}
            content={isSuccess ? stringifyArgs(toolArgs) : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      default: {
        const statusStyles = isSuccess
          ? { container: 'bg-emerald-500/10 border-emerald-500/30', text: 'text-emerald-400' }
          : { container: 'bg-red-500/10 border-red-500/30', text: 'text-red-400' };
        const argsStr = stringifyArgs(toolArgs);

        body = (
          <div className={cn('rounded-lg border p-3', statusStyles.container)}>
            <p className={cn('text-xs font-medium mb-2', statusStyles.text)}>
              {toolName}
            </p>
            {argsStr && (
              <pre className="text-xs text-text-muted overflow-x-auto mb-2">{argsStr}</pre>
            )}
            {content && (
              <pre
                className={cn(
                  'text-xs overflow-x-auto max-h-64 overflow-y-auto',
                  isSuccess ? 'text-text-muted' : 'text-red-400'
                )}
              >
                {content}
              </pre>
            )}
            {exitCode !== undefined && exitCode !== null && exitCode !== 0 && (
              <p className="mt-1 text-xs text-red-400">Exit code: {exitCode}</p>
            )}
          </div>
        );
        break;
      }
    }
  }

  return (
    <div className="flex min-w-0 gap-3">
      <div className="w-8 shrink-0" />
      <div className="min-w-0 flex-1">{body}</div>
    </div>
  );
}
