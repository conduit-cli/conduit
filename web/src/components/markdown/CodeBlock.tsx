import { useState, useEffect, useRef } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '../../lib/cn';
import { CopyButton } from '../ui/CopyButton';
import { getHighlighter, normalizeLanguage, THEME_MAP } from '../../lib/shiki';

interface CodeBlockProps {
  code: string;
  language?: string;
  showLineNumbers?: boolean;
  maxHeight?: number;
  className?: string;
}

const COLLAPSE_THRESHOLD = 20; // lines
const DEFAULT_MAX_HEIGHT = 400; // px

export function CodeBlock({
  code,
  language,
  showLineNumbers = false,
  maxHeight = DEFAULT_MAX_HEIGHT,
  className,
}: CodeBlockProps) {
  const [highlightedHtml, setHighlightedHtml] = useState<string | null>(null);
  const [isExpanded, setIsExpanded] = useState(false);
  const [shouldCollapse, setShouldCollapse] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);

  const normalizedLang = normalizeLanguage(language);
  const lineCount = code.split('\n').length;

  useEffect(() => {
    let cancelled = false;

    async function highlight() {
      try {
        const highlighter = await getHighlighter();
        if (cancelled) return;

        // Get current theme from document
        const isDark = !document.documentElement.classList.contains('light');
        const theme = isDark ? THEME_MAP.dark : THEME_MAP.light;

        const html = highlighter.codeToHtml(code, {
          lang: normalizedLang === 'text' ? 'text' : normalizedLang,
          theme,
        });

        if (!cancelled) {
          setHighlightedHtml(html);
        }
      } catch (err) {
        console.error('Syntax highlighting failed:', err);
        // Fallback to plain text
        if (!cancelled) {
          setHighlightedHtml(null);
        }
      }
    }

    highlight();
    return () => {
      cancelled = true;
    };
  }, [code, normalizedLang]);

  useEffect(() => {
    if (contentRef.current) {
      const scrollHeight = contentRef.current.scrollHeight;
      setShouldCollapse(lineCount > COLLAPSE_THRESHOLD || scrollHeight > maxHeight);
    }
  }, [highlightedHtml, lineCount, maxHeight]);

  const displayLanguage = normalizedLang !== 'text' ? normalizedLang : null;

  return (
    <div className={cn('group relative rounded-lg border border-border overflow-hidden', className)}>
      {/* Header with language badge and copy button */}
      <div className="flex items-center justify-between bg-surface-elevated px-3 py-1.5 border-b border-border">
        <span className="text-xs font-medium text-text-muted">
          {displayLanguage || 'text'}
        </span>
        <CopyButton text={code} />
      </div>

      {/* Code content */}
      <div
        ref={contentRef}
        className={cn(
          'overflow-auto bg-background',
          !isExpanded && shouldCollapse && 'max-h-[400px]'
        )}
        style={!isExpanded && shouldCollapse ? { maxHeight } : undefined}
      >
        {highlightedHtml ? (
          <div
            className={cn(
              'shiki-wrapper p-3',
              showLineNumbers && 'show-line-numbers'
            )}
            dangerouslySetInnerHTML={{ __html: highlightedHtml }}
          />
        ) : (
          <pre className="p-3 text-sm text-text overflow-x-auto">
            <code>{code}</code>
          </pre>
        )}
      </div>

      {/* Expand/collapse button */}
      {shouldCollapse && (
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className={cn(
            'flex w-full items-center justify-center gap-1 py-1.5',
            'bg-surface-elevated border-t border-border',
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
  );
}
