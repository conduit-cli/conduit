import { test, expect } from '@playwright/test';
import { mockApi, sessionId } from './fixtures';
import { installMockWebSocket } from './websocket-mock';

test.beforeEach(async ({ page }) => {
  await mockApi(page);
  await installMockWebSocket(page);
});

test('processing indicators stay hidden on TurnStarted', async ({ page }) => {
  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await expect(page.getByPlaceholder('Type a message...')).toBeVisible();

  await page.evaluate((id) => {
    const ws = (window as unknown as { __mockWebSocket?: { sendMessage: (msg: unknown) => void } })
      .__mockWebSocket;
    ws?.sendMessage({ type: 'agent_event', session_id: id, event: { type: 'TurnStarted' } });
  }, sessionId);

  await expect(page.getByText('Processing...')).toHaveCount(0);
  await expect(page.locator('[aria-label=\"Processing\"]')).toHaveCount(0);
});
