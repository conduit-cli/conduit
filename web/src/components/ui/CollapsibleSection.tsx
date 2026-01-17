import { useState, useRef, useEffect, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '../../lib/cn';

interface CollapsibleSectionProps {
  title: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  className?: string;
  headerClassName?: string;
  contentClassName?: string;
}

export function CollapsibleSection({
  title,
  children,
  defaultOpen = false,
  className,
  headerClassName,
  contentClassName,
}: CollapsibleSectionProps) {
  const [isOpen, setIsOpen] = useState(defaultOpen);
  const [height, setHeight] = useState<number | undefined>(defaultOpen ? undefined : 0);
  const contentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!contentRef.current) return;

    if (isOpen) {
      const contentHeight = contentRef.current.scrollHeight;
      setHeight(contentHeight);
      // After animation, set to auto to handle dynamic content
      const timer = setTimeout(() => setHeight(undefined), 200);
      return () => clearTimeout(timer);
    } else {
      // First set to current height, then animate to 0
      setHeight(contentRef.current.scrollHeight);
      requestAnimationFrame(() => {
        setHeight(0);
      });
    }
  }, [isOpen]);

  return (
    <div className={cn('rounded-lg border border-border overflow-hidden', className)}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'flex w-full items-center gap-2 px-3 py-2 text-left',
          'bg-surface-elevated hover:bg-bg-highlight',
          'transition-colors duration-150',
          headerClassName
        )}
      >
        <ChevronRight
          className={cn(
            'h-4 w-4 text-text-muted transition-transform duration-200',
            isOpen && 'rotate-90'
          )}
        />
        <span className="flex-1 text-sm text-text-muted">{title}</span>
      </button>
      <div
        ref={contentRef}
        style={{ height: height === undefined ? 'auto' : height }}
        className={cn(
          'overflow-hidden transition-[height] duration-200 ease-in-out',
          contentClassName
        )}
      >
        <div className="border-t border-border">{children}</div>
      </div>
    </div>
  );
}
