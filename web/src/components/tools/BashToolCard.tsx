import { Terminal } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { TerminalOutput } from '../TerminalOutput';

interface BashToolCardProps {
  status: ToolStatus;
  command: string;
  output?: string;
  exitCode?: number | null;
  error?: string;
}

export function BashToolCard({ status, command, output, exitCode, error }: BashToolCardProps) {
  // For subtitle, show first line and truncate if needed
  const firstLine = command.split(/\\n|\n/)[0];
  const subtitle = firstLine.length > 60 ? `${firstLine.slice(0, 60)}...` : firstLine;

  return (
    <ToolCard
      icon={<Terminal className="h-4 w-4" />}
      title="Bash"
      subtitle={subtitle}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : (
        <div className="p-2">
          <TerminalOutput command={command} output={output} exitCode={exitCode} />
        </div>
      )}
    </ToolCard>
  );
}
