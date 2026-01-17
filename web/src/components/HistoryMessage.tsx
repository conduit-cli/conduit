import { memo } from 'react';
import { cn } from '../lib/cn';
import { User, Bot, AlertCircle, Clock, Coins } from 'lucide-react';
import type { SessionEvent } from '../types';
import { MarkdownBody } from './markdown';
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

interface HistoryMessageProps {
  event: SessionEvent;
}

// Parse tool arguments - may be JSON or plain string depending on tool
function parseToolArgs(toolArgs: string | undefined): Record<string, unknown> {
  if (!toolArgs) return {};

  // First try JSON parsing
  try {
    return JSON.parse(toolArgs);
  } catch {
    // For certain tools, tool_args is a plain string with specific meaning
    // Return it in a structured way based on tool type
    return { raw: toolArgs };
  }
}

// Render tool result based on tool name
function renderToolResult(
  toolName: string | undefined,
  toolArgs: string | undefined,
  content: string | undefined,
  exitCode: number | undefined
) {
  if (!toolName) {
    return content ? (
      <pre className="rounded bg-surface-elevated p-2 text-xs text-text-muted overflow-x-auto max-h-64 overflow-y-auto">
        {content}
      </pre>
    ) : null;
  }

  const parsedArgs = parseToolArgs(toolArgs);
  const isSuccess = exitCode === undefined || exitCode === 0;
  const status: ToolStatus = isSuccess ? 'success' : 'error';

  // For tools where tool_args is a plain string (file path, command, etc.)
  const rawArg = parsedArgs.raw ? String(parsedArgs.raw) : '';

  switch (toolName) {
    case 'Read':
      // tool_args is the file path directly
      const readPath = parsedArgs.file_path || parsedArgs.path || rawArg;
      return (
        <ReadToolCard
          status={status}
          filePath={String(readPath)}
          content={isSuccess ? content : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    case 'Edit':
      const editPath = parsedArgs.file_path || parsedArgs.path || rawArg;
      return (
        <EditToolCard
          status={status}
          filePath={String(editPath)}
          content={isSuccess ? content : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    case 'Write':
      const writePath = parsedArgs.file_path || parsedArgs.path || rawArg;
      return (
        <WriteToolCard
          status={status}
          filePath={String(writePath)}
          content={isSuccess ? content : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    case 'Bash':
      // For Bash, tool_args is often the command itself as a string, not JSON
      const bashCommand = parsedArgs.command
        ? String(parsedArgs.command)
        : (parsedArgs.raw ? String(parsedArgs.raw) : (toolArgs || ''));
      return (
        <BashToolCard
          status={status}
          command={bashCommand}
          output={isSuccess ? content : undefined}
          exitCode={exitCode}
          error={!isSuccess && exitCode !== undefined ? content : undefined}
        />
      );
    case 'Glob':
      // tool_args may be the pattern directly
      const globPattern = parsedArgs.pattern || rawArg;
      return (
        <GlobToolCard
          status={status}
          pattern={String(globPattern)}
          content={isSuccess ? content : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    case 'Grep':
      // tool_args format: "pattern in /path" or just the pattern
      let grepPattern = parsedArgs.pattern ? String(parsedArgs.pattern) : '';
      let grepPath = parsedArgs.path ? String(parsedArgs.path) : undefined;

      if (!grepPattern && rawArg) {
        // Parse "pattern in /path" format
        const inMatch = rawArg.match(/^(.+?)\s+in\s+(.+)$/);
        if (inMatch) {
          grepPattern = inMatch[1];
          grepPath = inMatch[2];
        } else {
          grepPattern = rawArg;
        }
      }

      return (
        <GrepToolCard
          status={status}
          pattern={grepPattern}
          path={grepPath}
          content={isSuccess ? content : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    case 'TodoWrite':
      return (
        <TodoWriteToolCard
          status={status}
          content={isSuccess ? toolArgs : undefined}
          error={!isSuccess ? content : undefined}
        />
      );
    default:
      // Fallback for unknown tools
      const statusStyles = isSuccess
        ? { container: 'bg-emerald-500/10 border-emerald-500/30', text: 'text-emerald-400' }
        : { container: 'bg-red-500/10 border-red-500/30', text: 'text-red-400' };

      return (
        <div className={cn('rounded-lg border p-3', statusStyles.container)}>
          <p className={cn('text-xs font-medium mb-2', statusStyles.text)}>
            {toolName}
          </p>
          {toolArgs && (
            <pre className="text-xs text-text-muted overflow-x-auto mb-2">{toolArgs}</pre>
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
          {exitCode !== undefined && exitCode !== 0 && (
            <p className="mt-1 text-xs text-red-400">Exit code: {exitCode}</p>
          )}
        </div>
      );
  }
}

export const HistoryMessage = memo(function HistoryMessage({ event }: HistoryMessageProps) {
  switch (event.role) {
    case 'user':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-blue-500/10">
            <User className="h-4 w-4 text-blue-400" />
          </div>
          <div className="min-w-0 flex-1">
            <MarkdownBody content={event.content} />
          </div>
        </div>
      );

    case 'assistant':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent/10">
            <Bot className="h-4 w-4 text-accent" />
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <MarkdownBody content={event.content} />
          </div>
        </div>
      );

    case 'tool':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="w-8 shrink-0" /> {/* Spacer to align with other messages */}
          <div className="min-w-0 flex-1">
            {renderToolResult(
              event.tool_name,
              event.tool_args,
              event.content,
              event.exit_code
            )}
          </div>
        </div>
      );

    case 'error':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-red-500/10">
            <AlertCircle className="h-4 w-4 text-red-400" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="break-words text-sm text-red-400">{event.content}</p>
          </div>
        </div>
      );

    case 'summary':
      if (!event.summary) return null;
      return (
        <div className="flex justify-center py-2">
          <div className="flex items-center gap-4 text-xs text-text-muted">
            {event.summary.duration_secs > 0 && (
              <span className="flex items-center gap-1">
                <Clock className="h-3 w-3" />
                {event.summary.duration_secs}s
              </span>
            )}
            {(event.summary.input_tokens > 0 || event.summary.output_tokens > 0) && (
              <span className="flex items-center gap-1">
                <Coins className="h-3 w-3" />
                {event.summary.input_tokens} in / {event.summary.output_tokens} out
              </span>
            )}
          </div>
        </div>
      );

    case 'system':
      return (
        <div className="flex justify-center py-2">
          <span className="text-xs italic text-text-muted">{event.content}</span>
        </div>
      );

    default:
      return null;
  }
});
