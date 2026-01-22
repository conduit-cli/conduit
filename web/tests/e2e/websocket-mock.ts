import type { Page } from '@playwright/test';

export async function installMockWebSocket(page: Page) {
  await page.addInitScript(() => {
    const instances: Array<{
      onopen?: () => void;
      onclose?: () => void;
      onmessage?: (event: { data: string }) => void;
      onerror?: (event: unknown) => void;
      readyState: number;
    }> = [];

    class MockWebSocket {
      static CONNECTING = 0;
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;

      readyState = MockWebSocket.CONNECTING;
      onopen?: () => void;
      onclose?: () => void;
      onmessage?: (event: { data: string }) => void;
      onerror?: (event: unknown) => void;
      private listeners: Map<string, Set<(event: unknown) => void>> = new Map();

      constructor() {
        instances.push(this);
        setTimeout(() => {
          this.readyState = MockWebSocket.OPEN;
          this.emit('open', undefined);
        }, 0);
      }

      addEventListener(type: string, listener: (event: unknown) => void) {
        if (!this.listeners.has(type)) {
          this.listeners.set(type, new Set());
        }
        this.listeners.get(type)?.add(listener);
      }

      removeEventListener(type: string, listener: (event: unknown) => void) {
        this.listeners.get(type)?.delete(listener);
      }

      send() {}

      close() {
        this.readyState = MockWebSocket.CLOSED;
        this.emit('close', undefined);
      }

      private emit(type: string, event: unknown) {
        const handler = (this as unknown as { [key: string]: ((event: unknown) => void) | undefined })[
          `on${type}`
        ];
        handler?.(event);
        this.listeners.get(type)?.forEach((listener) => listener(event));
      }
    }

    const NativeWebSocket = window.WebSocket;
    (window as unknown as { __mockWebSocket?: unknown }).__mockWebSocket = {
      instances,
      sendMessage(message: unknown) {
        const target = instances[instances.length - 1];
        if (!target || !target.onmessage) return;
        target.onmessage({ data: JSON.stringify(message) });
      },
    };

    window.WebSocket = new Proxy(NativeWebSocket, {
      construct(target, args) {
        const url = args[0];
        if (typeof url === 'string' && url.includes('/ws')) {
          return new MockWebSocket();
        }
        return new (target as unknown as { new (...innerArgs: unknown[]): WebSocket })(...args);
      },
    });
  });
}
