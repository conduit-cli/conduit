import { memo } from 'react';
import { cn } from '../lib/cn';
import {
  Sparkles,
  Brain,
  Wand2,
  CheckCircle2,
  XCircle,
  FileCode2,
  TerminalSquare,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import type { AgentEvent } from '../types';

interface ChatMessageProps {
  event: AgentEvent;
}

function formatToolPayload(payload: unknown) {
  if (payload == null) return '';
  return typeof payload === 'string' ? payload : JSON.stringify(payload, null, 2);
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
            <div className="prose prose-sm prose-invert max-w-none">
              <p className="whitespace-pre-wrap break-words text-pretty text-sm text-text">{event.text}</p>
            </div>
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
            <p className="mt-1 whitespace-pre-wrap break-words text-pretty text-sm italic text-text-muted">{event.text}</p>
          </div>
        </div>
      );

    case 'ToolStarted': {
      const args = formatToolPayload(event.arguments);
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-amber-500/10">
            <Wand2 className="h-4 w-4 text-amber-400" />
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex items-center gap-2 text-xs font-medium text-amber-400">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              <span>Running tool: {event.tool_name}</span>
            </div>
            {args && (
              <details className="rounded-lg border border-border bg-surface-elevated">
                <summary className="cursor-pointer px-2 py-1 text-xs text-text-muted">View arguments</summary>
                <pre className="border-t border-border p-2 text-xs text-text-muted">{args}</pre>
              </details>
            )}
          </div>
        </div>
      );
    }

    case 'ToolCompleted': {
      const output = event.result ? event.result : event.error ?? '';
      const statusStyles = event.success
        ? { container: 'bg-emerald-500/10', text: 'text-emerald-400' }
        : { container: 'bg-red-500/10', text: 'text-red-400' };

      return (
        <div className="flex min-w-0 gap-3">
          <div className={cn('flex size-8 shrink-0 items-center justify-center rounded-lg', statusStyles.container)}>
            {event.success ? (
              <CheckCircle2 className={cn('h-4 w-4', statusStyles.text)} />
            ) : (
              <XCircle className={cn('h-4 w-4', statusStyles.text)} />
            )}
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <p className={cn('text-xs font-medium', statusStyles.text)}>
              {event.success ? 'Tool completed' : 'Tool failed'}
            </p>
            {output && (
              <details className="rounded-lg border border-border bg-surface-elevated">
                <summary className="cursor-pointer px-2 py-1 text-xs text-text-muted">
                  {event.success ? 'View output' : 'View error'}
                </summary>
                <pre className={cn('border-t border-border p-2 text-xs', event.success ? 'text-text-muted' : 'text-red-400')}>
                  {output}
                </pre>
              </details>
            )}
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
      const exitCode = event.exit_code;
      const exitLabel = exitCode === null ? null : `Exit ${exitCode}`;
      const exitClass = exitCode === 0 ? 'text-emerald-400' : 'text-red-400';

      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-slate-500/10">
            <TerminalSquare className="h-4 w-4 text-slate-400" />
          </div>
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex items-center gap-2 text-xs font-medium text-slate-300">
              <span>Command output</span>
              {exitLabel && <span className={cn('font-semibold', exitClass)}>{exitLabel}</span>}
            </div>
            <details className="rounded-lg border border-border bg-surface-elevated">
              <summary className="cursor-pointer px-2 py-1 text-xs text-text-muted">View details</summary>
              <div className="space-y-2 border-t border-border p-2">
                <pre className="overflow-auto rounded bg-surface px-2 py-1 text-xs text-text-muted">
                  {event.command}
                </pre>
                {event.output && (
                  <pre className="max-h-48 overflow-auto rounded bg-black/50 p-2 font-mono text-xs text-green-400">
                    {event.output}
                  </pre>
                )}
              </div>
            </details>
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
