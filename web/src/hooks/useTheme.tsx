// Theme management hook and context

import {
  createContext,
  useContext,
  useEffect,
  useState,
  useCallback,
  type ReactNode,
} from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as api from '../lib/api';
import {
  applyTheme,
  saveThemePreference,
  loadThemePreference,
  type Theme,
  type ThemeInfo,
} from '../lib/themes';

interface ThemeContextValue {
  // Current theme
  currentTheme: Theme | null;
  currentThemeName: string | null;
  isLight: boolean;

  // Available themes
  themes: ThemeInfo[];
  isLoading: boolean;

  // Actions
  setTheme: (name: string) => void;
  toggleTheme: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

// Query keys
const themeKeys = {
  list: ['themes'] as const,
  current: ['themes', 'current'] as const,
};

export function ThemeProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [localThemeName, setLocalThemeName] = useState<string | null>(null);

  // Fetch available themes
  const { data: themesData, isLoading: isLoadingThemes } = useQuery({
    queryKey: themeKeys.list,
    queryFn: api.getThemes,
    staleTime: 60000, // Cache for 1 minute
  });

  // Fetch current theme
  const { data: currentTheme, isLoading: isLoadingCurrent } = useQuery({
    queryKey: themeKeys.current,
    queryFn: api.getCurrentTheme,
    staleTime: 30000,
  });

  // Set theme mutation
  const setThemeMutation = useMutation({
    mutationFn: api.setTheme,
    onSuccess: (theme) => {
      queryClient.setQueryData(themeKeys.current, theme);
      saveThemePreference(theme.name);
      setLocalThemeName(theme.name);
    },
  });

  // Apply theme when it changes
  useEffect(() => {
    if (currentTheme) {
      applyTheme(currentTheme);
      setLocalThemeName(currentTheme.name);
    }
  }, [currentTheme]);

  // Load saved preference on mount
  useEffect(() => {
    const savedTheme = loadThemePreference();
    if (savedTheme && savedTheme !== currentTheme?.name) {
      setThemeMutation.mutate(savedTheme);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const setTheme = useCallback(
    (name: string) => {
      setThemeMutation.mutate(name);
    },
    [setThemeMutation]
  );

  const toggleTheme = useCallback(() => {
    if (!currentTheme) return;

    // Find the opposite theme (light <-> dark)
    const targetIsLight = !currentTheme.isLight;
    const themes = themesData?.themes || [];

    // Try to find a matching theme variant
    let targetTheme: string | null = null;

    // If current theme is catppuccin-mocha, switch to catppuccin-latte, etc.
    const baseName = currentTheme.name.replace(/-dark$|-light$|-mocha$|-latte$/, '');
    const possibleTargets = themes.filter(
      (t) => t.name.startsWith(baseName) && t.isLight === targetIsLight
    );

    if (possibleTargets.length > 0) {
      targetTheme = possibleTargets[0].name;
    } else {
      // Fall back to default light/dark
      targetTheme = targetIsLight ? 'default-light' : 'default-dark';
    }

    if (targetTheme) {
      setTheme(targetTheme);
    }
  }, [currentTheme, themesData, setTheme]);

  const value: ThemeContextValue = {
    currentTheme: currentTheme || null,
    currentThemeName: localThemeName,
    isLight: currentTheme?.isLight ?? false,
    themes: themesData?.themes || [],
    isLoading: isLoadingThemes || isLoadingCurrent,
    setTheme,
    toggleTheme,
  };

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}
