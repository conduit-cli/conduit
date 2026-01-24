import { test, expect } from '@playwright/test';
import { mockApi, session, sessionId } from './fixtures';
import { installMockWebSocket } from './websocket-mock';

test.beforeEach(async ({ page }) => {
  await installMockWebSocket(page);
});

test('auto-drains queued messages when idle', async ({ page }) => {
  const queuedMessage = {
    id: 'queued-1',
    mode: 'follow-up',
    text: 'List the top-level files.',
    images: [],
    created_at: '2026-01-22T14:27:46.876Z',
  };
  const queueMessages = [queuedMessage];

  const opencodeSession = {
    ...session,
    agent_type: 'opencode',
    model: 'opencode/glm-4.7',
    model_display_name: 'opencode/glm-4.7',
  };

  await mockApi(page, { session: opencodeSession, queueMessages });

  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await page.waitForResponse('**/api/repositories');
  await expect(page.locator('textarea[data-chat-input="true"]')).toBeVisible();

  const deleteRequest = page.waitForRequest((req) =>
    req.url().includes(`/api/sessions/${sessionId}/queue/${queuedMessage.id}`) &&
    req.method() === 'DELETE'
  );

  await page.waitForResponse(`**/api/sessions/${sessionId}/queue`);
  await deleteRequest;

  await expect(page.getByText('Queue (1)')).toHaveCount(0);
});
