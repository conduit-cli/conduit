// React hooks for WebSocket communication

import { createContext, useContext, useEffect, useState, useCallback, useRef, type ReactNode } from 'react';
import { getWebSocket, type ConnectionState, type ConduitWebSocket } from '../lib/websocket';
import type { AgentEvent, ImageAttachment, ServerMessage } from '../types';

// WebSocket context
interface WebSocketContextValue {
  ws: ConduitWebSocket;
  connectionState: ConnectionState;
  processingSessionIds: Set<string>;
  sendInput: (
    sessionId: string,
    input: string,
    hidden?: boolean,
    images?: ImageAttachment[]
  ) => void;
  sendPrompt: (
    sessionId: string,
    prompt: string,
    workingDir: string,
    model?: string,
    hidden?: boolean,
    images?: ImageAttachment[]
  ) => void;
  startSession: (
    sessionId: string,
    prompt: string,
    workingDir: string,
    model?: string,
    hidden?: boolean,
    images?: ImageAttachment[]
  ) => void;
  stopSession: (sessionId: string) => void;
  respondToControl: (sessionId: string, requestId: string, response: unknown) => void;
}

const WebSocketContext = createContext<WebSocketContextValue | null>(null);

// Provider component
interface WebSocketProviderProps {
  children: ReactNode;
}

export function WebSocketProvider({ children }: WebSocketProviderProps) {
  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const [processingSessionIds, setProcessingSessionIds] = useState<Set<string>>(new Set());
  const runningSessionsRef = useRef(new Set<string>());
  const pendingPromptsRef = useRef(
    new Map<
      string,
      { prompt: string; workingDir: string; model?: string; images?: ImageAttachment[] }
    >()
  );
  const ws = getWebSocket();

  const handleServerMessage = useCallback(
    (message: ServerMessage) => {
      if (message.type === 'session_started') {
        runningSessionsRef.current.add(message.session_id);
        pendingPromptsRef.current.delete(message.session_id);
      }

      if (message.type === 'session_ended') {
        runningSessionsRef.current.delete(message.session_id);
        setProcessingSessionIds((prev) => {
          const next = new Set(prev);
          next.delete(message.session_id);
          return next;
        });
      }

      if (message.type === 'agent_event') {
        runningSessionsRef.current.add(message.session_id);
        const event = message.event;
        if (event.type === 'TurnStarted') {
          setProcessingSessionIds((prev) => {
            const next = new Set(prev);
            next.add(message.session_id);
            return next;
          });
        } else if (event.type === 'TurnCompleted' || event.type === 'TurnFailed') {
          setProcessingSessionIds((prev) => {
            const next = new Set(prev);
            next.delete(message.session_id);
            return next;
          });
        }
      }

      if (message.type === 'error' && message.session_id) {
        const pending = pendingPromptsRef.current.get(message.session_id);
        if (pending && message.message.includes('already running')) {
          runningSessionsRef.current.add(message.session_id);
          pendingPromptsRef.current.delete(message.session_id);
          ws.sendInput(message.session_id, pending.prompt, false, pending.images);
        }
      }
    },
    [ws]
  );

  useEffect(() => {
    ws.updateOptions({
      onConnect: () => setConnectionState('connected'),
      onDisconnect: () => setConnectionState('disconnected'),
      onError: () => setConnectionState('error'),
      onMessage: handleServerMessage,
    });
  }, [ws, handleServerMessage]);

  useEffect(() => {
    setConnectionState('connecting');
    ws.connect();
  }, [ws]);

  const sendInput = useCallback(
    (sessionId: string, input: string, hidden?: boolean, images?: ImageAttachment[]) => {
      ws.sendInput(sessionId, input, hidden, images);
    },
    [ws]
  );

  const startSession = useCallback(
    (
      sessionId: string,
      prompt: string,
      workingDir: string,
      model?: string,
      hidden?: boolean,
      images?: ImageAttachment[]
    ) => {
      ws.startSession(sessionId, prompt, workingDir, model, hidden, images);
    },
    [ws]
  );

  const sendPrompt = useCallback(
    (
      sessionId: string,
      prompt: string,
      workingDir: string,
      model?: string,
      hidden?: boolean,
      images?: ImageAttachment[]
    ) => {
      if (runningSessionsRef.current.has(sessionId)) {
        ws.sendInput(sessionId, prompt, hidden, images);
        return;
      }
      pendingPromptsRef.current.set(sessionId, { prompt, workingDir, model, images });
      ws.startSession(sessionId, prompt, workingDir, model, hidden, images);
    },
    [ws]
  );

  const stopSession = useCallback(
    (sessionId: string) => {
      ws.stopSession(sessionId);
    },
    [ws]
  );

  const respondToControl = useCallback(
    (sessionId: string, requestId: string, response: unknown) => {
      ws.respondToControl(sessionId, requestId, response);
    },
    [ws]
  );

  const value: WebSocketContextValue = {
    ws,
    connectionState,
    processingSessionIds,
    sendInput,
    sendPrompt,
    startSession,
    stopSession,
    respondToControl,
  };

  return <WebSocketContext.Provider value={value}>{children}</WebSocketContext.Provider>;
}

// Hook to access WebSocket context
export function useWebSocket(): WebSocketContextValue {
  const context = useContext(WebSocketContext);
  if (!context) {
    throw new Error('useWebSocket must be used within a WebSocketProvider');
  }
  return context;
}

// Hook for WebSocket connection state only
export function useWebSocketConnection(): ConnectionState {
  const { connectionState } = useWebSocket();
  return connectionState;
}

// Hook for accessing which sessions are currently processing
export function useProcessingSessions(): Set<string> {
  const { processingSessionIds } = useWebSocket();
  return processingSessionIds;
}

// Hook for subscribing to session events
const MAX_SESSION_EVENTS = 500;
const MAX_RAW_EVENTS = 200;
const SKIPPED_EVENT_TYPES = new Set(['Raw', 'TokenUsage', 'ContextCompaction']);

export function useSessionEvents(sessionId: string | null): AgentEvent[] {
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const { ws } = useWebSocket();

  useEffect(() => {
    if (!sessionId) {
      setEvents([]);
      return;
    }

    const handleEvent = (event: AgentEvent) => {
      if (SKIPPED_EVENT_TYPES.has(event.type)) {
        return;
      }

      setEvents((prev) => {
        let next = prev;

        if (event.type === 'CommandOutput' && event.is_streaming) {
          const last = prev[prev.length - 1];
          if (last?.type === 'CommandOutput' && last.is_streaming && last.command === event.command) {
            next = [...prev.slice(0, -1), event];
          } else {
            next = [...prev, event];
          }
        } else if (event.type === 'AssistantMessage') {
          const last = prev[prev.length - 1];
          if (last?.type === 'AssistantMessage' && !last.is_final) {
            next = [...prev.slice(0, -1), { ...event, text: last.text + event.text }];
          } else {
            next = [...prev, event];
          }
        } else if (event.type === 'AssistantReasoning') {
          const last = prev[prev.length - 1];
          if (last?.type === 'AssistantReasoning') {
            next = [...prev.slice(0, -1), { ...event, text: last.text + event.text }];
          } else {
            next = [...prev, event];
          }
        } else {
          next = [...prev, event];
        }

        if (next.length > MAX_SESSION_EVENTS) {
          next = next.slice(-MAX_SESSION_EVENTS);
        }

        return next;
      });
    };

    const unsubscribe = ws.subscribe(sessionId, handleEvent);

    return () => {
      unsubscribe();
      setEvents([]);
    };
  }, [sessionId, ws]);

  return events;
}

export function useRawSessionEvents(sessionId: string | null, enabled = true): AgentEvent[] {
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const { ws } = useWebSocket();

  useEffect(() => {
    if (!sessionId || !enabled) {
      setEvents([]);
      return;
    }

    const handleEvent = (event: AgentEvent) => {
      setEvents((prev) => {
        const next = [...prev, event];
        if (next.length > MAX_RAW_EVENTS) {
          return next.slice(-MAX_RAW_EVENTS);
        }
        return next;
      });
    };

    const unsubscribe = ws.subscribe(sessionId, handleEvent);

    return () => {
      unsubscribe();
      setEvents([]);
    };
  }, [sessionId, enabled, ws]);

  return events;
}

// Hook for managing a session with full controls
export function useAgentSession(sessionId: string | null) {
  const events = useSessionEvents(sessionId);
  const { sendInput, startSession, stopSession, respondToControl } = useWebSocket();
  const [currentMessage, setCurrentMessage] = useState('');
  const [isRunning, setIsRunning] = useState(false);

  // Track state from events
  useEffect(() => {
    if (events.length === 0) {
      setIsRunning(false);
      setCurrentMessage('');
      return;
    }

    const lastEvent = events[events.length - 1];
    switch (lastEvent.type) {
      case 'TurnStarted':
        setIsRunning(true);
        break;
      case 'TurnCompleted':
      case 'TurnFailed':
        setIsRunning(false);
        break;
      case 'AssistantMessage':
        if (lastEvent.is_final) {
          setCurrentMessage('');
        } else {
          setCurrentMessage((prev) => prev + lastEvent.text);
        }
        break;
    }
  }, [events]);

  const clearEvents = useCallback(() => {
    // Events are managed internally, this is a no-op but kept for API compatibility
  }, []);

  const boundStartSession = useCallback(
    (prompt: string, workingDir: string, model?: string) => {
      if (sessionId) {
        startSession(sessionId, prompt, workingDir, model);
      }
    },
    [sessionId, startSession]
  );

  const boundSendInput = useCallback(
    (input: string) => {
      if (sessionId) {
        sendInput(sessionId, input);
      }
    },
    [sessionId, sendInput]
  );

  const boundStopSession = useCallback(() => {
    if (sessionId) {
      stopSession(sessionId);
    }
  }, [sessionId, stopSession]);

  const boundRespondToControl = useCallback(
    (requestId: string, response: unknown) => {
      if (sessionId) {
        respondToControl(sessionId, requestId, response);
      }
    },
    [sessionId, respondToControl]
  );

  return {
    events,
    currentMessage,
    isRunning,
    startSession: boundStartSession,
    sendInput: boundSendInput,
    stopSession: boundStopSession,
    respondToControl: boundRespondToControl,
    clearEvents,
  };
}
