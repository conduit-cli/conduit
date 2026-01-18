import { useRef, useEffect, type KeyboardEvent } from 'react';
import { Send, Loader2 } from 'lucide-react';
import { cn } from '../lib/cn';

interface ChatInputProps {
  onSend: (message: string) => void;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  focusKey?: string | null;
  history?: string[];
  // Session/workspace info for status line
  modelDisplayName?: string | null;
  agentType?: 'claude' | 'codex' | 'gemini' | null;
  agentMode?: string | null;
  gitStats?: { additions: number; deletions: number } | null;
  branch?: string | null;
  // Model selection
  onModelClick?: () => void;
  canChangeModel?: boolean;
}

// Format branch name with ellipsis for long paths
function formatBranch(branch: string): string {
  if (branch.includes('/')) {
    return '…/' + branch.split('/').pop();
  }
  return branch;
}

export function ChatInput({
  onSend,
  value,
  onChange,
  disabled = false,
  placeholder = 'Type a message...',
  focusKey,
  history = [],
  modelDisplayName,
  agentType,
  agentMode,
  gitStats,
  branch,
  onModelClick,
  canChangeModel = false,
}: ChatInputProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const historyIndexRef = useRef<number | null>(null);
  const historyDraftRef = useRef('');

  useEffect(() => {
    if (!textareaRef.current) return;
    textareaRef.current.focus();
  }, [focusKey]);

  useEffect(() => {
    historyIndexRef.current = null;
    historyDraftRef.current = '';
  }, [focusKey]);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = 'auto';
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [value]);

  const handleSubmit = () => {
    const trimmed = value.trim();
    if (trimmed && !disabled) {
      onSend(trimmed);
      historyIndexRef.current = null;
      historyDraftRef.current = '';
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'ArrowUp') {
      const textarea = textareaRef.current;
      if (!textarea) return;
      if (history.length === 0) return;
      if (e.shiftKey) return;
      const selectionStart = textarea.selectionStart ?? 0;
      const selectionEnd = textarea.selectionEnd ?? 0;
      if (selectionStart !== selectionEnd) return;
      const isEmpty = value.length === 0;
      const atStart = selectionStart === 0;
      if (!isEmpty && !atStart) return;
      e.preventDefault();
      if (historyIndexRef.current === null) {
        historyDraftRef.current = value;
        historyIndexRef.current = history.length - 1;
      } else if (historyIndexRef.current > 0) {
        historyIndexRef.current -= 1;
      }
      const nextValue = history[historyIndexRef.current] ?? '';
      onChange(nextValue);
      requestAnimationFrame(() => {
        if (textareaRef.current) {
          const len = nextValue.length;
          textareaRef.current.setSelectionRange(len, len);
        }
      });
      return;
    }

    if (e.key === 'ArrowDown') {
      if (historyIndexRef.current === null) return;
      e.preventDefault();
      if (historyIndexRef.current < history.length - 1) {
        historyIndexRef.current += 1;
        const nextValue = history[historyIndexRef.current] ?? '';
        onChange(nextValue);
        requestAnimationFrame(() => {
          if (textareaRef.current) {
            const len = nextValue.length;
            textareaRef.current.setSelectionRange(len, len);
          }
        });
      } else {
        historyIndexRef.current = null;
        const draftValue = historyDraftRef.current;
        historyDraftRef.current = '';
        onChange(draftValue);
        requestAnimationFrame(() => {
          if (textareaRef.current) {
            const len = draftValue.length;
            textareaRef.current.setSelectionRange(len, len);
          }
        });
      }
      return;
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="border-t border-border bg-surface p-4">
      <div className="flex items-end gap-3">
        <div className="relative flex-1">
          <textarea
            ref={textareaRef}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            disabled={disabled}
            rows={1}
            className={cn(
              'w-full resize-none rounded-lg border border-border bg-surface-elevated px-4 py-3 text-sm text-text placeholder-text-muted',
              'focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent',
              'disabled:cursor-not-allowed disabled:opacity-50'
            )}
          />
        </div>
        <button
          onClick={handleSubmit}
          disabled={disabled || !value.trim()}
          aria-label={disabled ? 'Sending...' : 'Send message'}
          className={cn(
            'flex size-11 shrink-0 items-center justify-center rounded-lg transition-colors',
            'disabled:cursor-not-allowed disabled:opacity-50',
            value.trim() && !disabled
              ? 'bg-accent text-white hover:bg-accent-hover'
              : 'bg-surface-elevated text-text-muted'
          )}
        >
          {disabled ? (
            <Loader2 className="h-5 w-5 animate-spin" />
          ) : (
            <Send className="h-5 w-5" />
          )}
        </button>
      </div>
      <div className="mt-2 flex items-center justify-between text-xs text-text-muted">
        {/* Left: Agent Mode + Model + Agent Type */}
        <div className="flex items-center gap-2">
          {agentMode && <span className="text-accent">{agentMode}</span>}
          {modelDisplayName && (
            canChangeModel && onModelClick ? (
              <button
                onClick={onModelClick}
                className="text-text transition-colors hover:text-accent hover:underline"
              >
                {modelDisplayName}
              </button>
            ) : (
              <span className="text-text">{modelDisplayName}</span>
            )
          )}
          {agentType && (
            <span>
              {agentType === 'claude'
                ? 'Claude Code'
                : agentType === 'codex'
                  ? 'Codex CLI'
                  : 'Gemini CLI'}
            </span>
          )}
          {!modelDisplayName && !agentType && <span>Press Enter to send, Shift+Enter for new line</span>}
        </div>

        {/* Right: Git stats + Branch */}
        <div className="flex items-center gap-1.5">
          {gitStats && (gitStats.additions > 0 || gitStats.deletions > 0) && (
            <>
              <span className="text-green-400">+{gitStats.additions}</span>
              <span className="text-red-400">-{gitStats.deletions}</span>
              <span>·</span>
            </>
          )}
          {branch && <span className="max-w-48 truncate">{formatBranch(branch)}</span>}
          {!gitStats && !branch && <span>Powered by Claude</span>}
        </div>
      </div>
    </div>
  );
}
