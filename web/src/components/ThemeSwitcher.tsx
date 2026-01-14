import { useState, useRef, useEffect } from 'react';
import { Palette, Sun, Moon, Check, ChevronDown } from 'lucide-react';
import { useTheme } from '../hooks';
import { cn } from '../lib/cn';

export function ThemeSwitcher() {
  const { themes, currentThemeName, isLight, setTheme, toggleTheme, isLoading } = useTheme();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Close dropdown on escape
  useEffect(() => {
    function handleEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    }

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, []);

  // Group themes by source
  const builtinThemes = themes.filter((t) => t.source === 'builtin');
  const vscodeThemes = themes.filter((t) => t.source === 'vscode');
  const customThemes = themes.filter((t) => t.source === 'toml' || t.source === 'custom');

  const handleThemeSelect = (name: string) => {
    setTheme(name);
    setIsOpen(false);
  };

  return (
    <div className="relative" ref={dropdownRef}>
      {/* Theme toggle button */}
      <div className="flex items-center gap-1">
        {/* Quick toggle light/dark */}
        <button
          onClick={toggleTheme}
          className="flex size-8 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          aria-label={isLight ? 'Switch to dark theme' : 'Switch to light theme'}
        >
          {isLight ? <Moon className="size-4" /> : <Sun className="size-4" />}
        </button>

        {/* Theme selector dropdown */}
        <button
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-1 rounded-md px-2 py-1.5 text-sm text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          aria-label="Select theme"
          aria-expanded={isOpen}
          aria-haspopup="listbox"
        >
          <Palette className="size-4" />
          <ChevronDown className={cn('size-3 transition-transform', isOpen && 'rotate-180')} />
        </button>
      </div>

      {/* Dropdown menu */}
      {isOpen && (
        <div className="absolute right-0 top-full z-50 mt-1 w-64 overflow-hidden rounded-lg border border-border bg-surface shadow-lg">
          <div className="max-h-80 overflow-y-auto p-1">
            {isLoading ? (
              <div className="px-3 py-2 text-sm text-text-muted">Loading themes...</div>
            ) : (
              <>
                {/* Built-in themes */}
                {builtinThemes.length > 0 && (
                  <ThemeGroup
                    label="Built-in"
                    themes={builtinThemes}
                    currentThemeName={currentThemeName}
                    onSelect={handleThemeSelect}
                  />
                )}

                {/* VS Code themes */}
                {vscodeThemes.length > 0 && (
                  <ThemeGroup
                    label="VS Code"
                    themes={vscodeThemes}
                    currentThemeName={currentThemeName}
                    onSelect={handleThemeSelect}
                  />
                )}

                {/* Custom themes */}
                {customThemes.length > 0 && (
                  <ThemeGroup
                    label="Custom"
                    themes={customThemes}
                    currentThemeName={currentThemeName}
                    onSelect={handleThemeSelect}
                  />
                )}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

interface ThemeGroupProps {
  label: string;
  themes: Array<{ name: string; displayName: string; isLight: boolean }>;
  currentThemeName: string | null;
  onSelect: (name: string) => void;
}

function ThemeGroup({ label, themes, currentThemeName, onSelect }: ThemeGroupProps) {
  return (
    <div className="mb-1">
      <div className="px-2 py-1 text-xs font-medium text-text-muted">{label}</div>
      {themes.map((theme) => (
        <button
          key={theme.name}
          onClick={() => onSelect(theme.name)}
          className={cn(
            'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors',
            theme.name === currentThemeName
              ? 'bg-accent/10 text-accent'
              : 'text-text hover:bg-surface-elevated'
          )}
          role="option"
          aria-selected={theme.name === currentThemeName}
        >
          {/* Light/dark indicator */}
          <span className="flex size-4 items-center justify-center">
            {theme.isLight ? (
              <Sun className="size-3 text-warning" />
            ) : (
              <Moon className="size-3 text-accent" />
            )}
          </span>

          {/* Theme name */}
          <span className="flex-1 truncate">{theme.displayName}</span>

          {/* Check mark for current */}
          {theme.name === currentThemeName && <Check className="size-4 text-accent" />}
        </button>
      ))}
    </div>
  );
}
