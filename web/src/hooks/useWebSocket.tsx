// React hooks for WebSocket communication

import { createContext, useContext, useEffect, useState, useCallback, type ReactNode } from 'react';
import { getWebSocket, type ConnectionState, type ConduitWebSocket } from '../lib/websocket';
import type { AgentEvent } from '../types';

// WebSocket context
interface WebSocketContextValue {
  ws: ConduitWebSocket;
  connectionState: ConnectionState;
  sendInput: (sessionId: string, input: string) => void;
  startSession: (sessionId: string, prompt: string, workingDir: string, model?: string) => void;
  stopSession: (sessionId: string) => void;
  respondToControl: (sessionId: string, requestId: string, allow: boolean) => void;
}

const WebSocketContext = createContext<WebSocketContextValue | null>(null);

// Provider component
interface WebSocketProviderProps {
  children: ReactNode;
}

export function WebSocketProvider({ children }: WebSocketProviderProps) {
  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const ws = getWebSocket();

  useEffect(() => {
    setConnectionState('connecting');
    ws.connect();
    // Connection state will be updated by the WebSocket callbacks when implemented
  }, [ws]);

  const sendInput = useCallback(
    (sessionId: string, input: string) => {
      ws.sendInput(sessionId, input);
    },
    [ws]
  );

  const startSession = useCallback(
    (sessionId: string, prompt: string, workingDir: string, model?: string) => {
      ws.startSession(sessionId, prompt, workingDir, model);
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
    (sessionId: string, requestId: string, allow: boolean) => {
      ws.respondToControl(sessionId, requestId, allow);
    },
    [ws]
  );

  const value: WebSocketContextValue = {
    ws,
    connectionState,
    sendInput,
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

// Hook for subscribing to session events
const MAX_SESSION_EVENTS = 500;
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
    (requestId: string, allow: boolean) => {
      if (sessionId) {
        respondToControl(sessionId, requestId, allow);
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
