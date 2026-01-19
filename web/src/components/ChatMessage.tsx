import { memo } from 'react';
import { cn } from '../lib/cn';
import {
  Sparkles,
  Brain,
  FileCode2,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import type { AgentEvent } from '../types';
import { MarkdownBody } from './markdown';
import { TerminalOutput } from './TerminalOutput';
import { ToolRunMessage } from './ToolRunMessage';
import { FilePathLink } from './FilePathLink';

interface ChatMessageProps {
  event: AgentEvent;
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
        <ToolRunMessage
          toolName={event.tool_name}
          toolArgs={event.arguments}
          status="running"
        />
      );
    }

    case 'ToolCompleted': {
      const output = event.result ?? event.error ?? '';
      return (
        <ToolRunMessage
          status={event.success ? 'success' : 'error'}
          output={output}
        />
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
              <FilePathLink
                path={event.path}
                className="break-all rounded bg-surface-elevated px-1 py-0.5 text-xs"
              />
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
