import { useMemo, useState } from 'react';
import { CheckCircle2, MessageSquarePlus, XCircle } from 'lucide-react';
import type { UserQuestion } from '../types';
import { cn } from '../lib/cn';

export type InlinePromptData =
  | {
      type: 'ask_user';
      toolUseId: string;
      questions: UserQuestion[];
      requestId: string | null;
    }
  | {
      type: 'exit_plan';
      toolUseId: string;
      plan: string;
      requestId: string | null;
    };

export type InlinePromptResponse =
  | {
      type: 'ask_user';
      answers: Record<string, { kind: 'single' | 'multiple'; values: string[] }>;
    }
  | {
      type: 'exit_plan';
      approved: boolean;
      feedback?: string;
    };

interface InlinePromptProps {
  prompt: InlinePromptData;
  onSubmit: (response: InlinePromptResponse) => void;
  onCancel: () => void;
  isPending?: boolean;
}

export function InlinePrompt({ prompt, onSubmit, onCancel, isPending }: InlinePromptProps) {
  if (prompt.type === 'ask_user') {
    return (
      <AskUserPrompt
        prompt={prompt}
        onSubmit={onSubmit}
        onCancel={onCancel}
        isPending={isPending}
      />
    );
  }

  return (
    <ExitPlanPrompt
      prompt={prompt}
      onSubmit={onSubmit}
      onCancel={onCancel}
      isPending={isPending}
    />
  );
}

function AskUserPrompt({
  prompt,
  onSubmit,
  onCancel,
  isPending,
}: {
  prompt: Extract<InlinePromptData, { type: 'ask_user' }>;
  onSubmit: (response: InlinePromptResponse) => void;
  onCancel: () => void;
  isPending?: boolean;
}) {
  const [selected, setSelected] = useState<Record<number, string[]>>({});
  const [customInput, setCustomInput] = useState<Record<number, string>>({});

  const hasAnswers = useMemo(() => {
    return prompt.questions.some((_, index) => {
      const values = selected[index] ?? [];
      const custom = customInput[index]?.trim();
      return values.length > 0 || Boolean(custom);
    });
  }, [prompt.questions, selected, customInput]);

  const handleSubmit = () => {
    const answers: Record<string, { kind: 'single' | 'multiple'; values: string[] }> = {};

    prompt.questions.forEach((question, index) => {
      const optionValues = selected[index] ?? [];
      const customValue = customInput[index]?.trim();
      const combined = customValue ? [...optionValues, customValue] : optionValues;
      if (combined.length === 0) return;
      answers[question.question] = {
        kind: question.multiSelect ? 'multiple' : 'single',
        values: question.multiSelect ? combined : [combined[0]],
      };
    });

    onSubmit({ type: 'ask_user', answers });
  };

  return (
    <div className="rounded-xl border border-border bg-surface p-4">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm font-medium text-text">
          <MessageSquarePlus className="h-4 w-4 text-accent" />
          <span>Questions from agent</span>
        </div>
        <button
          onClick={onCancel}
          className="text-xs text-text-muted hover:text-text"
          disabled={isPending}
        >
          Cancel
        </button>
      </div>

      <div className="space-y-4">
        {prompt.questions.map((question, index) => (
          <div key={`${question.header}-${index}`} className="space-y-2">
            <div className="text-sm text-text">
              <span className="rounded bg-surface-elevated px-2 py-0.5 text-xs text-text-muted">
                {question.header || `Q${index + 1}`}
              </span>
              <p className="mt-2 text-sm text-text-bright">{question.question}</p>
            </div>

            <div className="space-y-2">
              {question.options.map((option) => {
                const isChecked = (selected[index] ?? []).includes(option.label);
                return (
                  <label
                    key={option.label}
                    className={cn(
                      'flex cursor-pointer items-start gap-3 rounded-lg border border-border px-3 py-2 text-sm transition-colors',
                      isChecked ? 'border-accent bg-accent/10 text-text' : 'text-text-muted hover:bg-surface-elevated'
                    )}
                  >
                    <input
                      type={question.multiSelect ? 'checkbox' : 'radio'}
                      name={`question-${index}`}
                      checked={isChecked}
                      onChange={(event) => {
                        const checked = event.target.checked;
                        setSelected((prev) => {
                          const current = prev[index] ?? [];
                          if (question.multiSelect) {
                            const next = checked
                              ? [...current, option.label]
                              : current.filter((value) => value !== option.label);
                            return { ...prev, [index]: next };
                          }
                          return { ...prev, [index]: checked ? [option.label] : [] };
                        });
                      }}
                      className="mt-1"
                    />
                    <div>
                      <div className="font-medium text-text">{option.label}</div>
                      {option.description && <p className="text-xs text-text-muted">{option.description}</p>}
                    </div>
                  </label>
                );
              })}

              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={customInput[index] ?? ''}
                  onChange={(event) =>
                    setCustomInput((prev) => ({
                      ...prev,
                      [index]: event.target.value,
                    }))
                  }
                  placeholder="Type something..."
                  className="w-full rounded-lg border border-border bg-surface-elevated px-3 py-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
                />
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="mt-4 flex items-center justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded-lg px-3 py-1.5 text-sm text-text-muted hover:bg-surface-elevated"
          disabled={isPending}
        >
          Cancel
        </button>
        <button
          onClick={handleSubmit}
          disabled={!hasAnswers || isPending}
          className="flex items-center gap-2 rounded-lg bg-accent px-3 py-1.5 text-sm text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-60"
        >
          <CheckCircle2 className="h-4 w-4" />
          Submit answers
        </button>
      </div>
    </div>
  );
}

function ExitPlanPrompt({
  prompt,
  onSubmit,
  onCancel,
  isPending,
}: {
  prompt: Extract<InlinePromptData, { type: 'exit_plan' }>;
  onSubmit: (response: InlinePromptResponse) => void;
  onCancel: () => void;
  isPending?: boolean;
}) {
  const [feedback, setFeedback] = useState('');

  return (
    <div className="rounded-xl border border-border bg-surface p-4">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm font-medium text-text">
          <MessageSquarePlus className="h-4 w-4 text-accent" />
          <span>Review plan</span>
        </div>
        <button
          onClick={onCancel}
          className="text-xs text-text-muted hover:text-text"
          disabled={isPending}
        >
          Cancel
        </button>
      </div>

      <div className="rounded-lg border border-border bg-surface-elevated p-3 text-sm text-text-muted">
        <pre className="whitespace-pre-wrap text-xs text-text">{prompt.plan || 'No plan content provided.'}</pre>
      </div>

      <div className="mt-3">
        <label className="text-xs text-text-muted">Feedback (optional)</label>
        <textarea
          value={feedback}
          onChange={(event) => setFeedback(event.target.value)}
          rows={3}
          className="mt-1 w-full rounded-lg border border-border bg-surface-elevated px-3 py-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
          placeholder="Type feedback to revise the plan"
        />
      </div>

      <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded-lg px-3 py-1.5 text-sm text-text-muted hover:bg-surface-elevated"
          disabled={isPending}
        >
          Cancel
        </button>
        <button
          onClick={() => onSubmit({ type: 'exit_plan', approved: false, feedback: feedback.trim() || undefined })}
          disabled={isPending || feedback.trim().length === 0}
          className="flex items-center gap-2 rounded-lg border border-border px-3 py-1.5 text-sm text-text-muted hover:bg-surface-elevated disabled:cursor-not-allowed disabled:opacity-60"
        >
          <XCircle className="h-4 w-4" />
          Send feedback
        </button>
        <button
          onClick={() => onSubmit({ type: 'exit_plan', approved: true })}
          disabled={isPending}
          className="flex items-center gap-2 rounded-lg bg-accent px-3 py-1.5 text-sm text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-60"
        >
          <CheckCircle2 className="h-4 w-4" />
          Approve plan
        </button>
      </div>
    </div>
  );
}
