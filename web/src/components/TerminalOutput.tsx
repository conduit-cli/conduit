import { useState, useRef, useEffect, useMemo } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '../lib/cn';
import { CopyButton } from './ui/CopyButton';

interface TerminalOutputProps {
  command: string;
  output?: string;
  exitCode?: number | null;
  maxHeight?: number;
  className?: string;
}

const DEFAULT_MAX_HEIGHT = 300;
const COLLAPSE_THRESHOLD = 15; // lines

// Strip ANSI escape codes from terminal output
function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g, '');
}

export function TerminalOutput({
  command,
  output,
  exitCode,
  maxHeight = DEFAULT_MAX_HEIGHT,
  className,
}: TerminalOutputProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [shouldCollapse, setShouldCollapse] = useState(false);
  const outputRef = useRef<HTMLPreElement>(null);

  // Clean output by stripping ANSI codes
  const cleanOutput = useMemo(() => output ? stripAnsi(output) : undefined, [output]);
  const lineCount = cleanOutput?.split('\n').length ?? 0;

  useEffect(() => {
    // Only use line count for collapse logic - scrollHeight can give false positives
    setShouldCollapse(lineCount > COLLAPSE_THRESHOLD);
  }, [lineCount]);

  const exitCodeDisplay = exitCode !== null && exitCode !== undefined;
  const isSuccess = exitCode === 0;

  return (
    <div className={cn('rounded-lg overflow-hidden border border-border', className)}>
      {/* Terminal header with command */}
      <div className="flex items-center justify-between bg-terminal-bg px-3 py-2 border-b border-border">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          {command ? (
            <>
              <span className="text-terminal-prompt font-mono text-sm shrink-0 self-start">$</span>
              <pre className="text-sm font-mono text-text-bright whitespace-pre-wrap break-all m-0">
                {command.replace(/\\n/g, '\n')}
              </pre>
            </>
          ) : (
            <span className="text-xs text-text-muted">Command output</span>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0 self-start">
          {exitCodeDisplay && (
            <span
              className={cn(
                'text-xs font-medium px-1.5 py-0.5 rounded',
                isSuccess
                  ? 'bg-success/20 text-success'
                  : 'bg-error/20 text-error'
              )}
            >
              {isSuccess ? 'OK' : `Exit ${exitCode}`}
            </span>
          )}
          {command && <CopyButton text={command} />}
        </div>
      </div>

      {/* Output area */}
      {cleanOutput && (
        <div className="bg-terminal-bg">
          <pre
            ref={outputRef}
            className={cn(
              'p-3 font-mono text-xs text-terminal-text overflow-auto',
              !isExpanded && shouldCollapse && 'max-h-[300px]'
            )}
            style={!isExpanded && shouldCollapse ? { maxHeight } : undefined}
          >
            {cleanOutput}
          </pre>

          {/* Expand/collapse button */}
          {shouldCollapse && (
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className={cn(
                'flex w-full items-center justify-center gap-1 py-1.5',
                'bg-surface-elevated/30 border-t border-border/50',
                'text-xs text-text-muted hover:text-text',
                'transition-colors duration-150'
              )}
            >
              {isExpanded ? (
                <>
                  <ChevronUp className="h-3.5 w-3.5" />
                  <span>Collapse</span>
                </>
              ) : (
                <>
                  <ChevronDown className="h-3.5 w-3.5" />
                  <span>Expand ({lineCount} lines)</span>
                </>
              )}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
