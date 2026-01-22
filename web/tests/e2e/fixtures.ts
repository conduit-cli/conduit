import type { Page, Route } from '@playwright/test';

export const sessionId = 'session-1';
export const workspaceId = 'workspace-1';

export const session = {
  id: sessionId,
  tab_index: 0,
  workspace_id: workspaceId,
  agent_type: 'codex',
  agent_mode: null,
  agent_session_id: 'agent-1',
  model: null,
  model_display_name: null,
  pr_number: null,
  created_at: '2026-01-22T14:27:46.876Z',
  title: 'Codex session',
};

export const repository = {
  id: 'repo-1',
  name: 'Live Jade',
  base_path: null,
  repository_url: null,
  workspace_mode: null,
  workspace_mode_effective: 'worktree',
  archive_delete_branch: false,
  archive_delete_branch_effective: false,
  archive_remote_prompt: false,
  archive_remote_prompt_effective: false,
  created_at: '2026-01-22T14:27:46.876Z',
  updated_at: '2026-01-22T14:27:46.876Z',
};

export const workspace = {
  id: workspaceId,
  repository_id: repository.id,
  name: 'Live Jade',
  branch: 'main',
  path: '/tmp/live-jade',
  created_at: '2026-01-22T14:27:46.876Z',
  last_accessed: '2026-01-22T14:27:46.876Z',
  is_default: true,
  archived_at: null,
};

export const uiState = {
  active_session_id: sessionId,
  tab_order: [sessionId],
  sidebar_open: true,
  last_workspace_id: workspaceId,
};

export const theme = {
  name: 'default-dark',
  displayName: 'Default Dark',
  isLight: false,
  colors: {
    bgTerminal: '#0f0f0f',
    bgBase: '#0f0f0f',
    bgSurface: '#161616',
    bgElevated: '#1c1c1c',
    bgHighlight: '#262626',
    markdownCodeBg: '#111111',
    markdownInlineCodeBg: '#1b1b1b',
    textBright: '#ffffff',
    textPrimary: '#e5e5e5',
    textSecondary: '#c7c7c7',
    textMuted: '#a3a3a3',
    textFaint: '#6b6b6b',
    accentPrimary: '#3b82f6',
    accentSecondary: '#2563eb',
    accentSuccess: '#22c55e',
    accentWarning: '#f59e0b',
    accentError: '#ef4444',
    agentClaude: '#f97316',
    agentCodex: '#22c55e',
    prOpenBg: '#1d4ed8',
    prMergedBg: '#16a34a',
    prClosedBg: '#dc2626',
    prDraftBg: '#f59e0b',
    prUnknownBg: '#6b7280',
    borderDefault: '#2a2a2a',
    borderFocused: '#3b82f6',
    borderDimmed: '#1f1f1f',
    diffAdd: '#166534',
    diffRemove: '#991b1b',
  },
};

export const themesResponse = {
  themes: [
    {
      name: theme.name,
      displayName: theme.displayName,
      isLight: theme.isLight,
      source: 'builtin',
    },
  ],
  current: theme.name,
};

export const historyEvents = [
  { role: 'user', content: 'Hello there.' },
  { role: 'assistant', content: 'Hi!' },
];

export const debugEntries = [
  {
    line: 1,
    entry_type: 'event_msg',
    status: 'INCLUDE',
    reason: 'event_msg user_message',
    raw: { type: 'event_msg', payload: { type: 'user_message', message: 'Hello there.' } },
  },
];

export const sessionEventsResponse = {
  events: historyEvents,
  total: historyEvents.length,
  offset: 0,
  limit: 200,
  debug_file: null,
  debug_entries: debugEntries,
};

function fulfillJson(route: Route, payload: unknown) {
  return route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify(payload),
  });
}

export async function mockApi(page: Page) {
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace('/api', '');

    if (path === '/bootstrap') {
      return fulfillJson(route, {
        ui_state: uiState,
        sessions: [session],
        workspaces: [workspace],
        active_session: session,
        active_workspace: workspace,
      });
    }

    if (path === '/repositories') {
      return fulfillJson(route, { repositories: [repository] });
    }

    if (path === '/workspaces') {
      return fulfillJson(route, { workspaces: [workspace] });
    }

    if (path === `/workspaces/${workspaceId}`) {
      return fulfillJson(route, workspace);
    }

    if (path === `/workspaces/${workspaceId}/status`) {
      return fulfillJson(route, {});
    }

    if (path === '/sessions') {
      return fulfillJson(route, { sessions: [session] });
    }

    if (path === `/sessions/${sessionId}`) {
      return fulfillJson(route, session);
    }

    if (path === `/sessions/${sessionId}/events`) {
      return fulfillJson(route, sessionEventsResponse);
    }

    if (path === `/sessions/${sessionId}/history`) {
      return fulfillJson(route, { history: [] });
    }

    if (path === `/sessions/${sessionId}/queue`) {
      return fulfillJson(route, { messages: [] });
    }

    if (path === '/ui/state') {
      return fulfillJson(route, uiState);
    }

    if (path === '/onboarding/base-dir') {
      return fulfillJson(route, { base_dir: null });
    }

    if (path === '/themes') {
      return fulfillJson(route, themesResponse);
    }

    if (path === '/themes/current') {
      return fulfillJson(route, theme);
    }

    if (path === '/models') {
      return fulfillJson(route, { groups: [] });
    }

    return fulfillJson(route, {});
  });
}
