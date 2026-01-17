import { memo } from 'react';
import { cn } from '../lib/cn';
import {
  Sparkles,
  Brain,
  Wand2,
  CheckCircle2,
  XCircle,
  FileCode2,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import type { AgentEvent } from '../types';
import { MarkdownBody } from './markdown';
import {
  ReadToolCard,
  EditToolCard,
  WriteToolCard,
  BashToolCard,
  GlobToolCard,
  GrepToolCard,
  TodoWriteToolCard,
} from './tools';
import type { ToolStatus } from './tools';
import { TerminalOutput } from './TerminalOutput';

interface ChatMessageProps {
  event: AgentEvent;
}

function formatToolPayload(payload: unknown) {
  if (payload == null) return '';
  return typeof payload === 'string' ? payload : JSON.stringify(payload, null, 2);
}

// Parse tool arguments to extract relevant fields
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
  return {};
}

// Render tool-specific card based on tool name
function renderToolStarted(toolName: string, args: unknown) {
  const parsedArgs = parseToolArgs(args);
  const status: ToolStatus = 'running';

  switch (toolName) {
    case 'Read':
      return (
        <ReadToolCard
          status={status}
          filePath={String(parsedArgs.file_path || parsedArgs.path || '')}
        />
      );
    case 'Edit':
      return (
        <EditToolCard
          status={status}
          filePath={String(parsedArgs.file_path || parsedArgs.path || '')}
        />
      );
    case 'Write':
      return (
        <WriteToolCard
          status={status}
          filePath={String(parsedArgs.file_path || parsedArgs.path || '')}
        />
      );
    case 'Bash':
      return (
        <BashToolCard
          status={status}
          command={String(parsedArgs.command || '')}
        />
      );
    case 'Glob':
      return (
        <GlobToolCard
          status={status}
          pattern={String(parsedArgs.pattern || '')}
        />
      );
    case 'Grep':
      return (
        <GrepToolCard
          status={status}
          pattern={String(parsedArgs.pattern || '')}
          path={parsedArgs.path ? String(parsedArgs.path) : undefined}
        />
      );
    case 'TodoWrite':
      return (
        <TodoWriteToolCard
          status={status}
          content={JSON.stringify(parsedArgs)}
        />
      );
    default:
      // Fallback for unknown tools
      const argsStr = formatToolPayload(args);
      return (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
          <div className="flex items-center gap-2 text-xs font-medium text-amber-400">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            <span>Running tool: {toolName}</span>
          </div>
          {argsStr && (
            <pre className="mt-2 text-xs text-text-muted overflow-x-auto">{argsStr}</pre>
          )}
        </div>
      );
  }
}

export const ChatMessage = memo(function ChatMessage({ event }: ChatMessageProps) {
  switch (event.type) {
    case 'AssistantMessage':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent/10">
            <Sparkles className="h-4 w-4 text-accent" />
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <MarkdownBody content={event.text} />
          </div>
        </div>
      );

    case 'AssistantReasoning':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-purple-500/10">
            <Brain className="h-4 w-4 text-purple-400" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-xs text-text-muted">Thinking...</p>
            <p className="mt-1 whitespace-pre-wrap break-words text-pretty text-sm italic text-text-muted">
              {event.text}
            </p>
          </div>
        </div>
      );

    case 'ToolStarted': {
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-amber-500/10">
            <Wand2 className="h-4 w-4 text-amber-400" />
          </div>
          <div className="min-w-0 flex-1">
            {renderToolStarted(event.tool_name, event.arguments)}
          </div>
        </div>
      );
    }

    case 'ToolCompleted': {
      const statusStyles = event.success
        ? { container: 'bg-emerald-500/10', border: 'border-emerald-500/30', text: 'text-emerald-400' }
        : { container: 'bg-red-500/10', border: 'border-red-500/30', text: 'text-red-400' };

      const output = event.result ?? event.error ?? '';

      return (
        <div className="flex min-w-0 gap-3">
          <div className={cn('flex size-8 shrink-0 items-center justify-center rounded-lg', statusStyles.container)}>
            {event.success ? (
              <CheckCircle2 className="h-4 w-4 text-emerald-400" />
            ) : (
              <XCircle className="h-4 w-4 text-red-400" />
            )}
          </div>
          <div className="min-w-0 flex-1">
            <div className={cn('rounded-lg border p-3', statusStyles.container, statusStyles.border)}>
              <p className={cn('text-xs font-medium', statusStyles.text)}>
                {event.success ? 'Tool completed' : 'Tool failed'}
              </p>
              {output && (
                <pre
                  className={cn(
                    'mt-2 text-xs overflow-x-auto max-h-64 overflow-y-auto',
                    event.success ? 'text-text-muted' : 'text-red-400'
                  )}
                >
                  {output}
                </pre>
              )}
            </div>
          </div>
        </div>
      );
    }

    case 'FileChanged':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-blue-500/10">
            <FileCode2 className="h-4 w-4 text-blue-400" />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            <p className="text-xs text-text-muted">
              <span className="font-medium text-blue-400">{event.operation}</span>{' '}
              <code className="break-all rounded bg-surface-elevated px-1 py-0.5">{event.path}</code>
            </p>
          </div>
        </div>
      );

    case 'CommandOutput': {
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-slate-500/10">
            <FileCode2 className="h-4 w-4 text-slate-400" />
          </div>
          <div className="min-w-0 flex-1">
            <TerminalOutput
              command={event.command}
              output={event.output}
              exitCode={event.exit_code}
            />
          </div>
        </div>
      );
    }

    case 'Error':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-red-500/10">
            <AlertTriangle className="h-4 w-4 text-red-400" />
          </div>
          <div className="min-w-0 flex-1">
            <p className={cn('break-words text-sm', event.is_fatal ? 'text-red-400' : 'text-yellow-400')}>
              {event.message}
            </p>
          </div>
        </div>
      );

    case 'TurnStarted':
      return (
        <div className="flex items-center justify-center gap-2 py-2 text-xs text-text-muted">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          <span>Processing...</span>
        </div>
      );

    case 'TurnCompleted':
      return (
        <div className="flex justify-center py-2">
          <span className="text-xs text-text-muted">
            Tokens: {event.usage.input_tokens} in / {event.usage.output_tokens} out
          </span>
        </div>
      );

    default:
      return null;
  }
});
