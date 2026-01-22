import { useState, useMemo } from 'react';
import { ChevronRight, Copy, Check, FileText, Wrench, MessageSquare, AlertCircle, Zap, Database, History } from 'lucide-react';
import type { AgentEvent } from '../types';
import { cn } from '../lib/cn';
import { copyText } from '../lib/clipboard';

interface RawEventCardProps {
  event: AgentEvent;
  index: number;
  defaultExpanded?: boolean;
}

interface EventMeta {
  type: string;
  badge: string;
  summary: string;
  icon: React.ReactNode;
  accentClass: string;
  borderClass: string;
  badgeClass: string;
}

function getEventMeta(event: AgentEvent): EventMeta {
  const iconClass = 'h-3.5 w-3.5';

  // Handle Raw events with history_load or debug status
  if (event.type === 'Raw') {
    const data = event.data as Record<string, unknown>;

    // History load summary event
    if (data?.type === 'history_load') {
      const total = data.total_entries ?? 0;
      const included = data.included ?? 0;
      const skipped = data.skipped ?? 0;
      return {
        type: 'history_load',
        badge: 'HISTORY',
        summary: `${total} entries • ${included} included • ${skipped} skipped`,
        icon: <History className={iconClass} />,
        accentClass: 'text-accent',
        borderClass: 'border-accent/30',
        badgeClass: 'bg-accent/15 text-accent',
      };
    }

    // Debug entries with status (INCLUDE, SKIP, ERROR)
    const typeStr = typeof data?.type === 'string' ? data.type : '';
    const status = typeof data?.status === 'string' ? data.status : null;
    const entryType = typeof data?.entry_type === 'string' ? data.entry_type : '';
    const line = typeof data?.line === 'number' ? data.line : null;
    const reason = typeof data?.reason === 'string' ? data.reason : '';

    if (status === 'INCLUDE') {
      return {
        type: 'debug_include',
        badge: 'INCLUDE',
        summary: `L${line} ${entryType}${reason ? ` • ${reason}` : ''}`,
        icon: <Check className={iconClass} />,
        accentClass: 'text-success',
        borderClass: 'border-success/30',
        badgeClass: 'bg-success/15 text-success',
      };
    }

    if (status === 'SKIP') {
      return {
        type: 'debug_skip',
        badge: 'SKIP',
        summary: `L${line} ${entryType}${reason ? ` • ${reason}` : ''}`,
        icon: <FileText className={iconClass} />,
        accentClass: 'text-warning',
        borderClass: 'border-warning/30',
        badgeClass: 'bg-warning/15 text-warning',
      };
    }

    if (status === 'ERROR') {
      return {
        type: 'debug_error',
        badge: 'ERROR',
        summary: `L${line} ${entryType}${reason ? ` • ${reason}` : ''}`,
        icon: <AlertCircle className={iconClass} />,
        accentClass: 'text-error',
        borderClass: 'border-error/30',
        badgeClass: 'bg-error/15 text-error',
      };
    }

    // Generic Raw event
    return {
      type: 'raw',
      badge: 'RAW',
      summary: typeStr || 'Unknown data',
      icon: <Database className={iconClass} />,
      accentClass: 'text-text-muted',
      borderClass: 'border-border',
      badgeClass: 'bg-surface-elevated text-text-muted',
    };
  }

  // Tool events
  if (event.type === 'ToolStarted') {
    return {
      type: 'tool_started',
      badge: event.tool_name,
      summary: `Tool started`,
      icon: <Wrench className={iconClass} />,
      accentClass: 'text-agent-claude',
      borderClass: 'border-agent-claude/30',
      badgeClass: 'bg-agent-claude/15 text-agent-claude',
    };
  }

  if (event.type === 'ToolCompleted') {
    const success = event.success;
    return {
      type: 'tool_completed',
      badge: success ? 'SUCCESS' : 'FAILED',
      summary: success ? 'Tool completed successfully' : (event.error || 'Tool failed'),
      icon: success ? <Check className={iconClass} /> : <AlertCircle className={iconClass} />,
      accentClass: success ? 'text-success' : 'text-error',
      borderClass: success ? 'border-success/30' : 'border-error/30',
      badgeClass: success ? 'bg-success/15 text-success' : 'bg-error/15 text-error',
    };
  }

  // Assistant messages
  if (event.type === 'AssistantMessage') {
    const preview = event.text.slice(0, 60) + (event.text.length > 60 ? '...' : '');
    return {
      type: 'assistant_message',
      badge: event.is_final ? 'FINAL' : 'STREAMING',
      summary: preview,
      icon: <MessageSquare className={iconClass} />,
      accentClass: 'text-agent-codex',
      borderClass: 'border-agent-codex/30',
      badgeClass: 'bg-agent-codex/15 text-agent-codex',
    };
  }

  // Token usage
  if (event.type === 'TokenUsage') {
    const usage = event.usage;
    const percent = event.usage_percent ? ` (${Math.round(event.usage_percent)}%)` : '';
    return {
      type: 'token_usage',
      badge: 'TOKENS',
      summary: `in: ${usage.input_tokens} • out: ${usage.output_tokens} • cached: ${usage.cached_tokens}${percent}`,
      icon: <Zap className={iconClass} />,
      accentClass: 'text-warning',
      borderClass: 'border-warning/20',
      badgeClass: 'bg-warning/10 text-warning',
    };
  }

  // Errors
  if (event.type === 'Error') {
    return {
      type: 'error',
      badge: event.is_fatal ? 'FATAL' : 'ERROR',
      summary: event.message,
      icon: <AlertCircle className={iconClass} />,
      accentClass: 'text-error',
      borderClass: 'border-error/30',
      badgeClass: 'bg-error/15 text-error',
    };
  }

  // Turn events
  if (event.type === 'TurnStarted') {
    return {
      type: 'turn_started',
      badge: 'TURN',
      summary: 'Turn started',
      icon: <Zap className={iconClass} />,
      accentClass: 'text-accent',
      borderClass: 'border-accent/20',
      badgeClass: 'bg-accent/10 text-accent',
    };
  }

  if (event.type === 'TurnCompleted') {
    const usage = event.usage;
    return {
      type: 'turn_completed',
      badge: 'DONE',
      summary: `Turn completed • ${usage.total_tokens} tokens`,
      icon: <Check className={iconClass} />,
      accentClass: 'text-success',
      borderClass: 'border-success/20',
      badgeClass: 'bg-success/10 text-success',
    };
  }

  if (event.type === 'TurnFailed') {
    return {
      type: 'turn_failed',
      badge: 'FAILED',
      summary: event.error,
      icon: <AlertCircle className={iconClass} />,
      accentClass: 'text-error',
      borderClass: 'border-error/30',
      badgeClass: 'bg-error/15 text-error',
    };
  }

  // Default for other types
  return {
    type: event.type.toLowerCase(),
    badge: event.type,
    summary: JSON.stringify(event).slice(0, 60),
    icon: <FileText className={iconClass} />,
    accentClass: 'text-text-muted',
    borderClass: 'border-border',
    badgeClass: 'bg-surface-elevated text-text-muted',
  };
}

function JsonValue({ value, depth = 0 }: { value: unknown; depth?: number }) {
  const [collapsed, setCollapsed] = useState(depth > 2);

  if (value === null) {
    return <span className="text-text-muted">null</span>;
  }

  if (typeof value === 'boolean') {
    return <span className="text-warning">{value ? 'true' : 'false'}</span>;
  }

  if (typeof value === 'number') {
    return <span className="text-accent">{value}</span>;
  }

  if (typeof value === 'string') {
    // Truncate very long strings
    if (value.length > 200 && depth > 0) {
      return (
        <span className="text-success">
          "{value.slice(0, 200)}
          <span className="text-text-muted">... ({value.length} chars)</span>"
        </span>
      );
    }
    return <span className="text-success">"{value}"</span>;
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return <span className="text-text-muted">[]</span>;
    }

    if (collapsed) {
      return (
        <span
          className="cursor-pointer text-text-muted hover:text-text"
          onClick={() => setCollapsed(false)}
        >
          [{value.length} items...]
        </span>
      );
    }

    return (
      <span>
        <span
          className="cursor-pointer text-text-muted hover:text-text"
          onClick={() => setCollapsed(true)}
        >
          [
        </span>
        <div className="ml-4">
          {value.map((item, i) => (
            <div key={i}>
              <JsonValue value={item} depth={depth + 1} />
              {i < value.length - 1 && <span className="text-text-muted">,</span>}
            </div>
          ))}
        </div>
        <span className="text-text-muted">]</span>
      </span>
    );
  }

  if (typeof value === 'object') {
    const entries = Object.entries(value);
    if (entries.length === 0) {
      return <span className="text-text-muted">{'{}'}</span>;
    }

    if (collapsed) {
      return (
        <span
          className="cursor-pointer text-text-muted hover:text-text"
          onClick={() => setCollapsed(false)}
        >
          {'{'}{entries.length} keys...{'}'}
        </span>
      );
    }

    return (
      <span>
        <span
          className="cursor-pointer text-text-muted hover:text-text"
          onClick={() => setCollapsed(true)}
        >
          {'{'}
        </span>
        <div className="ml-4">
          {entries.map(([key, val], i) => (
            <div key={key}>
              <span className="text-agent-claude">"{key}"</span>
              <span className="text-text-muted">: </span>
              <JsonValue value={val} depth={depth + 1} />
              {i < entries.length - 1 && <span className="text-text-muted">,</span>}
            </div>
          ))}
        </div>
        <span className="text-text-muted">{'}'}</span>
      </span>
    );
  }

  return <span className="text-text-muted">{String(value)}</span>;
}

export function RawEventCard({ event, index, defaultExpanded = false }: RawEventCardProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);
  const [copied, setCopied] = useState(false);

  const meta = useMemo(() => getEventMeta(event), [event]);
  const jsonString = useMemo(() => JSON.stringify(event, null, 2), [event]);

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await copyText(jsonString);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  };

  return (
    <div
      className={cn(
        'rounded-lg border overflow-hidden',
        'bg-surface transition-all duration-150',
        meta.borderClass
      )}
    >
      {/* Header - always visible */}
      <div
        role="button"
        tabIndex={0}
        onClick={() => setIsExpanded(!isExpanded)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            setIsExpanded((prev) => !prev);
          }
        }}
        className={cn(
          'flex w-full items-center gap-2 px-3 py-2 text-left',
          'hover:bg-surface-elevated/50 transition-colors duration-100',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60'
        )}
      >
        <ChevronRight
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-text-muted transition-transform duration-150',
            isExpanded && 'rotate-90'
          )}
        />

        {/* Icon */}
        <span className={cn('shrink-0', meta.accentClass)}>{meta.icon}</span>

        {/* Badge */}
        <span
          className={cn(
            'shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide',
            meta.badgeClass
          )}
        >
          {meta.badge}
        </span>

        {/* Summary */}
        <span className="min-w-0 flex-1 truncate text-xs text-text-secondary">
          {meta.summary}
        </span>

        {/* Index */}
        <span className="shrink-0 text-[10px] tabular-nums text-text-muted">
          #{index}
        </span>

        {/* Copy button */}
        <button
          onClick={handleCopy}
          className={cn(
            'shrink-0 rounded p-1 transition-colors',
            'text-text-muted hover:text-text hover:bg-surface-elevated'
          )}
          title={copied ? 'Copied!' : 'Copy JSON'}
        >
          {copied ? (
            <Check className="h-3 w-3 text-success" />
          ) : (
            <Copy className="h-3 w-3" />
          )}
        </button>
      </div>

      {/* Expanded content */}
      {isExpanded && (
        <div className="border-t border-border bg-background p-3">
          <pre className="overflow-x-auto font-mono text-xs leading-relaxed">
            <JsonValue value={event} />
          </pre>
        </div>
      )}
    </div>
  );
}
