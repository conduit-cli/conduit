import { memo } from 'react';
import { cn } from '../lib/cn';
import { Bot, Wrench, FileCode, Terminal, AlertCircle } from 'lucide-react';
import type { AgentEvent } from '../types';

interface ChatMessageProps {
  event: AgentEvent;
}

export const ChatMessage = memo(function ChatMessage({ event }: ChatMessageProps) {
  switch (event.type) {
    case 'AssistantMessage':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent/10">
            <Bot className="h-4 w-4 text-accent" />
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
            <Bot className="h-4 w-4 text-purple-400" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-xs text-text-muted">Thinking...</p>
            <p className="mt-1 whitespace-pre-wrap break-words text-pretty text-sm italic text-text-muted">{event.text}</p>
          </div>
        </div>
      );

    case 'ToolStarted':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-yellow-500/10">
            <Wrench className="h-4 w-4 text-yellow-400" />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            <p className="text-xs font-medium text-yellow-400">Using tool: {event.tool_name}</p>
            {event.arguments != null && (
              <pre className="mt-1 max-h-20 overflow-auto rounded bg-surface-elevated p-2 text-xs text-text-muted">
                {typeof event.arguments === 'string'
                  ? event.arguments
                  : JSON.stringify(event.arguments, null, 2)}
              </pre>
            )}
          </div>
        </div>
      );

    case 'ToolCompleted':
      return (
        <div className="flex min-w-0 gap-3">
          <div
            className={cn(
              'flex size-8 shrink-0 items-center justify-center rounded-lg',
              event.success ? 'bg-green-500/10' : 'bg-red-500/10'
            )}
          >
            <Wrench className={cn('h-4 w-4', event.success ? 'text-green-400' : 'text-red-400')} />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            <p
              className={cn(
                'text-xs font-medium',
                event.success ? 'text-green-400' : 'text-red-400'
              )}
            >
              {event.success ? 'Tool completed' : 'Tool failed'}
            </p>
            {event.result && (
              <pre className="mt-1 max-h-40 overflow-auto rounded bg-surface-elevated p-2 text-xs text-text-muted">
                {event.result}
              </pre>
            )}
            {event.error && <p className="mt-1 break-words text-xs text-red-400">{event.error}</p>}
          </div>
        </div>
      );

    case 'FileChanged':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-blue-500/10">
            <FileCode className="h-4 w-4 text-blue-400" />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            <p className="text-xs text-text-muted">
              <span className="font-medium text-blue-400">{event.operation}</span>{' '}
              <code className="break-all rounded bg-surface-elevated px-1 py-0.5">{event.path}</code>
            </p>
          </div>
        </div>
      );

    case 'CommandOutput':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-gray-500/10">
            <Terminal className="h-4 w-4 text-gray-400" />
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">
            <pre className="overflow-auto rounded bg-surface-elevated p-2 text-xs text-text-muted">
              {event.command}
            </pre>
            {event.output && (
              <pre className="mt-1 max-h-40 overflow-auto rounded bg-black/50 p-2 font-mono text-xs text-green-400">
                {event.output}
              </pre>
            )}
            {event.exit_code !== null && event.exit_code !== 0 && (
              <p className="mt-1 text-xs text-red-400">Exit code: {event.exit_code}</p>
            )}
          </div>
        </div>
      );

    case 'Error':
      return (
        <div className="flex min-w-0 gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-red-500/10">
            <AlertCircle className="h-4 w-4 text-red-400" />
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
        <div className="flex justify-center py-2">
          <span className="text-xs text-text-muted">Processing...</span>
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
