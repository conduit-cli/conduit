// Theme system for the web interface
// Themes are loaded dynamically from the backend to support VS Code themes

export interface ThemeColors {
  // Background layers
  bgTerminal: string;
  bgBase: string;
  bgSurface: string;
  bgElevated: string;
  bgHighlight: string;
  markdownCodeBg: string;
  markdownInlineCodeBg: string;

  // Text hierarchy
  textBright: string;
  textPrimary: string;
  textSecondary: string;
  textMuted: string;
  textFaint: string;

  // Accent colors
  accentPrimary: string;
  accentSecondary: string;
  accentSuccess: string;
  accentWarning: string;
  accentError: string;

  // Agent colors
  agentClaude: string;
  agentCodex: string;

  // PR state colors
  prOpenBg: string;
  prMergedBg: string;
  prClosedBg: string;
  prDraftBg: string;
  prUnknownBg: string;

  // Border colors
  borderDefault: string;
  borderFocused: string;
  borderDimmed: string;

  // Diff colors
  diffAdd: string;
  diffRemove: string;
}

export interface Theme {
  name: string;
  displayName: string;
  isLight: boolean;
  colors: ThemeColors;
}

export interface ThemeInfo {
  name: string;
  displayName: string;
  isLight: boolean;
  source: 'builtin' | 'vscode' | 'toml' | 'custom';
}

export interface ThemeListResponse {
  themes: ThemeInfo[];
  current: string;
}

// Apply a theme by setting CSS custom properties on :root
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  const { colors } = theme;

  // Background layers
  root.style.setProperty('--color-background', colors.bgBase);
  root.style.setProperty('--color-surface', colors.bgSurface);
  root.style.setProperty('--color-surface-elevated', colors.bgElevated);
  root.style.setProperty('--color-bg-highlight', colors.bgHighlight);
  root.style.setProperty('--color-markdown-code-bg', colors.markdownCodeBg);
  root.style.setProperty('--color-markdown-inline-code-bg', colors.markdownInlineCodeBg);

  // Text hierarchy
  root.style.setProperty('--color-text', colors.textPrimary);
  root.style.setProperty('--color-text-bright', colors.textBright);
  root.style.setProperty('--color-text-muted', colors.textMuted);
  root.style.setProperty('--color-text-secondary', colors.textSecondary);
  root.style.setProperty('--color-text-faint', colors.textFaint);

  // Accent colors
  root.style.setProperty('--color-accent', colors.accentPrimary);
  root.style.setProperty('--color-accent-hover', colors.accentSecondary);
  root.style.setProperty('--color-success', colors.accentSuccess);
  root.style.setProperty('--color-warning', colors.accentWarning);
  root.style.setProperty('--color-error', colors.accentError);

  // Agent colors
  root.style.setProperty('--color-agent-claude', colors.agentClaude);
  root.style.setProperty('--color-agent-codex', colors.agentCodex);

  // PR state colors
  root.style.setProperty('--color-pr-open', colors.prOpenBg);
  root.style.setProperty('--color-pr-merged', colors.prMergedBg);
  root.style.setProperty('--color-pr-closed', colors.prClosedBg);
  root.style.setProperty('--color-pr-draft', colors.prDraftBg);
  root.style.setProperty('--color-pr-unknown', colors.prUnknownBg);

  // Border colors
  root.style.setProperty('--color-border', colors.borderDefault);
  root.style.setProperty('--color-border-subtle', colors.borderDimmed);
  root.style.setProperty('--color-border-focused', colors.borderFocused);

  // Diff colors
  root.style.setProperty('--color-diff-add', colors.diffAdd);
  root.style.setProperty('--color-diff-remove', colors.diffRemove);

  // Scrollbar colors (derived from theme)
  root.style.setProperty('--color-scrollbar-thumb', colors.bgElevated);
  root.style.setProperty('--color-scrollbar-thumb-hover', colors.bgHighlight);

  // Set color scheme for native elements
  root.style.setProperty('color-scheme', theme.isLight ? 'light' : 'dark');
}

// Storage key for theme preference
const THEME_STORAGE_KEY = 'conduit-theme';

// Save theme preference to localStorage
export function saveThemePreference(themeName: string): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, themeName);
  } catch {
    // localStorage might be unavailable
  }
}

// Load theme preference from localStorage
export function loadThemePreference(): string | null {
  try {
    return localStorage.getItem(THEME_STORAGE_KEY);
  } catch {
    return null;
  }
}
