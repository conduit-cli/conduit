import { test, expect } from '@playwright/test';
import { installMockWebSocket } from './websocket-mock';
import { mockApi, session } from './fixtures';

const piSession = {
  ...session,
  agent_type: 'pi' as const,
  title: 'Identify current AI model',
  model: 'anthropic/claude-haiku-4-5',
  model_display_name: 'anthropic/claude-haiku-4-5',
};

test.beforeEach(async ({ page }) => {
  await mockApi(page, { session: piSession });
  await installMockWebSocket(page);
});

test('renders Pi sessions with Pi agent labels instead of Gemini', async ({ page }) => {
  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await expect(page.getByPlaceholder('Type a message...')).toBeVisible();

  await expect(page.getByRole('banner').getByText('Pi', { exact: true })).toBeVisible();
  await expect(page.getByText('· anthropic/claude-haiku-4-5')).toBeVisible();
  await expect(page.getByText('anthropic/claude-haiku-4-5 · Pi')).toBeVisible();
  await expect(page.getByText('Gemini CLI')).toHaveCount(0);
});
