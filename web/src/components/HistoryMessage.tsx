import { memo } from 'react';
import { cn } from '../lib/cn';
import { User, Bot, Wrench, AlertCircle, Clock, Coins } from 'lucide-react';
import type { SessionEvent } from '../types';

interface HistoryMessageProps {
  event: SessionEvent;
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
            <p className="whitespace-pre-wrap break-words text-pretty text-sm text-text">{event.content}</p>
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
            <div className="prose prose-sm prose-invert max-w-none">
              <p className="whitespace-pre-wrap break-words text-pretty text-sm text-text">{event.content}</p>
            </div>
          </div>
        </div>
      );

    case 'tool':
      return (
        <div className="flex min-w-0 gap-3">
          <div
            className={cn(
              'flex size-8 shrink-0 items-center justify-center rounded-lg',
              event.exit_code === 0 || event.exit_code === undefined
                ? 'bg-green-500/10'
                : 'bg-red-500/10'
            )}
          >
            <Wrench
              className={cn(
                'h-4 w-4',
                event.exit_code === 0 || event.exit_code === undefined
                  ? 'text-green-400'
                  : 'text-red-400'
              )}
            />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            {event.tool_name && (
              <p className="text-xs font-medium text-yellow-400">{event.tool_name}</p>
            )}
            {event.tool_args && (
              <pre className="mt-1 max-h-20 overflow-auto rounded bg-surface-elevated p-2 text-xs text-text-muted">
                {event.tool_args}
              </pre>
            )}
            {event.content && (
              <pre className="mt-1 max-h-40 overflow-auto rounded bg-surface-elevated p-2 text-xs text-text-muted">
                {event.content}
              </pre>
            )}
            {event.exit_code !== undefined && event.exit_code !== 0 && (
              <p className="mt-1 text-xs text-red-400">Exit code: {event.exit_code}</p>
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
