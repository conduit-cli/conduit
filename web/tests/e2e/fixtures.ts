import type { Page, Route } from '@playwright/test';
import type { ListModelsResponse, QueuedMessage, Session, UiState } from '../../src/types';

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
  model_invalid: false,
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
  syntax: {
    markdownBlock: {
      foreground: '#e5e5e5',
      background: '#0f0f0f',
      caret: '#3b82f6',
      selection: '#262626',
      selectionForeground: '#ffffff',
      gutter: '#161616',
      gutterForeground: '#a3a3a3',
      lineHighlight: '#262626',
      accent: '#2563eb',
      tokens: {
        comment: '#a3a3a3',
        keyword: '#3b82f6',
        type: '#2563eb',
        function: '#ffffff',
        string: '#22c55e',
        constant: '#f59e0b',
        property: '#2563eb',
        parameter: '#ffffff',
        punctuation: '#c7c7c7',
        invalid: '#ef4444',
      },
      fontStyles: {
        keywordBold: true,
        typeBold: true,
        invalidUnderline: true,
      },
    },
    markdownInline: {
      foreground: '#ffffff',
      background: '#0f0f0f',
      caret: '#3b82f6',
      selection: '#262626',
      selectionForeground: '#ffffff',
      gutter: '#161616',
      gutterForeground: '#a3a3a3',
      lineHighlight: '#262626',
      accent: '#2563eb',
      tokens: {
        comment: '#a3a3a3',
        keyword: '#3b82f6',
        type: '#2563eb',
        function: '#ffffff',
        string: '#22c55e',
        constant: '#f59e0b',
        property: '#2563eb',
        parameter: '#ffffff',
        punctuation: '#c7c7c7',
        invalid: '#ef4444',
      },
      fontStyles: {
        keywordBold: true,
        typeBold: true,
        invalidUnderline: true,
      },
    },
    sourceFile: {
      foreground: '#e5e5e5',
      background: '#0f0f0f',
      caret: '#3b82f6',
      selection: '#262626',
      selectionForeground: '#ffffff',
      gutter: '#161616',
      gutterForeground: '#a3a3a3',
      lineHighlight: '#262626',
      accent: '#2563eb',
      tokens: {
        comment: '#a3a3a3',
        keyword: '#3b82f6',
        type: '#2563eb',
        function: '#2563eb',
        string: '#22c55e',
        constant: '#f59e0b',
        property: '#2563eb',
        parameter: '#ffffff',
        punctuation: '#c7c7c7',
        invalid: '#ef4444',
      },
      fontStyles: {
        keywordBold: true,
        typeBold: true,
        invalidUnderline: true,
      },
    },
  },
};

export const alternateTheme = {
  ...theme,
  name: 'default-light',
  displayName: 'Default Light',
  isLight: true,
  colors: {
    ...theme.colors,
    bgBase: '#f6f6f6',
    bgSurface: '#efefef',
    bgElevated: '#e6e6e6',
    bgHighlight: '#d9d9d9',
    textBright: '#111827',
    textPrimary: '#1f2937',
    textSecondary: '#374151',
    textMuted: '#6b7280',
    accentPrimary: '#dc2626',
    accentSecondary: '#b91c1c',
    accentSuccess: '#059669',
    accentWarning: '#d97706',
    accentError: '#991b1b',
    borderDefault: '#d1d5db',
    borderDimmed: '#e5e7eb',
  },
  syntax: {
    markdownBlock: {
      ...theme.syntax.markdownBlock,
      foreground: '#1f2937',
      background: '#f6f6f6',
      selection: '#d9d9d9',
      selectionForeground: '#111827',
      gutter: '#efefef',
      gutterForeground: '#6b7280',
      lineHighlight: '#d9d9d9',
      caret: '#dc2626',
      accent: '#b91c1c',
      tokens: {
        ...theme.syntax.markdownBlock.tokens,
        keyword: '#dc2626',
        type: '#b91c1c',
        function: '#111827',
        string: '#059669',
        constant: '#d97706',
        property: '#b91c1c',
        punctuation: '#374151',
        invalid: '#991b1b',
      },
    },
    markdownInline: {
      ...theme.syntax.markdownInline,
      foreground: '#111827',
      background: '#f6f6f6',
      selection: '#d9d9d9',
      selectionForeground: '#111827',
      gutter: '#efefef',
      gutterForeground: '#6b7280',
      lineHighlight: '#d9d9d9',
      caret: '#dc2626',
      accent: '#b91c1c',
      tokens: {
        ...theme.syntax.markdownInline.tokens,
        keyword: '#dc2626',
        type: '#b91c1c',
        function: '#111827',
        string: '#059669',
        constant: '#d97706',
        property: '#b91c1c',
        punctuation: '#374151',
        invalid: '#991b1b',
      },
    },
    sourceFile: {
      ...theme.syntax.sourceFile,
      foreground: '#1f2937',
      background: '#f6f6f6',
      selection: '#d9d9d9',
      selectionForeground: '#111827',
      gutter: '#efefef',
      gutterForeground: '#6b7280',
      lineHighlight: '#d9d9d9',
      caret: '#dc2626',
      accent: '#b91c1c',
      tokens: {
        ...theme.syntax.sourceFile.tokens,
        keyword: '#dc2626',
        type: '#b91c1c',
        function: '#b91c1c',
        string: '#059669',
        constant: '#d97706',
        property: '#b91c1c',
        punctuation: '#374151',
        invalid: '#991b1b',
      },
    },
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
    {
      name: alternateTheme.name,
      displayName: alternateTheme.displayName,
      isLight: alternateTheme.isLight,
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

export const settingsResponse = {
  items: [
    {
      id: 'projects_directory',
      title: 'Projects Directory',
      description: 'Where Conduit scans for local git projects',
      value: 'Not set',
    },
    {
      id: 'default_model',
      title: 'Default Model',
      description: 'Agent + model used for new sessions',
      value: 'Codex: GPT-5.4',
    },
    {
      id: 'enabled_providers',
      title: 'Enabled Providers',
      description: 'Providers shown in model selection',
      value: 'Claude, Codex, Gemini, OpenCode, Pi',
    },
    {
      id: 'theme',
      title: 'Theme',
      description: 'Active color theme',
      value: theme.displayName,
    },
    {
      id: 'workspace_defaults',
      title: 'Workspace Defaults',
      description: 'Defaults applied when a repo has no override',
      value: 'worktree, delete branch on, remote prompt on',
    },
  ],
};

export const modelsResponse: ListModelsResponse = {
  groups: [
    {
      agent_type: 'pi',
      section_title: 'Pi',
      icon: 'π',
      models: [
        {
          id: 'anthropic/claude-haiku-4-5',
          display_name: 'anthropic/claude-haiku-4-5',
          description: 'Pi model from anthropic',
          is_default: true,
          agent_type: 'pi',
          context_window: 200000,
        },
      ],
    },
  ],
};

function fulfillJson(route: Route, payload: unknown) {
  return route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify(payload),
  });
}

export async function mockApi(
  page: Page,
  overrides: {
    session?: Session;
    uiState?: UiState;
    queueMessages?: QueuedMessage[];
    sessionEvents?: typeof sessionEventsResponse;
    settings?: typeof settingsResponse;
    models?: ListModelsResponse;
  } = {}
) {
  const effectiveSession = overrides.session ?? session;
  const effectiveUiState = overrides.uiState ?? {
    ...uiState,
    active_session_id: effectiveSession.id,
    tab_order: [effectiveSession.id],
  };
  const queueMessages = overrides.queueMessages ?? [];
  const effectiveSessionEvents = overrides.sessionEvents ?? sessionEventsResponse;
  const effectiveSettings = overrides.settings ?? settingsResponse;
  const effectiveModels = overrides.models ?? modelsResponse;
  let currentTheme = theme;

  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace('/api', '');

    if (path === '/bootstrap') {
      return fulfillJson(route, {
        ui_state: effectiveUiState,
        sessions: [effectiveSession],
        workspaces: [workspace],
        active_session: effectiveSession,
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
      return fulfillJson(route, { sessions: [effectiveSession] });
    }

    if (path === `/sessions/${effectiveSession.id}`) {
      return fulfillJson(route, effectiveSession);
    }

    if (path === `/sessions/${effectiveSession.id}/events`) {
      return fulfillJson(route, effectiveSessionEvents);
    }

    if (path === `/sessions/${effectiveSession.id}/history`) {
      return fulfillJson(route, { history: [] });
    }

    if (path === `/sessions/${effectiveSession.id}/queue`) {
      if (route.request().method() === 'GET') {
        return fulfillJson(route, { messages: queueMessages });
      }
      return fulfillJson(route, { messages: queueMessages });
    }

    if (path.startsWith(`/sessions/${effectiveSession.id}/queue/`)) {
      if (route.request().method() === 'DELETE') {
        queueMessages.splice(0, queueMessages.length);
        return fulfillJson(route, { ok: true });
      }
      return fulfillJson(route, { ok: true });
    }

    if (path === '/ui/state') {
      return fulfillJson(route, effectiveUiState);
    }

    if (path === '/onboarding/base-dir') {
      return fulfillJson(route, { base_dir: null });
    }

    if (path === '/themes') {
      return fulfillJson(route, themesResponse);
    }

    if (path === '/themes/current') {
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON() as { name?: string } | null;
        currentTheme = body?.name === alternateTheme.name ? alternateTheme : theme;
      }
      return fulfillJson(route, currentTheme);
    }

    if (path === '/settings') {
      return fulfillJson(route, effectiveSettings);
    }

    if (path === '/models') {
      return fulfillJson(route, effectiveModels);
    }

    if (path === '/models/default') {
      return route.fulfill({ status: 204, body: '' });
    }

    return fulfillJson(route, {});
  });
}
