import { createHighlighter, type Highlighter, type BundledTheme } from 'shiki';

let highlighterPromise: Promise<Highlighter> | null = null;

const SUPPORTED_LANGUAGES = [
  'typescript',
  'javascript',
  'python',
  'rust',
  'go',
  'bash',
  'shell',
  'json',
  'yaml',
  'toml',
  'markdown',
  'html',
  'css',
  'sql',
  'diff',
  'tsx',
  'jsx',
] as const;

export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const THEME_MAP: Record<'dark' | 'light', BundledTheme> = {
  dark: 'catppuccin-mocha',
  light: 'catppuccin-latte',
};

export async function getHighlighter(): Promise<Highlighter> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: Object.values(THEME_MAP),
      langs: [...SUPPORTED_LANGUAGES],
    });
  }
  return highlighterPromise;
}

export function normalizeLanguage(lang: string | undefined): string {
  if (!lang) return 'text';

  const normalized = lang.toLowerCase().trim();

  // Common aliases
  const aliases: Record<string, string> = {
    'ts': 'typescript',
    'js': 'javascript',
    'py': 'python',
    'rs': 'rust',
    'sh': 'bash',
    'zsh': 'bash',
    'yml': 'yaml',
    'md': 'markdown',
    'htm': 'html',
  };

  const mapped = aliases[normalized] || normalized;

  // Check if language is supported
  if (SUPPORTED_LANGUAGES.includes(mapped as SupportedLanguage)) {
    return mapped;
  }

  return 'text';
}

export function isSupportedLanguage(lang: string): boolean {
  return SUPPORTED_LANGUAGES.includes(normalizeLanguage(lang) as SupportedLanguage);
}
