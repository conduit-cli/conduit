import { useState, useCallback } from 'react';
import { Check, Copy } from 'lucide-react';
import { cn } from '../../lib/cn';
import { copyText } from '../../lib/clipboard';

interface CopyButtonProps {
  text: string;
  className?: string;
}

export function CopyButton({ text, className }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await copyText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }, [text]);

  return (
    <button
      onClick={handleCopy}
      className={cn(
        'flex items-center justify-center rounded p-1.5',
        'text-text-muted hover:text-text hover:bg-surface-elevated/50',
        'transition-all duration-200',
        className
      )}
      title={copied ? 'Copied!' : 'Copy to clipboard'}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-success" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </button>
  );
}
