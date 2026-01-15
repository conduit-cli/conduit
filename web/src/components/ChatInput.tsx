import { useState, useRef, useEffect, type KeyboardEvent } from 'react';
import { Send, Loader2 } from 'lucide-react';
import { cn } from '../lib/cn';

interface ChatInputProps {
  onSend: (message: string) => void;
  disabled?: boolean;
  placeholder?: string;
  // Session/workspace info for status line
  modelDisplayName?: string | null;
  agentType?: 'claude' | 'codex' | 'gemini' | null;
  agentMode?: string | null;
  gitStats?: { additions: number; deletions: number } | null;
  branch?: string | null;
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
  disabled = false,
  placeholder = 'Type a message...',
  modelDisplayName,
  agentType,
  agentMode,
  gitStats,
  branch,
}: ChatInputProps) {
  const [message, setMessage] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = 'auto';
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [message]);

  const handleSubmit = () => {
    const trimmed = message.trim();
    if (trimmed && !disabled) {
      onSend(trimmed);
      setMessage('');
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
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
            value={message}
            onChange={(e) => setMessage(e.target.value)}
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
          disabled={disabled || !message.trim()}
          aria-label={disabled ? 'Sending...' : 'Send message'}
          className={cn(
            'flex size-11 shrink-0 items-center justify-center rounded-lg transition-colors',
            'disabled:cursor-not-allowed disabled:opacity-50',
            message.trim() && !disabled
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
          {modelDisplayName && <span className="text-text">{modelDisplayName}</span>}
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
