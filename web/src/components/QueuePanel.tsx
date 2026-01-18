import { ArrowUp, ArrowDown, Send, Trash2 } from 'lucide-react';
import { cn } from '../lib/cn';
import type { QueuedMessage } from '../types';

interface QueuePanelProps {
  messages: QueuedMessage[];
  canSend: boolean;
  onSend: (message: QueuedMessage) => void;
  onRemove: (messageId: string) => void;
  onMove: (messageId: string, position: number) => void;
}

function modeLabel(mode: QueuedMessage['mode']): string {
  return mode === 'steer' ? 'Steer' : 'Queued';
}

export function QueuePanel({
  messages,
  canSend,
  onSend,
  onRemove,
  onMove,
}: QueuePanelProps) {
  if (messages.length === 0) return null;

  return (
    <div className="border-t border-border bg-surface px-4 py-2">
      <div className="flex items-center justify-between">
        <div className="text-xs font-medium text-text-muted">
          Queue <span className="text-text">({messages.length})</span>
        </div>
      </div>
      <div className="mt-2 space-y-2">
        {messages.map((message, index) => (
          <div
            key={message.id}
            className="flex items-start gap-2 rounded-lg border border-border/60 bg-surface-elevated px-3 py-2 text-sm"
          >
            <div className="mt-0.5 shrink-0 rounded bg-accent/15 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-accent">
              {modeLabel(message.mode)}
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate text-text">{message.text.trim() || '<empty>'}</div>
              {message.images.length > 0 && (
                <div className="mt-1 text-[11px] text-text-muted">
                  {message.images.length} image{message.images.length === 1 ? '' : 's'}
                </div>
              )}
            </div>
            <div className="flex items-center gap-1">
              <button
                className={cn(
                  'rounded p-1 text-text-muted transition-colors hover:bg-surface hover:text-text',
                  index === 0 && 'cursor-not-allowed opacity-40'
                )}
                disabled={index === 0}
                aria-label="Move up"
                onClick={() => onMove(message.id, index - 1)}
              >
                <ArrowUp className="h-3.5 w-3.5" />
              </button>
              <button
                className={cn(
                  'rounded p-1 text-text-muted transition-colors hover:bg-surface hover:text-text',
                  index === messages.length - 1 && 'cursor-not-allowed opacity-40'
                )}
                disabled={index === messages.length - 1}
                aria-label="Move down"
                onClick={() => onMove(message.id, index + 1)}
              >
                <ArrowDown className="h-3.5 w-3.5" />
              </button>
              <button
                className={cn(
                  'rounded p-1 transition-colors',
                  canSend
                    ? 'text-text-muted hover:bg-surface hover:text-text'
                    : 'cursor-not-allowed opacity-40 text-text-muted'
                )}
                disabled={!canSend}
                aria-label="Send queued message"
                onClick={() => onSend(message)}
              >
                <Send className="h-3.5 w-3.5" />
              </button>
              <button
                className="rounded p-1 text-text-muted transition-colors hover:bg-surface hover:text-red-400"
                aria-label="Remove queued message"
                onClick={() => onRemove(message.id)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
