import { memo } from 'react';
import { User, Bot, AlertCircle, Clock, Coins } from 'lucide-react';
import type { SessionEvent } from '../types';
import { MarkdownBody } from './markdown';
import { ToolRunMessage } from './ToolRunMessage';

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

    case 'reasoning':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-slate-500/10">
            <Bot className="h-4 w-4 text-text-muted" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-xs uppercase tracking-wide text-text-muted">Thinking</p>
            <p className="mt-1 whitespace-pre-wrap break-words text-sm italic text-text-muted">{event.content}</p>
          </div>
        </div>
      );

    case 'tool':
      return (
        <ToolRunMessage
          toolName={event.tool_name}
          toolArgs={event.tool_args}
          status={event.exit_code === undefined || event.exit_code === 0 ? 'success' : 'error'}
          output={event.content}
          exitCode={event.exit_code}
        />
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
