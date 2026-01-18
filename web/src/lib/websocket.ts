// WebSocket client for real-time agent communication

import type { ClientMessage, ServerMessage, AgentEvent, ImageAttachment } from '../types';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

export interface WebSocketOptions {
  onConnect?: () => void;
  onDisconnect?: () => void;
  onError?: (error: Event) => void;
  onMessage?: (message: ServerMessage) => void;
  reconnectDelay?: number;
  maxReconnectAttempts?: number;
}

export class ConduitWebSocket {
  private ws: WebSocket | null = null;
  private url: string;
  private options: WebSocketOptions;
  private reconnectAttempts = 0;
  private reconnectTimeout: number | null = null;
  private pingInterval: number | null = null;
  private messageHandlers: Map<string, Set<(event: AgentEvent) => void>> = new Map();
  private activeSubscriptions: Set<string> = new Set();

  constructor(url: string, options: WebSocketOptions = {}) {
    this.url = url;
    this.options = {
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      ...options,
    };
  }

  updateOptions(options: WebSocketOptions): void {
    this.options = {
      ...this.options,
      ...options,
    };
  }

  connect(): void {
    if (this.ws?.readyState === WebSocket.OPEN) return;

    this.ws = new WebSocket(this.url);

    this.ws.onopen = () => {
      this.reconnectAttempts = 0;
      this.startPing();
      this.resubscribeAll();
      this.options.onConnect?.();
    };

    this.ws.onclose = () => {
      this.stopPing();
      this.options.onDisconnect?.();
      this.attemptReconnect();
    };

    this.ws.onerror = (error) => {
      this.options.onError?.(error);
    };

    this.ws.onmessage = (event) => {
      try {
        const message = JSON.parse(event.data) as ServerMessage;
        this.handleMessage(message);
        this.options.onMessage?.(message);
      } catch (e) {
        console.error('Failed to parse WebSocket message:', e);
      }
    };
  }

  disconnect(): void {
    this.stopPing();
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  send(message: ClientMessage): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    } else {
      console.warn('WebSocket not connected, cannot send message');
    }
  }

  // Subscribe to a session's events
  subscribe(sessionId: string, handler: (event: AgentEvent) => void): () => void {
    // Add handler to local map
    if (!this.messageHandlers.has(sessionId)) {
      this.messageHandlers.set(sessionId, new Set());
    }
    this.messageHandlers.get(sessionId)!.add(handler);

    // Send subscribe message
    this.subscribeIfConnected(sessionId);

    // Return unsubscribe function
    return () => {
      const handlers = this.messageHandlers.get(sessionId);
      if (handlers) {
        handlers.delete(handler);
        if (handlers.size === 0) {
          this.messageHandlers.delete(sessionId);
          if (this.activeSubscriptions.has(sessionId)) {
            this.activeSubscriptions.delete(sessionId);
            this.send({ type: 'unsubscribe', session_id: sessionId });
          }
        }
      }
    };
  }

  // Start a new session
  startSession(
    sessionId: string,
    prompt: string,
    workingDir: string,
    model?: string,
    hidden?: boolean,
    images?: ImageAttachment[]
  ): void {
    this.send({
      type: 'start_session',
      session_id: sessionId,
      prompt,
      working_dir: workingDir,
      model,
      hidden,
      images,
    });
  }

  // Send input to a running session
  sendInput(sessionId: string, input: string, hidden?: boolean, images?: ImageAttachment[]): void {
    this.send({ type: 'send_input', session_id: sessionId, input, hidden, images });
  }

  // Stop a session
  stopSession(sessionId: string): void {
    this.send({ type: 'stop_session', session_id: sessionId });
  }

  // Respond to a control request
  respondToControl(sessionId: string, requestId: string, response: unknown): void {
    this.send({
      type: 'respond_to_control',
      session_id: sessionId,
      request_id: requestId,
      response,
    });
  }

  private handleMessage(message: ServerMessage): void {
    if (message.type === 'agent_event') {
      const handlers = this.messageHandlers.get(message.session_id);
      if (handlers) {
        handlers.forEach((handler) => handler(message.event));
      }
    }
  }

  private isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  private subscribeIfConnected(sessionId: string): void {
    if (!this.isConnected() || this.activeSubscriptions.has(sessionId)) {
      return;
    }
    this.send({ type: 'subscribe', session_id: sessionId });
    this.activeSubscriptions.add(sessionId);
  }

  private resubscribeAll(): void {
    if (!this.isConnected()) {
      return;
    }
    this.activeSubscriptions.clear();
    for (const sessionId of this.messageHandlers.keys()) {
      this.send({ type: 'subscribe', session_id: sessionId });
      this.activeSubscriptions.add(sessionId);
    }
  }

  private startPing(): void {
    this.pingInterval = window.setInterval(() => {
      this.send({ type: 'ping' });
    }, 30000);
  }

  private stopPing(): void {
    if (this.pingInterval) {
      clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }

  private attemptReconnect(): void {
    if (this.reconnectAttempts >= (this.options.maxReconnectAttempts ?? 5)) {
      console.log('Max reconnect attempts reached');
      return;
    }

    this.reconnectAttempts++;
    const delay = (this.options.reconnectDelay ?? 1000) * Math.pow(2, this.reconnectAttempts - 1);

    this.reconnectTimeout = window.setTimeout(() => {
      console.log(`Reconnecting (attempt ${this.reconnectAttempts})...`);
      this.connect();
    }, delay);
  }
}

// Global WebSocket instance
let globalWs: ConduitWebSocket | null = null;

export function getWebSocket(options?: WebSocketOptions): ConduitWebSocket {
  if (!globalWs) {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    globalWs = new ConduitWebSocket(wsUrl, options);
  } else if (options) {
    globalWs.updateOptions(options);
  }
  return globalWs;
}
