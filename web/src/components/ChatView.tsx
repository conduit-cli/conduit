import { useEffect, useMemo, useRef, useState } from 'react';
import { HistoryMessage } from './HistoryMessage';
import { ChatMessage } from './ChatMessage';
import { ChatInput } from './ChatInput';
import { InlinePrompt, type InlinePromptData, type InlinePromptResponse } from './InlinePrompt';
import {
  useSessionEvents,
  useWebSocket,
  useSessionEventsFromApi,
  useWorkspace,
  useWorkspaceStatus,
  useRawSessionEvents,
} from '../hooks';
import type { Session, UserQuestion } from '../types';
import { MessageSquarePlus, Loader2, Bug } from 'lucide-react';
import { cn } from '../lib/cn';

interface ChatViewProps {
  session: Session | null;
  onNewSession?: () => void;
  isLoadingSession?: boolean;
}

const INLINE_PROMPT_TOOLS = new Set(['AskUserQuestion', 'ExitPlanMode']);

function normalizeQuestions(questions: UserQuestion[]): UserQuestion[] {
  return questions.map((question, index) => {
    const header = question.header?.trim();
    if (header) {
      return { ...question };
    }
    const fallback = question.question?.trim()?.slice(0, 12);
    return {
      ...question,
      header: fallback && fallback.length > 0 ? fallback : `Q${index + 1}`,
    };
  });
}

function parseAskUserQuestions(args: unknown): UserQuestion[] | null {
  if (!args || typeof args !== 'object') return null;
  const questions = (args as { questions?: unknown }).questions;
  if (!Array.isArray(questions)) return null;
  return normalizeQuestions(questions as UserQuestion[]);
}

function parseExitPlan(args: unknown): string | null {
  if (!args || typeof args !== 'object') return null;
  const plan = (args as { plan?: unknown }).plan;
  return typeof plan === 'string' ? plan : null;
}

function buildPermissionAllowResponse(updatedInput: unknown, toolUseId?: string | null) {
  return {
    behavior: 'allow',
    updatedInput,
    ...(toolUseId ? { toolUseID: toolUseId } : {}),
  };
}

function buildPermissionDenyResponse(message: string, toolUseId?: string | null) {
  return {
    behavior: 'deny',
    message,
    ...(toolUseId ? { toolUseID: toolUseId } : {}),
  };
}

function buildAskUserUpdatedInput(
  questions: UserQuestion[],
  answers: Record<string, { kind: 'single' | 'multiple'; values: string[] }>
) {
  const formattedAnswers: Record<string, string> = {};
  Object.entries(answers).forEach(([question, answer]) => {
    formattedAnswers[question] = answer.values.join(', ');
  });
  return {
    questions,
    answers: formattedAnswers,
  };
}

function buildExitPlanUpdatedInput(plan: string) {
  return { plan };
}

export function ChatView({ session, onNewSession, isLoadingSession }: ChatViewProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const { sendPrompt, respondToControl } = useWebSocket();
  const wsEvents = useSessionEvents(session?.id ?? null);
  const [historyReady, setHistoryReady] = useState(false);
  const { data: historyEvents = [], isLoading: isLoadingHistory } = useSessionEventsFromApi(
    session?.id ?? null,
    {
      enabled: historyReady && !!session?.id,
      query: { tail: true, limit: 200 },
    }
  );
  const { data: workspace } = useWorkspace(session?.workspace_id ?? '');
  const { data: status } = useWorkspaceStatus(session?.workspace_id ?? null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isAwaitingResponse, setIsAwaitingResponse] = useState(false);
  const [hasInitiallyScrolled, setHasInitiallyScrolled] = useState(false);
  const [inlinePrompt, setInlinePrompt] = useState<InlinePromptData | null>(null);
  const [pendingControlResponse, setPendingControlResponse] = useState<unknown | null>(null);
  const [showRawEvents, setShowRawEvents] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [optimisticMessages, setOptimisticMessages] = useState<Record<string, string[]>>({});
  const lastHistoryUserCount = useRef<Record<string, number>>({});
  const lastHistoryEventCount = useRef<Record<string, number>>({});
  const rawEvents = useRawSessionEvents(session?.id ?? null, showRawEvents);

  useEffect(() => {
    setHistoryReady(true);
  }, []);

  // Track processing state based on websocket events
  useEffect(() => {
    if (wsEvents.length === 0) {
      setIsProcessing(isAwaitingResponse);
      return;
    }

    const lastEvent = wsEvents[wsEvents.length - 1];
    if (lastEvent.type === 'TurnStarted') {
      setIsProcessing(true);
      setIsAwaitingResponse(false);
    } else if (
      lastEvent.type === 'TurnCompleted' ||
      lastEvent.type === 'TurnFailed' ||
      lastEvent.type === 'Error' ||
      (lastEvent.type === 'AssistantMessage' && lastEvent.is_final)
    ) {
      setIsProcessing(false);
      setIsAwaitingResponse(false);
    }
  }, [wsEvents, isAwaitingResponse]);

  useEffect(() => {
    setInlinePrompt(null);
    setPendingControlResponse(null);
    setShowRawEvents(false);
    setIsAwaitingResponse(false);
  }, [session?.id]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'r') {
        event.preventDefault();
        setShowRawEvents((prev) => !prev);
      }
      if (event.key === 'Escape' && showRawEvents) {
        setShowRawEvents(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [showRawEvents]);

  // Reset scroll state when session changes
  useEffect(() => {
    setHasInitiallyScrolled(false);
  }, [session?.id]);

  useEffect(() => {
    if (!session) return;

    const userCount = historyEvents.filter((event) => event.role === 'user').length;
    const previousCount = lastHistoryUserCount.current[session.id] ?? 0;
    if (userCount === previousCount) return;
    lastHistoryUserCount.current[session.id] = userCount;

    const delta = Math.max(0, userCount - previousCount);
    if (delta === 0) return;

    setOptimisticMessages((prev) => {
      const current = prev[session.id] ?? [];
      if (current.length === 0) return prev;

      const remaining = current.slice(delta);
      if (remaining.length === current.length) return prev;

      const next = { ...prev };
      if (remaining.length === 0) {
        delete next[session.id];
      } else {
        next[session.id] = remaining;
      }
      return next;
    });
  }, [historyEvents, session]);

  useEffect(() => {
    if (!session || historyEvents.length === 0) return;

    const previousCount = lastHistoryEventCount.current[session.id] ?? 0;
    if (historyEvents.length === previousCount) return;
    lastHistoryEventCount.current[session.id] = historyEvents.length;

    const lastEvent = historyEvents[historyEvents.length - 1];
    if (!isProcessing && !isAwaitingResponse) return;

    if (lastEvent.role === 'assistant' || lastEvent.role === 'summary' || lastEvent.role === 'error') {
      setIsProcessing(false);
      setIsAwaitingResponse(false);
    }
  }, [historyEvents, session, isProcessing, isAwaitingResponse]);

  // Scroll to bottom - instant for initial load, smooth for new messages
  useEffect(() => {
    if (!scrollContainerRef.current) return;

    const container = scrollContainerRef.current;

    // Initial scroll when history loads - instant, no animation
    if (historyEvents.length > 0 && !hasInitiallyScrolled) {
      container.scrollTop = container.scrollHeight;
      setHasInitiallyScrolled(true);
      return;
    }

    // Smooth scroll for new WebSocket messages
    if (wsEvents.length > 0 && hasInitiallyScrolled) {
      container.scrollTo({ top: container.scrollHeight, behavior: 'smooth' });
    }
  }, [wsEvents, historyEvents, hasInitiallyScrolled]);

  useEffect(() => {
    if (!session || wsEvents.length === 0) return;

    const lastEvent = wsEvents[wsEvents.length - 1];

    if (lastEvent.type === 'ToolStarted' && INLINE_PROMPT_TOOLS.has(lastEvent.tool_name)) {
      if (lastEvent.tool_name === 'AskUserQuestion') {
        const questions = parseAskUserQuestions(lastEvent.arguments);
        if (questions) {
          setInlinePrompt({
            type: 'ask_user',
            toolUseId: lastEvent.tool_id,
            questions,
            requestId: null,
          });
          return;
        }
      }

      if (lastEvent.tool_name === 'ExitPlanMode') {
        const plan = parseExitPlan(lastEvent.arguments) ?? '';
        setInlinePrompt({
          type: 'exit_plan',
          toolUseId: lastEvent.tool_id,
          plan,
          requestId: null,
        });
        return;
      }
    }

    if (lastEvent.type === 'ControlRequest' && inlinePrompt) {
      if (lastEvent.tool_use_id && lastEvent.tool_use_id === inlinePrompt.toolUseId) {
        setInlinePrompt({ ...inlinePrompt, requestId: lastEvent.request_id });
        if (pendingControlResponse) {
          respondToControl(session.id, lastEvent.request_id, pendingControlResponse);
          setPendingControlResponse(null);
          setInlinePrompt(null);
        }
        return;
      }
    }

    if (lastEvent.type === 'ToolCompleted' && inlinePrompt) {
      if (lastEvent.tool_id === inlinePrompt.toolUseId) {
        setInlinePrompt(null);
        setPendingControlResponse(null);
      }
    }
  }, [
    wsEvents,
    inlinePrompt,
    pendingControlResponse,
    respondToControl,
    session,
    setInlinePrompt,
  ]);

  const handleSend = (message: string) => {
    if (!session || !workspace) return;
    setOptimisticMessages((prev) => ({
      ...prev,
      [session.id]: [...(prev[session.id] ?? []), message],
    }));
    setIsAwaitingResponse(true);
    sendPrompt(session.id, message, workspace.path, session.model ?? undefined);
    setDrafts((prev) => ({ ...prev, [session.id]: '' }));
  };

  const handleDraftChange = (value: string) => {
    if (!session) return;
    setDrafts((prev) => ({ ...prev, [session.id]: value }));
  };

  const handlePromptSubmit = (response: InlinePromptResponse) => {
    if (!session || !inlinePrompt) return;

    let controlResponse: unknown;

    if (response.type === 'ask_user' && inlinePrompt.type === 'ask_user') {
      const updatedInput = buildAskUserUpdatedInput(inlinePrompt.questions, response.answers);
      controlResponse = buildPermissionAllowResponse(updatedInput, inlinePrompt.toolUseId);
    } else if (response.type === 'exit_plan' && inlinePrompt.type === 'exit_plan') {
      if (response.approved) {
        const updatedInput = buildExitPlanUpdatedInput(inlinePrompt.plan);
        controlResponse = buildPermissionAllowResponse(updatedInput, inlinePrompt.toolUseId);
      } else {
        const feedbackMessage = response.feedback
          ? `User feedback on plan: ${response.feedback}`
          : 'User feedback on plan.';
        controlResponse = buildPermissionDenyResponse(feedbackMessage, inlinePrompt.toolUseId);
      }
    } else {
      return;
    }

    if (inlinePrompt.requestId) {
      respondToControl(session.id, inlinePrompt.requestId, controlResponse);
      setInlinePrompt(null);
      setPendingControlResponse(null);
      return;
    }

    setPendingControlResponse(controlResponse);
  };

  const handlePromptCancel = () => {
    if (!session || !inlinePrompt) return;
    const controlResponse = buildPermissionDenyResponse('User cancelled the prompt.', inlinePrompt.toolUseId);
    if (inlinePrompt.requestId) {
      respondToControl(session.id, inlinePrompt.requestId, controlResponse);
      setInlinePrompt(null);
      setPendingControlResponse(null);
      return;
    }
    setPendingControlResponse(controlResponse);
  };

  const visibleWsEvents = useMemo(
    () =>
      wsEvents.filter((event) => {
        if (
          event.type === 'SessionInit' ||
          event.type === 'Raw' ||
          event.type === 'TokenUsage' ||
          event.type === 'ContextCompaction' ||
          event.type === 'ControlRequest'
        ) {
          return false;
        }
        if (event.type === 'ToolStarted' && INLINE_PROMPT_TOOLS.has(event.tool_name)) {
          return false;
        }
        if (inlinePrompt && event.type === 'ToolCompleted' && event.tool_id === inlinePrompt.toolUseId) {
          return false;
        }
        return true;
      }),
    [wsEvents, inlinePrompt]
  );

  // Check if we have content to display
  const hasHistory = historyEvents.length > 0;
  const hasWsEvents = visibleWsEvents.length > 0;
  const draftValue = session ? drafts[session.id] ?? '' : '';
  const optimisticUserMessages = session ? optimisticMessages[session.id] ?? [] : [];
  const hasOptimisticMessages = optimisticUserMessages.length > 0;
  const hasContent = hasHistory || hasWsEvents || hasOptimisticMessages;

  // Loading session state (when workspace is selected but session is being created/fetched)
  if (isLoadingSession) {
    return (
      <div className="flex h-full flex-col items-center justify-center text-text-muted">
        <Loader2 className="mb-4 h-16 w-16 animate-spin opacity-50" />
        <h2 className="mb-2 text-xl font-medium text-text">Loading Session...</h2>
        <p className="text-center">Setting up your workspace session</p>
      </div>
    );
  }

  // No session selected state
  if (!session) {
    return (
      <div className="flex h-full flex-col items-center justify-center text-text-muted">
        <MessageSquarePlus className="mb-4 h-16 w-16 opacity-50" />
        <h2 className="mb-2 text-xl font-medium text-text">No Session Selected</h2>
        <p className="mb-6 text-center">
          Select an existing session from the sidebar
          <br />
          or create a new one to get started.
        </p>
        {onNewSession && (
          <button
            onClick={onNewSession}
            className="rounded-lg bg-accent px-6 py-2.5 text-sm font-medium text-white transition-colors hover:bg-accent-hover"
          >
            Start New Session
          </button>
        )}
      </div>
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      {/* Session header */}
      <div className="flex shrink-0 items-center justify-between border-b border-border px-4 py-3">
        <div className="flex items-center gap-3">
          <span
            className={cn(
              'h-3 w-3 rounded-full',
              session.agent_type === 'claude'
                ? 'bg-orange-400'
                : session.agent_type === 'codex'
                ? 'bg-green-400'
                : 'bg-blue-400'
            )}
          />
          <div>
            <h3 className="font-medium text-text">
              {session.title || `Session ${session.tab_index + 1}`}
            </h3>
            <p className="text-xs text-text-muted">
              {session.model && <span>{session.model}</span>}
              {session.model && ' · '}
              <span className="capitalize">
                {session.agent_type === 'claude'
                  ? 'Claude Code'
                  : session.agent_type === 'codex'
                  ? 'Codex CLI'
                  : 'Gemini CLI'}
              </span>
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {(isProcessing || isLoadingHistory) && (
            <div className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>{isLoadingHistory ? 'Loading history...' : 'Processing...'}</span>
            </div>
          )}
          <button
            onClick={() => setShowRawEvents((prev) => !prev)}
            className={cn(
              'flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors',
              showRawEvents
                ? 'bg-accent/20 text-accent'
                : 'text-text-muted hover:bg-surface-elevated hover:text-text'
            )}
          >
            <Bug className="h-3.5 w-3.5" />
            Raw events
          </button>
        </div>
      </div>

      {/* Messages area */}
      <div ref={scrollContainerRef} className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
        {!hasContent && !isLoadingHistory ? (
          <div className="flex h-full items-center justify-center text-text-muted">
            <p>Send a message to start the conversation</p>
          </div>
        ) : (
          <div className="min-w-0 space-y-4">
            {isLoadingHistory && (
              <div className="flex items-center gap-2 text-xs text-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                <span>Loading history…</span>
              </div>
            )}
            {/* Historical messages from API */}
            {historyEvents.map((event, index) => (
              <HistoryMessage key={`history-${index}`} event={event} />
            ))}
            {optimisticUserMessages.map((message, index) => (
              <HistoryMessage key={`optimistic-${index}`} event={{ role: 'user', content: message }} />
            ))}
            {/* Real-time messages from WebSocket */}
            {visibleWsEvents.map((event, index) => (
              <ChatMessage key={`ws-${index}`} event={event} />
            ))}
            {inlinePrompt && (
              <InlinePrompt
                prompt={inlinePrompt}
                onSubmit={handlePromptSubmit}
                onCancel={handlePromptCancel}
                isPending={!inlinePrompt.requestId}
              />
            )}
          </div>

        )}
      </div>

      {showRawEvents && (
        <div className="absolute right-0 top-0 z-10 flex h-full w-full max-w-md flex-col border-l border-border bg-surface shadow-xl">
          <div className="flex items-center justify-between border-b border-border px-4 py-3">
            <div className="text-sm font-medium text-text">Raw events</div>
            <button
              onClick={() => setShowRawEvents(false)}
              className="text-xs text-text-muted hover:text-text"
            >
              Close
            </button>
          </div>
          <div className="flex-1 overflow-y-auto p-3">
            {rawEvents.length === 0 ? (
              <p className="text-xs text-text-muted">No raw events captured.</p>
            ) : (
              <div className="space-y-2">
                {rawEvents.map((event, index) => (
                  <pre
                    key={`raw-${index}`}
                    className="rounded-lg border border-border bg-surface-elevated p-2 text-xs text-text-muted"
                  >
                    {JSON.stringify(event, null, 2)}
                  </pre>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Input area */}
      <ChatInput
        onSend={handleSend}
        value={draftValue}
        onChange={handleDraftChange}
        disabled={isProcessing}
        placeholder={isProcessing ? 'Waiting for response...' : 'Type a message...'}
        focusKey={session?.id ?? null}
        modelDisplayName={session?.model_display_name}
        agentType={session?.agent_type}
        agentMode={session?.agent_mode}
        gitStats={status?.git_stats}
        branch={workspace?.branch}
      />
    </div>
  );
}
