import type { ReactNode } from 'react';
import { Wrench } from 'lucide-react';
import { cn } from '../lib/cn';
import {
  ReadToolCard,
  EditToolCard,
  WriteToolCard,
  BashToolCard,
  GlobToolCard,
  GrepToolCard,
  TodoWriteToolCard,
  TaskToolCard,
  WebSearchToolCard,
  ToolCard,
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

function extractQuery(args: Record<string, unknown>, rawArg: string): string | undefined {
  const candidates = ['query', 'q', 'search', 'term', 'prompt', 'input'];
  for (const key of candidates) {
    const value = args[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  const raw = rawArg.trim();
  return raw.length > 0 ? raw : undefined;
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
  const normalizedToolName = toolName?.toLowerCase();
  let body: ReactNode = null;

  if (!toolName) {
    body = (
      <ToolCard icon={<Wrench className="h-4 w-4" />} title="Tool output" status={status}>
        {content ? (
          <pre className="p-3 text-xs text-text-muted overflow-x-auto max-h-64 overflow-y-auto">
            {content}
          </pre>
        ) : (
          <div className="p-3 text-xs text-text-muted">No output</div>
        )}
      </ToolCard>
    );
  } else if (normalizedToolName === 'websearch' || normalizedToolName === 'web_search' || normalizedToolName === 'web-search') {
    const query = extractQuery(parsedArgs, rawArg);
    body = (
      <WebSearchToolCard
        status={status}
        query={query}
        content={isSuccess ? content : undefined}
        error={!isSuccess ? content : undefined}
      />
    );
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
      case 'Task': {
        const description = parsedArgs.description ? String(parsedArgs.description) : undefined;
        const prompt = parsedArgs.prompt ? String(parsedArgs.prompt) : undefined;
        body = (
          <TaskToolCard
            status={status}
            description={description}
            prompt={prompt}
            output={isSuccess ? content : undefined}
            error={!isSuccess ? content : undefined}
          />
        );
        break;
      }
      default: {
        const argsStr = stringifyArgs(toolArgs);
        const hasArgs = argsStr.trim().length > 0;
        const hasContent = content.trim().length > 0;

        body = (
          <ToolCard icon={<Wrench className="h-4 w-4" />} title={toolName ?? 'Tool'} status={status}>
            {hasArgs || hasContent ? (
              <div className="space-y-3 p-3 text-xs text-text-muted">
                {hasArgs && (
                  <div>
                    <p className="text-[11px] uppercase tracking-wide text-text-faint">Args</p>
                    <pre className="mt-1 max-h-40 overflow-x-auto whitespace-pre-wrap">{argsStr}</pre>
                  </div>
                )}
                {hasContent && (
                  <div>
                    <p className="text-[11px] uppercase tracking-wide text-text-faint">Output</p>
                    <pre
                      className={cn(
                        'mt-1 max-h-64 overflow-x-auto whitespace-pre-wrap',
                        isSuccess ? 'text-text-muted' : 'text-red-400'
                      )}
                    >
                      {content}
                    </pre>
                  </div>
                )}
                {exitCode !== undefined && exitCode !== null && exitCode !== 0 && (
                  <p className="text-red-400">Exit code: {exitCode}</p>
                )}
              </div>
            ) : (
              <div className="p-3 text-xs text-text-muted">No output</div>
            )}
          </ToolCard>
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
