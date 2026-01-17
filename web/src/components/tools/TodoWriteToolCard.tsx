import { CheckSquare, Circle, CheckCircle2, CircleDot } from 'lucide-react';
import { ToolCard, type ToolStatus } from './ToolCard';
import { cn } from '../../lib/cn';

interface TodoWriteToolCardProps {
  status: ToolStatus;
  content?: string;
  error?: string;
}

interface TodoItem {
  content: string;
  status: 'pending' | 'in_progress' | 'completed';
  activeForm?: string;
}

interface TodoData {
  todos: TodoItem[];
}

function parseTodoContent(content: string): TodoData | null {
  try {
    const data = JSON.parse(content);
    if (data && Array.isArray(data.todos)) {
      return data as TodoData;
    }
    return null;
  } catch {
    return null;
  }
}

function TodoItemDisplay({ item }: { item: TodoItem }) {
  const statusIcon = {
    pending: <Circle className="h-3.5 w-3.5 text-text-muted" />,
    in_progress: <CircleDot className="h-3.5 w-3.5 text-amber-400" />,
    completed: <CheckCircle2 className="h-3.5 w-3.5 text-success" />,
  };

  const statusStyles = {
    pending: 'text-text-muted',
    in_progress: 'text-text font-medium',
    completed: 'text-text-muted line-through',
  };

  return (
    <div className="flex items-start gap-2 py-1.5 px-3">
      <span className="mt-0.5 shrink-0">{statusIcon[item.status]}</span>
      <span className={cn('text-sm', statusStyles[item.status])}>
        {item.content}
      </span>
    </div>
  );
}

export function TodoWriteToolCard({ status, content, error }: TodoWriteToolCardProps) {
  const todoData = content ? parseTodoContent(content) : null;
  const todoCount = todoData?.todos.length ?? 0;
  const completedCount = todoData?.todos.filter(t => t.status === 'completed').length ?? 0;
  const inProgressCount = todoData?.todos.filter(t => t.status === 'in_progress').length ?? 0;

  return (
    <ToolCard
      icon={<CheckSquare className="h-4 w-4" />}
      title="TodoWrite"
      subtitle={todoData ? `${completedCount}/${todoCount} completed${inProgressCount > 0 ? `, ${inProgressCount} in progress` : ''}` : undefined}
      status={status}
    >
      {error ? (
        <div className="p-3 text-sm text-error">{error}</div>
      ) : todoData && todoData.todos.length > 0 ? (
        <div className="py-1 divide-y divide-border/30">
          {todoData.todos.map((item, idx) => (
            <TodoItemDisplay key={idx} item={item} />
          ))}
        </div>
      ) : content ? (
        <pre className="p-3 text-xs text-text-muted overflow-auto max-h-[200px]">
          {content}
        </pre>
      ) : (
        <div className="p-3 text-xs text-text-muted">Todos updated</div>
      )}
    </ToolCard>
  );
}
