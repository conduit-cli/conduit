import { useEffect, useLayoutEffect, useMemo, useRef, useState, useCallback } from 'react';
import { HistoryMessage } from './HistoryMessage';
import { ChatMessage } from './ChatMessage';
import { ToolRunMessage } from './ToolRunMessage';
import { ChatInput } from './ChatInput';
import { QueuePanel } from './QueuePanel';
import { InlinePrompt, type InlinePromptData, type InlinePromptResponse } from './InlinePrompt';
import { RawEventsPanel } from './RawEventsPanel';
import { ModelSelectorDialog } from './ModelSelectorDialog';
import {
  useSessionEvents,
  useWebSocket,
  useWorkspace,
  useWorkspaceStatus,
  useRawSessionEvents,
  useUpdateSession,
  useSetDefaultModel,
  useSessionQueue,
  useAddQueueMessage,
  useUpdateQueueMessage,
  useDeleteQueueMessage,
  useSessionHistory,
  useWorkspaceActions,
} from '../hooks';
import { getSessionEventsPage } from '../lib/api';
import type {
  Session,
  UserQuestion,
  SessionEvent,
  HistoryDebugEntry,
  AgentEvent,
  QueuedMessage,
  ImageAttachment,
} from '../types';
import { MessageSquarePlus, Loader2, Bug, GitBranch, GitPullRequest } from 'lucide-react';
import { cn } from '../lib/cn';

interface ChatViewProps {
  session: Session | null;
  onNewSession?: () => void;
  isLoadingSession?: boolean;
  onForkedSession?: (session: Session, workspace: { id: string }) => void;
}

const INLINE_PROMPT_TOOLS = new Set(['AskUserQuestion', 'ExitPlanMode']);

type ImageDraft = {
  id: string;
  file: File;
  previewUrl: string;
};

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error('Failed to read image'));
    reader.readAsDataURL(file);
  });
}

function parseImageDataUrl(dataUrl: string, fallbackType: string): ImageAttachment {
  if (dataUrl.startsWith('data:')) {
    const [meta, data] = dataUrl.split(',');
    const mediaType = meta?.split(';')[0]?.replace('data:', '') || fallbackType;
    return { data, media_type: mediaType };
  }
  return { data: dataUrl, media_type: fallbackType || 'application/octet-stream' };
}

function buildHistoryRawEvents(
  debugEntries: HistoryDebugEntry[] | undefined,
  debugFile: string | null | undefined,
  historyEventCount: number
): AgentEvent[] {
  const entries = debugEntries ?? [];
  const shouldInclude =
    debugFile !== undefined || debugEntries !== undefined || historyEventCount > 0;
  if (!shouldInclude) return [];
  const included = entries.filter((entry) => entry.status === 'INCLUDE').length;
  const skipped = entries.filter((entry) => entry.status === 'SKIP').length;
  const events: AgentEvent[] = [
    {
      type: 'Raw',
      data: {
        type: 'history_load',
        file: debugFile ?? null,
        total_entries: entries.length,
        included,
        skipped,
      },
    },
  ];

  entries.forEach((entry) => {
    events.push({
      type: 'Raw',
      data: {
        type: `L${entry.line} ${entry.status} ${entry.entry_type}`,
        line: entry.line,
        entry_type: entry.entry_type,
        status: entry.status,
        reason: entry.reason,
        raw: entry.raw,
      },
    });
  });

  return events;
}

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

export function ChatView({
  session,
  onNewSession,
  isLoadingSession,
  onForkedSession,
}: ChatViewProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const topSentinelRef = useRef<HTMLDivElement>(null);
  const pendingScrollAdjustment = useRef<{ previousHeight: number; previousTop: number } | null>(null);
  const isPrependingHistory = useRef(false);
  const isPinnedToBottom = useRef(true);
  const { sendPrompt, respondToControl } = useWebSocket();
  const wsEvents = useSessionEvents(session?.id ?? null);
  const updateSessionMutation = useUpdateSession();
  const setDefaultModelMutation = useSetDefaultModel();
  const [historyEvents, setHistoryEvents] = useState<SessionEvent[]>([]);
  const [historyOffset, setHistoryOffset] = useState(0);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const { data: workspace } = useWorkspace(session?.workspace_id ?? '');
  const { data: status } = useWorkspaceStatus(session?.workspace_id ?? null);
  const { handleForkSession, handleCreatePr, isForking, isCreatingPr } = useWorkspaceActions({
    session,
    workspace: workspace ?? null,
    onForkedSession,
  });
  const { data: inputHistory } = useSessionHistory(session?.id ?? null);
  const { data: queueData } = useSessionQueue(session?.id ?? null);
  const addQueueMutation = useAddQueueMessage();
  const updateQueueMutation = useUpdateQueueMessage();
  const deleteQueueMutation = useDeleteQueueMessage();
  const [isProcessing, setIsProcessing] = useState(false);
  const [isAwaitingResponse, setIsAwaitingResponse] = useState(false);
  const [hasInitiallyScrolled, setHasInitiallyScrolled] = useState(false);
  const [inlinePrompt, setInlinePrompt] = useState<InlinePromptData | null>(null);
  const [pendingControlResponse, setPendingControlResponse] = useState<unknown | null>(null);
  const [showRawEvents, setShowRawEvents] = useState(false);
  const [showModelSelector, setShowModelSelector] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [optimisticMessages, setOptimisticMessages] = useState<Record<string, string[]>>({});
  const [attachmentsBySession, setAttachmentsBySession] = useState<Record<string, ImageDraft[]>>(
    {}
  );
  const lastHistoryUserCount = useRef<Record<string, number>>({});
  const lastHistoryEventCount = useRef<Record<string, number>>({});
  const [historyRawEvents, setHistoryRawEvents] = useState<AgentEvent[]>([]);
  const rawEvents = useRawSessionEvents(session?.id ?? null, true);

  const historyLimit = 200;
  const hasMoreHistory = historyOffset > 0;

  useEffect(() => {
    let isActive = true;

    setHistoryEvents([]);
    setHistoryOffset(0);
    setIsLoadingMore(false);
    setHistoryRawEvents([]);

    if (!session?.id) {
      setIsLoadingHistory(false);
      return () => {
        isActive = false;
      };
    }

    setIsLoadingHistory(true);
    getSessionEventsPage(session.id, { tail: true, limit: historyLimit })
      .then((response) => {
        if (!isActive) return;
        setHistoryEvents(response.events);
        setHistoryOffset(response.offset);
        setHistoryRawEvents(
          buildHistoryRawEvents(
            response.debug_entries,
            response.debug_file,
            response.events.length
          )
        );
      })
      .catch(() => {
        if (!isActive) return;
        setHistoryEvents([]);
        setHistoryOffset(0);
      })
      .finally(() => {
        if (!isActive) return;
        setIsLoadingHistory(false);
      });

    return () => {
      isActive = false;
    };
  }, [session?.id, historyLimit]);

  const loadMoreHistory = useCallback(async () => {
    if (!session?.id || isLoadingMore || historyOffset === 0) return;

    const nextOffset = Math.max(0, historyOffset - historyLimit);
    const container = scrollContainerRef.current;
    if (container) {
      pendingScrollAdjustment.current = {
        previousHeight: container.scrollHeight,
        previousTop: container.scrollTop,
      };
    }
    isPrependingHistory.current = true;
    setIsLoadingMore(true);

    try {
      const response = await getSessionEventsPage(session.id, {
        offset: nextOffset,
        limit: historyLimit,
      });
      setHistoryEvents((prev) => [...response.events, ...prev]);
      setHistoryOffset(response.offset);
    } finally {
      setIsLoadingMore(false);
    }
  }, [historyLimit, historyOffset, isLoadingMore, session?.id]);

  useLayoutEffect(() => {
    if (!pendingScrollAdjustment.current || !scrollContainerRef.current) return;
    const { previousHeight, previousTop } = pendingScrollAdjustment.current;
    const container = scrollContainerRef.current;
    const newHeight = container.scrollHeight;
    container.scrollTop = previousTop + (newHeight - previousHeight);
    pendingScrollAdjustment.current = null;
    isPrependingHistory.current = false;
  }, [historyEvents]);

  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;

    const updatePinnedState = () => {
      const distanceFromBottom = container.scrollHeight - (container.scrollTop + container.clientHeight);
      isPinnedToBottom.current = distanceFromBottom < 48;
    };

    updatePinnedState();
    container.addEventListener('scroll', updatePinnedState, { passive: true });
    return () => container.removeEventListener('scroll', updatePinnedState);
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

  useEffect(() => {
    const container = scrollContainerRef.current;
    const sentinel = topSentinelRef.current;
    if (!container || !sentinel || !hasMoreHistory) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          loadMoreHistory();
        }
      },
      {
        root: container,
        rootMargin: '150px 0px 0px 0px',
        threshold: 0,
      }
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMoreHistory, loadMoreHistory]);

  // Reset scroll state when session changes
  useEffect(() => {
    setHasInitiallyScrolled(false);
  }, [session?.id]);

  useEffect(() => {
    if (!session || historyEvents.length === 0) return;

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

  const draftValue = session ? drafts[session.id] ?? '' : '';
  const optimisticUserMessages = session ? optimisticMessages[session.id] ?? [] : [];

  // Scroll to bottom - instant for initial load, follow new messages when pinned
  useEffect(() => {
    if (!scrollContainerRef.current) return;
    if (isPrependingHistory.current) return;

    const container = scrollContainerRef.current;

    // Initial scroll when history loads - instant, no animation
    if ((historyEvents.length > 0 || wsEvents.length > 0 || optimisticUserMessages.length > 0) && !hasInitiallyScrolled) {
      container.scrollTop = container.scrollHeight;
      setHasInitiallyScrolled(true);
      isPinnedToBottom.current = true;
      return;
    }

    if (!hasInitiallyScrolled || !isPinnedToBottom.current) return;

    const behavior = isProcessing ? 'auto' : 'smooth';
    container.scrollTo({ top: container.scrollHeight, behavior });
  }, [wsEvents, historyEvents, optimisticUserMessages.length, hasInitiallyScrolled, isProcessing]);

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
    void sendWithAttachments(message);
  };

  const buildImagePayload = useCallback(async (attachments: ImageDraft[]) => {
    if (attachments.length === 0) return [];
    const payloads = await Promise.all(
      attachments.map(async (attachment) => {
        const dataUrl = await readFileAsDataUrl(attachment.file);
        return parseImageDataUrl(dataUrl, attachment.file.type);
      })
    );
    return payloads;
  }, []);

  const handleAttachImages = (files: File[]) => {
    if (!session) return;
    const next = files
      .filter((file) => file.type.startsWith('image/'))
      .map((file) => ({
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        file,
        previewUrl: URL.createObjectURL(file),
      }));
    if (next.length === 0) return;
    setAttachmentsBySession((prev) => {
      const current = prev[session.id] ?? [];
      return { ...prev, [session.id]: [...current, ...next] };
    });
  };

  const handleRemoveAttachment = (attachmentId: string) => {
    if (!session) return;
    setAttachmentsBySession((prev) => {
      const current = prev[session.id] ?? [];
      const remaining = current.filter((attachment) => {
        if (attachment.id === attachmentId) {
          URL.revokeObjectURL(attachment.previewUrl);
          return false;
        }
        return true;
      });
      return { ...prev, [session.id]: remaining };
    });
  };

  const clearAttachments = (sessionId: string) => {
    setAttachmentsBySession((prev) => {
      const current = prev[sessionId] ?? [];
      current.forEach((attachment) => URL.revokeObjectURL(attachment.previewUrl));
      if (current.length === 0) {
        return prev;
      }
      const next = { ...prev };
      delete next[sessionId];
      return next;
    });
  };

  useEffect(() => {
    if (!session?.id) return;
    return () => {
      clearAttachments(session.id);
    };
  }, [session?.id]);

  const sendWithAttachments = async (message: string) => {
    if (!session || !workspace) return;

    let images: ImageAttachment[] = [];
    try {
      images = await buildImagePayload(currentAttachments);
    } catch (error) {
      console.error('Failed to encode images', error);
      return;
    }
    if (message.trim().length === 0 && images.length === 0) {
      return;
    }

    if (message.trim().length > 0) {
      setOptimisticMessages((prev) => ({
        ...prev,
        [session.id]: [...(prev[session.id] ?? []), message],
      }));
    }

    setIsAwaitingResponse(true);
    sendPrompt(session.id, message, workspace.path, session.model ?? undefined, false, images);
    setDrafts((prev) => ({ ...prev, [session.id]: '' }));
    clearAttachments(session.id);
  };

  const handleQueue = (message: string) => {
    if (!session) return;
    addQueueMutation.mutate(
      {
        id: session.id,
        data: {
          mode: 'follow-up',
          text: message,
          images: [],
        },
      },
      {
        onSuccess: () => {
          setDrafts((prev) => ({ ...prev, [session.id]: '' }));
        },
      }
    );
  };

  const handleSendQueued = (queued: QueuedMessage) => {
    if (!session || !workspace) return;
    setOptimisticMessages((prev) => ({
      ...prev,
      [session.id]: [...(prev[session.id] ?? []), queued.text],
    }));
    setIsAwaitingResponse(true);
    sendPrompt(session.id, queued.text, workspace.path, session.model ?? undefined);
    deleteQueueMutation.mutate({ id: session.id, messageId: queued.id });
  };

  const handleRemoveQueued = (messageId: string) => {
    if (!session) return;
    deleteQueueMutation.mutate({ id: session.id, messageId });
  };

  const handleMoveQueued = (messageId: string, position: number) => {
    if (!session) return;
    updateQueueMutation.mutate({ id: session.id, messageId, data: { position } });
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

  type ToolRunEvent = {
    type: 'ToolRun';
    tool_id?: string;
    tool_name?: string;
    arguments?: unknown;
    status: 'running' | 'success' | 'error';
    output?: string;
  };

  type RenderableEvent = typeof wsEvents[number] | ToolRunEvent;

  const renderableWsEvents = useMemo(() => {
    const toolIdToName = new Map<string, string>();
    wsEvents.forEach((event) => {
      if (event.type === 'ToolStarted') {
        toolIdToName.set(event.tool_id, event.tool_name);
      }
    });

    const merged: RenderableEvent[] = [];
    const toolIndexById = new Map<string, number>();

    const shouldSkip = (event: typeof wsEvents[number]) => {
      if (
        event.type === 'SessionInit' ||
        event.type === 'Raw' ||
        event.type === 'TokenUsage' ||
        event.type === 'ContextCompaction' ||
        event.type === 'ControlRequest'
      ) {
        return true;
      }
      if (event.type === 'ToolStarted' && INLINE_PROMPT_TOOLS.has(event.tool_name)) {
        return true;
      }
      if (event.type === 'ToolStarted' && event.tool_name === 'Bash') {
        return true;
      }
      if (event.type === 'ToolCompleted' && toolIdToName.get(event.tool_id) === 'Bash') {
        return true;
      }
      if (inlinePrompt && event.type === 'ToolCompleted' && event.tool_id === inlinePrompt.toolUseId) {
        return true;
      }
      return false;
    };

    for (const event of wsEvents) {
      if (shouldSkip(event)) {
        continue;
      }

      if (event.type === 'ToolStarted') {
        const run: ToolRunEvent = {
          type: 'ToolRun',
          tool_id: event.tool_id,
          tool_name: event.tool_name,
          arguments: event.arguments,
          status: 'running',
        };
        toolIndexById.set(event.tool_id, merged.length);
        merged.push(run);
        continue;
      }

      if (event.type === 'ToolCompleted') {
        const output = event.success ? (event.result ?? '') : (event.error ?? event.result ?? '');
        const status = event.success ? 'success' : 'error';
        const existingIndex = toolIndexById.get(event.tool_id);
        if (existingIndex !== undefined) {
          const existing = merged[existingIndex];
          if (existing.type === 'ToolRun') {
            merged[existingIndex] = {
              ...existing,
              status,
              output,
            };
            continue;
          }
        }

        merged.push({
          type: 'ToolRun',
          tool_id: event.tool_id,
          tool_name: toolIdToName.get(event.tool_id),
          status,
          output,
        });
        continue;
      }

      merged.push(event);
    }

    return merged;
  }, [inlinePrompt, wsEvents]);

  // Check if we have content to display
  const hasHistory = historyEvents.length > 0;
  const hasWsEvents = renderableWsEvents.length > 0;
  const hasOptimisticMessages = optimisticUserMessages.length > 0;
  const hasContent = hasHistory || hasWsEvents || hasOptimisticMessages;
  const rawEventsForView = useMemo(() => {
    const liveEvents = rawEvents.length > 0 ? rawEvents : wsEvents;
    return [...historyRawEvents, ...liveEvents];
  }, [historyRawEvents, rawEvents, wsEvents]);
  const userMessageHistory = useMemo(() => {
    if (!session) return [];
    if (inputHistory?.history?.length) {
      return [...inputHistory.history, ...optimisticUserMessages];
    }
    const historyMessages = historyEvents
      .filter((event) => event.role === 'user')
      .map((event) => event.content);
    return [...historyMessages, ...optimisticUserMessages];
  }, [historyEvents, optimisticUserMessages, session, inputHistory]);

  // Can only change model if session hasn't started (no agent_session_id) and not processing
  const canChangeModel = !session?.agent_session_id && !isProcessing;
  const canChangeMode =
    session?.agent_type === 'claude' && !session?.agent_session_id && !isProcessing;
  const effectiveAgentMode = session?.agent_mode ?? 'build';
  const queuedMessages = queueData?.messages ?? [];
  const canSendQueued = !!session && !!workspace && !isProcessing;
  const currentAttachments = session ? attachmentsBySession[session.id] ?? [] : [];

  const handleModelSelect = useCallback((modelId: string, newAgentType: 'claude' | 'codex' | 'gemini') => {
    if (!session) return;
    // Only include agent_type in the request if it's different from current
    const data: { model: string; agent_type?: 'claude' | 'codex' | 'gemini' } = { model: modelId };
    if (newAgentType !== session.agent_type) {
      data.agent_type = newAgentType;
    }
    updateSessionMutation.mutate(
      { id: session.id, data },
      {
        onSuccess: () => {
          setShowModelSelector(false);
        },
      }
    );
  }, [session, updateSessionMutation]);

  const handleSetDefaultModel = useCallback(
    (modelId: string, newAgentType: 'claude' | 'codex' | 'gemini') => {
      setDefaultModelMutation.mutate({ agent_type: newAgentType, model_id: modelId });
    },
    [setDefaultModelMutation]
  );

  const handleToggleAgentMode = useCallback(() => {
    if (!session) return;
    const nextMode = effectiveAgentMode === 'plan' ? 'build' : 'plan';
    updateSessionMutation.mutate({ id: session.id, data: { agent_mode: nextMode } });
  }, [effectiveAgentMode, session, updateSessionMutation]);

  // Keyboard shortcut for toggling plan mode (Ctrl+Shift+P)
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'p') {
        event.preventDefault();
        if (canChangeMode) {
          handleToggleAgentMode();
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [canChangeMode, handleToggleAgentMode]);

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
        {(isProcessing || isLoadingHistory || isLoadingMore) && (
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>
              {isLoadingHistory || isLoadingMore ? 'Loading history...' : 'Processing...'}
            </span>
          </div>
        )}

          <button
            onClick={handleForkSession}
            disabled={!session?.workspace_id || isProcessing || isForking}
            className={cn(
              'flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors',
              'text-text-muted hover:bg-surface-elevated hover:text-text',
              (!session?.workspace_id || isProcessing || isForking) &&
                'cursor-not-allowed opacity-50'
            )}
            aria-label="Fork session"
          >
            <GitBranch className="h-3.5 w-3.5" />
            Fork
          </button>

          <button
            onClick={handleCreatePr}
            disabled={!session?.workspace_id || isProcessing || isCreatingPr}
            className={cn(
              'flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors',
              'text-text-muted hover:bg-surface-elevated hover:text-text',
              (!session?.workspace_id || isProcessing || isCreatingPr) &&
                'cursor-not-allowed opacity-50'
            )}
            aria-label="Create pull request"
          >
            <GitPullRequest className="h-3.5 w-3.5" />
            PR
          </button>

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
            <div ref={topSentinelRef} />
            {(isLoadingHistory || isLoadingMore) && (
              <div className="flex items-center gap-2 text-xs text-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                <span>{isLoadingMore ? 'Loading older messages…' : 'Loading history…'}</span>
              </div>
            )}
            {!isLoadingHistory && !hasMoreHistory && historyEvents.length > 0 && (
              <div className="text-xs text-text-muted">Start of history</div>
            )}
            {/* Historical messages from API */}
            {historyEvents.map((event, index) => (
              <HistoryMessage key={`history-${index}`} event={event} />
            ))}
            {optimisticUserMessages.map((message, index) => (
              <HistoryMessage key={`optimistic-${index}`} event={{ role: 'user', content: message }} />
            ))}
            {/* Real-time messages from WebSocket */}
            {renderableWsEvents.map((event, index) => (
              event.type === 'ToolRun' ? (
                <ToolRunMessage
                  key={`ws-tool-${event.tool_id ?? index}`}
                  toolName={event.tool_name}
                  toolArgs={event.arguments}
                  status={event.status}
                  output={event.output}
                />
              ) : (
                <ChatMessage key={`ws-${index}`} event={event} />
              )
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
        <RawEventsPanel
          events={rawEventsForView}
          onClose={() => setShowRawEvents(false)}
        />
      )}

      <QueuePanel
        messages={queuedMessages}
        canSend={canSendQueued}
        onSend={handleSendQueued}
        onRemove={handleRemoveQueued}
        onMove={handleMoveQueued}
      />

      {/* Input area */}
      <ChatInput
        onSend={handleSend}
        onQueue={handleQueue}
        value={draftValue}
        onChange={handleDraftChange}
        inputDisabled={false}
        sendDisabled={isProcessing || !workspace}
        queueDisabled={!session || currentAttachments.length > 0}
        placeholder={isProcessing ? 'Waiting for response...' : 'Type a message...'}
        focusKey={session?.id ?? null}
        history={userMessageHistory}
        modelDisplayName={session?.model_display_name}
        agentType={session?.agent_type}
        agentMode={session?.agent_type === 'claude' ? effectiveAgentMode : undefined}
        gitStats={status?.git_stats}
        branch={workspace?.branch}
        onModelClick={() => setShowModelSelector(true)}
        canChangeModel={canChangeModel}
        onModeToggle={session?.agent_type === 'claude' ? handleToggleAgentMode : undefined}
        canChangeMode={canChangeMode}
        attachments={currentAttachments.map((attachment) => ({
          id: attachment.id,
          previewUrl: attachment.previewUrl,
          name: attachment.file.name,
        }))}
        onAttachImages={handleAttachImages}
        onRemoveAttachment={handleRemoveAttachment}
      />

      {/* Model selector dialog */}
      <ModelSelectorDialog
        isOpen={showModelSelector}
        onClose={() => setShowModelSelector(false)}
        currentModel={session?.model ?? null}
        agentType={session?.agent_type ?? 'claude'}
        onSelect={handleModelSelect}
        onSetDefault={handleSetDefaultModel}
        isUpdating={updateSessionMutation.isPending}
        isSettingDefault={setDefaultModelMutation.isPending}
      />
    </div>
  );
}
