import { test, expect, type Page } from '@playwright/test';
import { mockApi, sessionEventsResponse } from './fixtures';
import { installMockWebSocket } from './websocket-mock';

async function installClipboardFallbackProbe(page: Page) {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      value: undefined,
      configurable: true,
    });

    (window as unknown as { __copiedText?: string }).__copiedText = '';
    document.execCommand = (command: string) => {
      if (command !== 'copy') return false;
      const active = document.activeElement as HTMLTextAreaElement | null;
      (window as unknown as { __copiedText?: string }).__copiedText = active?.value ?? '';
      return true;
    };
  });
}

test.beforeEach(async ({ page }) => {
  await mockApi(page);
  await installMockWebSocket(page);
  await installClipboardFallbackProbe(page);
});

test('raw events copy uses fallback clipboard', async ({ page }) => {
  await page.goto('/');
  await page.waitForResponse('**/api/bootstrap');
  await expect(page.getByPlaceholder('Type a message...')).toBeVisible();
  await page.getByRole('button', { name: /raw events/i }).click();

  const copyButton = page.locator('button[title="Copy JSON"]').first();
  await expect(copyButton).toBeVisible();
  await copyButton.click();

  const copiedText = await page.evaluate(
    () => (window as unknown as { __copiedText?: string }).__copiedText ?? ''
  );

  const expected = JSON.stringify(
    {
      type: 'Raw',
      data: {
        type: 'history_load',
        file: null,
        total_entries: sessionEventsResponse.debug_entries.length,
        included: sessionEventsResponse.debug_entries.filter((entry) => entry.status === 'INCLUDE')
          .length,
        skipped: sessionEventsResponse.debug_entries.filter((entry) => entry.status === 'SKIP')
          .length,
      },
    },
    null,
    2
  );

  expect(copiedText).toBe(expected);
});
