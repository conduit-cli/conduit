import type { ReactNode, ComponentPropsWithoutRef } from 'react';
import React from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '../../lib/cn';
import { CodeBlock } from './CodeBlock';
import { InlineCode } from './InlineCode';
import { FilePathLink } from '../FilePathLink';

// Regex to match file paths:
// - Absolute paths: /foo/bar.ts
// - Relative paths: ./foo/bar.ts, ../foo/bar.ts
// - Hidden directory paths: .opencode/plan/file.md
// - Relative paths with directories: src/components/file.tsx
const FILE_PATH_REGEX = /(?:^|\s|[(`])((\/[\w.-]+)+\.\w+|\.\.?\/[\w./-]+\.\w+|\.[a-zA-Z][\w.-]*\/[\w./-]+\.\w+|(?:[a-zA-Z][\w.-]*\/)+[\w.-]+\.\w+)(?=[\s,;:)\]`]|$)/g;

function processTextWithFilePaths(text: string): ReactNode[] {
  const result: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  // Reset regex state
  FILE_PATH_REGEX.lastIndex = 0;

  while ((match = FILE_PATH_REGEX.exec(text)) !== null) {
    const filePath = match[1];
    const matchStart = match.index + (match[0].length - filePath.length - (match[0].endsWith(filePath) ? 0 : 1));

    // Add text before the match
    if (matchStart > lastIndex) {
      result.push(text.slice(lastIndex, matchStart));
    }

    // Add the file path link
    result.push(
      <FilePathLink key={matchStart} path={filePath} className="text-inherit" />
    );

    lastIndex = matchStart + filePath.length;
  }

  // Add remaining text
  if (lastIndex < text.length) {
    result.push(text.slice(lastIndex));
  }

  return result.length > 0 ? result : [text];
}

function processChildren(children: ReactNode): ReactNode {
  if (typeof children === 'string') {
    const processed = processTextWithFilePaths(children);
    return processed.length === 1 && typeof processed[0] === 'string'
      ? children
      : <>{processed}</>;
  }

  if (Array.isArray(children)) {
    return children.map((child, index) => {
      if (typeof child === 'string') {
        const processed = processTextWithFilePaths(child);
        return processed.length === 1 && typeof processed[0] === 'string'
          ? child
          : <React.Fragment key={index}>{processed}</React.Fragment>;
      }
      return child;
    });
  }

  return children;
}

interface MarkdownBodyProps {
  content: string;
  className?: string;
}

export function MarkdownBody({ content, className }: MarkdownBodyProps) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      className={cn('prose prose-sm prose-invert max-w-none', className)}
      components={{
        p: ({ children }: { children?: ReactNode }) => (
          <p className="whitespace-pre-wrap break-words text-pretty text-sm text-text mb-3 last:mb-0">
            {processChildren(children)}
          </p>
        ),
        h1: ({ children }: { children?: ReactNode }) => (
          <h1 className="text-xl font-bold text-text-bright mb-4 mt-6 first:mt-0">
            {children}
          </h1>
        ),
        h2: ({ children }: { children?: ReactNode }) => (
          <h2 className="text-lg font-semibold text-text-bright mb-3 mt-5 first:mt-0">
            {children}
          </h2>
        ),
        h3: ({ children }: { children?: ReactNode }) => (
          <h3 className="text-base font-semibold text-text-bright mb-2 mt-4 first:mt-0">
            {children}
          </h3>
        ),
        ul: ({ children }: { children?: ReactNode }) => (
          <ul className="list-disc list-inside space-y-1 mb-3 text-sm text-text">
            {children}
          </ul>
        ),
        ol: ({ children }: { children?: ReactNode }) => (
          <ol className="list-decimal list-inside space-y-1 mb-3 text-sm text-text">
            {children}
          </ol>
        ),
        li: ({ children }: { children?: ReactNode }) => (
          <li className="text-text">{processChildren(children)}</li>
        ),
        blockquote: ({ children }: { children?: ReactNode }) => (
          <blockquote className="border-l-2 border-accent pl-4 italic text-text-muted mb-3">
            {children}
          </blockquote>
        ),
        a: ({ href, children }: { href?: string; children?: ReactNode }) => (
          <a
            href={href}
            target="_blank"
            rel="noopener noreferrer"
            className="text-accent hover:text-accent-hover underline underline-offset-2"
          >
            {children}
          </a>
        ),
        strong: ({ children }: { children?: ReactNode }) => (
          <strong className="font-semibold text-text-bright">{children}</strong>
        ),
        em: ({ children }: { children?: ReactNode }) => (
          <em className="italic text-text">{children}</em>
        ),
        hr: () => <hr className="border-border my-4" />,
        table: ({ children }: { children?: ReactNode }) => (
          <div className="overflow-x-auto mb-3">
            <table className="min-w-full border-collapse text-sm">{children}</table>
          </div>
        ),
        thead: ({ children }: { children?: ReactNode }) => (
          <thead className="bg-surface-elevated">{children}</thead>
        ),
        th: ({ children }: { children?: ReactNode }) => (
          <th className="border border-border px-3 py-2 text-left font-semibold text-text-bright">
            {children}
          </th>
        ),
        td: ({ children }: { children?: ReactNode }) => (
          <td className="border border-border px-3 py-2 text-text">{processChildren(children)}</td>
        ),
        code: (props: ComponentPropsWithoutRef<'code'>) => {
          const { children, className } = props;
          const match = /language-(\w+)/.exec(className || '');
          const isCodeBlock = Boolean(match);
          const codeContent = String(children).replace(/\n$/, '');

          if (isCodeBlock) {
            return <CodeBlock code={codeContent} language={match?.[1]} />;
          }

          return <InlineCode>{children}</InlineCode>;
        },
        pre: ({ children }: { children?: ReactNode }) => {
          // The code component handles rendering, so pre just passes through
          return <div className="my-3">{children}</div>;
        },
      }}
    >
      {content}
    </ReactMarkdown>
  );
}
