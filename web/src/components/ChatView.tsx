import { useEffect, useRef, useState } from 'react';
import { HistoryMessage } from './HistoryMessage';
import { ChatMessage } from './ChatMessage';
import { ChatInput } from './ChatInput';
import { useSessionEvents, useWebSocket, useSessionEventsFromApi, useWorkspace, useWorkspaceStatus } from '../hooks';
import type { Session } from '../types';
import { MessageSquarePlus, Loader2 } from 'lucide-react';
import { cn } from '../lib/cn';

interface ChatViewProps {
  session: Session | null;
  onNewSession?: () => void;
  isLoadingSession?: boolean;
}

export function ChatView({ session, onNewSession, isLoadingSession }: ChatViewProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const { sendInput } = useWebSocket();
  const wsEvents = useSessionEvents(session?.id ?? null);
  const { data: historyEvents = [], isLoading: isLoadingHistory } = useSessionEventsFromApi(session?.id ?? null);
  const { data: workspace } = useWorkspace(session?.workspace_id ?? '');
  const { data: status } = useWorkspaceStatus(session?.workspace_id ?? null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [hasInitiallyScrolled, setHasInitiallyScrolled] = useState(false);

  // Track processing state based on websocket events
  useEffect(() => {
    if (wsEvents.length === 0) {
      setIsProcessing(false);
      return;
    }

    const lastEvent = wsEvents[wsEvents.length - 1];
    if (lastEvent.type === 'TurnStarted') {
      setIsProcessing(true);
    } else if (lastEvent.type === 'TurnCompleted' || lastEvent.type === 'Error') {
      setIsProcessing(false);
    }
  }, [wsEvents]);

  // Reset scroll state when session changes
  useEffect(() => {
    setHasInitiallyScrolled(false);
  }, [session?.id]);

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

  const handleSend = (message: string) => {
    if (session) {
      sendInput(session.id, message);
    }
  };

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

  // Filter websocket events to show meaningful ones (skip internal events)
  const visibleWsEvents = wsEvents.filter(
    (event) =>
      event.type !== 'SessionInit' &&
      event.type !== 'Raw' &&
      event.type !== 'TokenUsage' &&
      event.type !== 'ContextCompaction'
  );

  // Check if we have content to display
  const hasHistory = historyEvents.length > 0;
  const hasWsEvents = visibleWsEvents.length > 0;
  const hasContent = hasHistory || hasWsEvents;

  return (
    <div className="flex h-full flex-col">
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
        {(isProcessing || isLoadingHistory) && (
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>{isLoadingHistory ? 'Loading history...' : 'Processing...'}</span>
          </div>
        )}
      </div>

      {/* Messages area */}
      <div ref={scrollContainerRef} className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
        {!hasContent && !isLoadingHistory ? (
          <div className="flex h-full items-center justify-center text-text-muted">
            <p>Send a message to start the conversation</p>
          </div>
        ) : (
          <div className="min-w-0 space-y-4">
            {/* Historical messages from API */}
            {historyEvents.map((event, index) => (
              <HistoryMessage key={`history-${index}`} event={event} />
            ))}
            {/* Real-time messages from WebSocket */}
            {visibleWsEvents.map((event, index) => (
              <ChatMessage key={`ws-${index}`} event={event} />
            ))}
          </div>
        )}
      </div>

      {/* Input area */}
      <ChatInput
        onSend={handleSend}
        disabled={isProcessing}
        placeholder={isProcessing ? 'Waiting for response...' : 'Type a message...'}
        modelDisplayName={session?.model_display_name}
        agentType={session?.agent_type}
        agentMode={session?.agent_mode}
        gitStats={status?.git_stats}
        branch={workspace?.branch}
      />
    </div>
  );
}
