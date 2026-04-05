import { test, expect } from '@playwright/test';
import { installMockWebSocket } from './websocket-mock';
import { mockApi, session } from './fixtures';

const busyPiSession = {
  ...session,
  agent_type: 'pi' as const,
  model: 'anthropic/claude-haiku-4-5',
  model_display_name: 'anthropic/claude-haiku-4-5',
  agent_session_id: 'pi-live-session',
  title: 'Busy Pi session',
};

test.beforeEach(async ({ page }) => {
  await mockApi(page, { session: busyPiSession });
  await installMockWebSocket(page);
});

test('settings can open default model picker even when the active session model is locked', async ({ page }) => {
  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await expect(page.getByPlaceholder('Type a message...')).toBeVisible();

  await page.getByLabel('Settings').click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();

  await page.getByRole('button', { name: /Default Model/i }).click();

  await expect(page.getByRole('heading', { name: 'Default Model' })).toBeVisible();
  await expect(
    page.getByRole('button', { name: /anthropic\/claude-haiku-4-5/i })
  ).toBeVisible();
  await expect(
    page.getByText('Wait for the current response to finish before changing the model.')
  ).toHaveCount(0);
});
