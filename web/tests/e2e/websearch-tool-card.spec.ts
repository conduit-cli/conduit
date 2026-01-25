import { test, expect } from '@playwright/test';
import { installMockWebSocket } from './websocket-mock';
import { mockApi, session } from './fixtures';

const opencodeSession = {
  ...session,
  agent_type: 'opencode',
  model: 'opencode/glm-4.7',
  model_display_name: 'opencode/glm-4.7',
};

const output = `# Zen
Curated list of models provided by OpenCode.

OpenCode Zen is a list of tested and verified models provided by the OpenCode team.`;

test.beforeEach(async ({ page }) => {
  await mockApi(page, { session: opencodeSession });
  await installMockWebSocket(page);
});

test('websearch tool renders with web search card styling', async ({ page }) => {
  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await expect(page.getByPlaceholder('Type a message...')).toBeVisible();

  await page.evaluate((payload) => {
    const ws = (window as unknown as { __mockWebSocket?: { sendMessage: (msg: unknown) => void } })
      .__mockWebSocket;
    ws?.sendMessage({
      type: 'agent_event',
      session_id: payload.sessionId,
      event: {
        type: 'ToolStarted',
        tool_name: 'websearch',
        tool_id: 'tool-1',
        arguments: { query: 'OpenCode Zen' },
      },
    });
    ws?.sendMessage({
      type: 'agent_event',
      session_id: payload.sessionId,
      event: {
        type: 'ToolCompleted',
        tool_id: 'tool-1',
        success: true,
        result: payload.output,
        error: null,
      },
    });
  }, { sessionId: opencodeSession.id, output });

  await expect(page.getByText('Web search')).toBeVisible();
  await expect(page.getByText('Query: OpenCode Zen')).toBeVisible();
  await expect(page.getByText('Curated list of models provided by OpenCode.')).toBeVisible();
});
