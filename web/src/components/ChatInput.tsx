import { useRef, useEffect, useState, type KeyboardEvent } from 'react';
import { Send, Loader2, GitBranch, ListPlus, ImagePlus, X } from 'lucide-react';
import { cn } from '../lib/cn';
import { ModeToggle } from './ModeToggle';

interface ChatInputProps {
  onSend: (message: string) => void;
  onQueue?: (message: string) => void;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  inputDisabled?: boolean;
  sendDisabled?: boolean;
  queueDisabled?: boolean;
  attachments?: { id: string; previewUrl: string; name: string }[];
  onAttachImages?: (files: File[]) => void;
  onRemoveAttachment?: (id: string) => void;
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
  // Mode toggle
  onModeToggle?: () => void;
  canChangeMode?: boolean;
}

export function ChatInput({
  onSend,
  onQueue,
  value,
  onChange,
  disabled = false,
  inputDisabled,
  sendDisabled,
  queueDisabled,
  attachments = [],
  onAttachImages,
  onRemoveAttachment,
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
  onModeToggle,
  canChangeMode = false,
}: ChatInputProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const statusBarRef = useRef<HTMLDivElement>(null);
  const historyIndexRef = useRef<number | null>(null);
  const historyDraftRef = useRef('');
  const [isCompact, setIsCompact] = useState(false);
  const effectiveInputDisabled = inputDisabled ?? disabled;
  const effectiveSendDisabled = sendDisabled ?? disabled;
  const effectiveQueueDisabled = queueDisabled ?? disabled;
  const hasAttachments = attachments.length > 0;

  // Responsive check for status bar - switch to compact mode when space is tight
  useEffect(() => {
    const statusBar = statusBarRef.current;
    if (!statusBar) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        // Switch to compact mode when width is under 400px
        setIsCompact(entry.contentRect.width < 400);
      }
    });

    observer.observe(statusBar);
    return () => observer.disconnect();
  }, []);

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
    if ((trimmed.length > 0 || hasAttachments) && !effectiveSendDisabled) {
      onSend(trimmed);
      historyIndexRef.current = null;
      historyDraftRef.current = '';
    }
  };

  const handleQueue = () => {
    if (!onQueue) return;
    const trimmed = value.trim();
    if (trimmed && !effectiveQueueDisabled) {
      onQueue(trimmed);
      historyIndexRef.current = null;
      historyDraftRef.current = '';
    }
  };

  const handleAttachClick = () => {
    fileInputRef.current?.click();
  };

  const handleFilesSelected = (files: FileList | null) => {
    if (!files || !onAttachImages) return;
    const nextFiles = Array.from(files);
    if (nextFiles.length === 0) return;
    onAttachImages(nextFiles);
    if (fileInputRef.current) {
      fileInputRef.current.value = '';
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
            disabled={effectiveInputDisabled}
            rows={1}
            className={cn(
              'w-full resize-none rounded-lg border border-border bg-surface-elevated px-4 py-3 text-sm text-text placeholder-text-muted',
              'focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent',
            'disabled:cursor-not-allowed disabled:opacity-50'
          )}
          />
        </div>
        {onAttachImages && (
          <>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              multiple
              className="hidden"
              onChange={(e) => handleFilesSelected(e.target.files)}
            />
            <button
              onClick={handleAttachClick}
              disabled={effectiveInputDisabled}
              aria-label="Attach images"
              className={cn(
                'flex size-11 shrink-0 items-center justify-center rounded-lg transition-colors',
                'disabled:cursor-not-allowed disabled:opacity-50',
                'bg-surface-elevated text-text-muted hover:bg-surface hover:text-text'
              )}
            >
              <ImagePlus className="h-5 w-5" />
            </button>
          </>
        )}
        {onQueue && (
          <button
            onClick={handleQueue}
            disabled={effectiveQueueDisabled || !value.trim()}
            aria-label="Queue message"
            className={cn(
              'flex size-11 shrink-0 items-center justify-center rounded-lg transition-colors',
              'disabled:cursor-not-allowed disabled:opacity-50',
              value.trim() && !effectiveQueueDisabled
                ? 'bg-surface-elevated text-text hover:bg-surface'
                : 'bg-surface-elevated text-text-muted'
            )}
          >
            <ListPlus className="h-5 w-5" />
          </button>
        )}
        <button
          onClick={handleSubmit}
          disabled={effectiveSendDisabled || (!value.trim() && !hasAttachments)}
          aria-label={effectiveSendDisabled ? 'Sending...' : 'Send message'}
          className={cn(
            'flex size-11 shrink-0 items-center justify-center rounded-lg transition-colors',
            'disabled:cursor-not-allowed disabled:opacity-50',
            (value.trim() || hasAttachments) && !effectiveSendDisabled
              ? 'bg-accent text-white hover:bg-accent-hover'
              : 'bg-surface-elevated text-text-muted'
          )}
        >
          {effectiveSendDisabled ? (
            <Loader2 className="h-5 w-5 animate-spin" />
          ) : (
            <Send className="h-5 w-5" />
          )}
        </button>
      </div>
      {attachments.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-2">
          {attachments.map((attachment) => (
            <div
              key={attachment.id}
              className="group relative h-16 w-16 overflow-hidden rounded-lg border border-border/60 bg-surface-elevated"
            >
              <img
                src={attachment.previewUrl}
                alt={attachment.name}
                className="h-full w-full object-cover"
              />
              {onRemoveAttachment && (
                <button
                  onClick={() => onRemoveAttachment(attachment.id)}
                  className="absolute right-1 top-1 rounded-full bg-surface/80 p-0.5 text-text-muted opacity-0 transition-opacity group-hover:opacity-100"
                  aria-label="Remove attachment"
                >
                  <X className="h-3 w-3" />
                </button>
              )}
            </div>
          ))}
        </div>
      )}
      <div
        ref={statusBarRef}
        className={cn(
          "mt-2 flex text-xs text-text-muted",
          isCompact ? "flex-col gap-1" : "items-center justify-between"
        )}
      >
        {/* Left: Agent Mode + Model + Agent Type */}
        <div className="flex items-center gap-2 min-w-0">
          {agentMode && onModeToggle && (
            <ModeToggle
              mode={agentMode as 'build' | 'plan'}
              onToggle={onModeToggle}
              disabled={!canChangeMode}
              size="sm"
            />
          )}
          {modelDisplayName ? (
            canChangeModel && onModelClick ? (
              <button
                onClick={onModelClick}
                className="text-text transition-colors hover:text-accent hover:underline shrink-0"
              >
                {modelDisplayName}
              </button>
            ) : (
              <span className="text-text shrink-0">{modelDisplayName}</span>
            )
          ) : (
            canChangeModel && onModelClick && (
              <button
                onClick={onModelClick}
                className="text-text-muted transition-colors hover:text-accent hover:underline shrink-0"
              >
                Select model
              </button>
            )
          )}
          {agentType && (
            <span className="shrink-0">
              {agentType === 'claude'
                ? 'Claude Code'
                : agentType === 'codex'
                  ? 'Codex CLI'
                  : 'Gemini CLI'}
            </span>
          )}
          {!modelDisplayName && !agentType && !canChangeModel && (
            <span className="truncate">Press Enter to send, Shift+Enter for new line</span>
          )}
        </div>

        {/* Right: Git stats + Branch */}
        {(gitStats || branch) && (
          <div className={cn(
            "flex items-center gap-2 min-w-0",
            isCompact && "justify-end"
          )}>
            {gitStats && (gitStats.additions > 0 || gitStats.deletions > 0) && (
              <div className="flex items-center gap-1 shrink-0 tabular-nums">
                <span className="text-diff-add">+{gitStats.additions}</span>
                <span className="text-diff-remove">-{gitStats.deletions}</span>
              </div>
            )}
            {branch && (
              <div
                className="flex items-center gap-1.5 min-w-0 group"
                title={branch}
              >
                <GitBranch className="h-3 w-3 shrink-0 text-text-muted/60" />
                <span
                  className={cn(
                    "font-mono text-[11px] tracking-tight text-text-muted/80",
                    "truncate",
                    isCompact ? "max-w-[120px]" : "max-w-[200px]"
                  )}
                >
                  {branch}
                </span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
