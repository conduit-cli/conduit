# Keyboard Shortcuts Quick Reference

All keyboard shortcuts in one place. Use `conduit debug-keys` to verify how your terminal reports key combinations.

## Global Shortcuts

These work in all modes unless overridden by context-specific bindings.

| Shortcut | Action |
|----------|--------|
| `Ctrl+Q` | Quit Conduit |
| `Ctrl+T` | Toggle sidebar |
| `Ctrl+N` | New project / Open project picker |
| `Ctrl+P` | Open command palette |
| `Ctrl+O` | Show model selector |
| `Ctrl+G` | Toggle view mode (Chat / Raw Events) |
| `Ctrl+4` | Toggle Build/Plan mode (Ctrl+\) |
| `Ctrl+Alt+P` | Open/create pull request |
| `Alt+T` | Show theme picker |
| `Alt+P` | Toggle metrics display |
| `Alt+I` | Open session import |
| `Alt+G` | Dump debug state |
| `Alt+Tab` | Next tab |
| `Alt+Shift+Tab` | Previous tab |
| `Alt+1` - `Alt+9` | Switch to tab 1-9 |
| `Alt+Shift+W` | Close current tab |
| `Alt+Shift+C` | Copy workspace path |
| `Alt+C` | Copy selection |

### Readline-Style Editing

| Shortcut | Action |
|----------|--------|
| `Ctrl+A` | Move cursor to start |
| `Ctrl+E` | Move cursor to end |
| `Ctrl+F` | Move cursor right |
| `Ctrl+B` | Move cursor left |
| `Alt+F` | Move word right |
| `Alt+B` | Move word left |
| `Ctrl+U` | Delete to start |
| `Ctrl+K` | Delete to end |
| `Ctrl+W` | Delete word back |
| `Alt+D` | Delete word forward |
| `Ctrl+D` | Delete character |
| `Ctrl+H` | Backspace |
| `Ctrl+J` | Insert newline |

### Scrolling (Global)

| Shortcut | Action |
|----------|--------|
| `Ctrl+Up` | Scroll up |
| `Ctrl+Down` | Scroll down |
| `Alt+Shift+J` | Scroll down |
| `Alt+Shift+K` | Scroll up |
| `Alt+Shift+F` | Page down |
| `Alt+Shift+B` | Page up |

## Chat Mode

| Shortcut | Action |
|----------|--------|
| `Enter` | Submit prompt |
| `Shift+Enter` | Insert newline |
| `Alt+Enter` | Insert newline |
| `Tab` | Toggle Build/Plan mode |
| `Page Up` | Scroll page up |
| `Page Down` | Scroll page down |
| `Esc` | Scroll to bottom |
| `Backspace` | Delete character |
| `Delete` | Delete forward |
| `Left` / `Right` | Move cursor |
| `Up` / `Down` | Move cursor (multiline) |
| `Home` / `End` | Start / end of line |

## Scrolling Mode

Entered when scrolling through chat history.

| Shortcut | Action |
|----------|--------|
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `Page Down` | Page down |
| `Page Up` | Page up |
| `g` / `Home` | Scroll to top |
| `G` / `End` | Scroll to bottom |
| `Esc` / `q` / `i` | Exit scrolling mode |

## Sidebar Mode

| Shortcut | Action |
|----------|--------|
| `j` / `Down` | Select next |
| `k` / `Up` | Select previous |
| `l` / `Right` / `Enter` | Expand or select |
| `h` / `Left` | Collapse |
| `r` | Add repository |
| `s` | Open settings |
| `x` | Archive or remove |
| `Tab` | Toggle Build/Plan mode |
| `Esc` | Exit sidebar |

## Dialog Mode

| Shortcut | Action |
|----------|--------|
| `Tab` / `Left` / `Right` | Toggle selection |
| `Enter` | Confirm |
| `Esc` | Cancel |
| `y` | Yes |
| `n` | No |
| `d` | Toggle details |

## Project Picker

| Shortcut | Action |
|----------|--------|
| `Down` / `Ctrl+J` | Select next |
| `Up` / `Ctrl+K` | Select previous |
| `Ctrl+F` | Page down |
| `Ctrl+B` | Page up |
| `Ctrl+A` | Add repository |
| `Enter` | Confirm selection |
| `Esc` | Cancel |

## Model Selector

| Shortcut | Action |
|----------|--------|
| `j` / `Down` | Select next |
| `k` / `Up` | Select previous |
| `Enter` | Confirm selection |
| `Esc` | Cancel |

## Command Palette

| Shortcut | Action |
|----------|--------|
| `Down` / `Ctrl+J` / `Ctrl+N` | Select next |
| `Up` / `Ctrl+K` / `Ctrl+P` | Select previous |
| `Enter` | Confirm selection |
| `Esc` | Cancel |
| `Backspace` | Delete character |

## Theme Picker

| Shortcut | Action |
|----------|--------|
| `Down` / `Ctrl+J` | Select next |
| `Up` / `Ctrl+K` | Select previous |
| `Enter` | Confirm selection |
| `Esc` | Cancel |

## Session Import

| Shortcut | Action |
|----------|--------|
| `j` / `Down` / `Ctrl+J` | Select next |
| `k` / `Up` / `Ctrl+K` | Select previous |
| `Ctrl+F` | Page down |
| `Ctrl+B` | Page up |
| `Tab` | Cycle import filter |
| `Enter` | Import selected session |
| `Esc` | Cancel |

## Raw Events View

| Shortcut | Action |
|----------|--------|
| `j` / `Down` | Select next event |
| `k` / `Up` | Select previous event |
| `l` / `Enter` / `Tab` | Toggle expand |
| `h` / `Esc` | Collapse |
| `e` | Toggle detail panel |
| `c` | Copy selected event |
| `Ctrl+J` | Scroll detail down |
| `Ctrl+K` | Scroll detail up |
| `Ctrl+F` | Detail page down |
| `Ctrl+B` | Detail page up |
| `g` | Detail scroll to top |
| `G` | Detail scroll to bottom |

## Key Notation

When customizing keybindings in `config.toml`:

| Notation | Meaning |
|----------|---------|
| `C-x` | Ctrl + x |
| `M-x` | Alt/Meta + x |
| `S-x` | Shift + x |
| `C-S-x` | Ctrl + Shift + x |
| `C-M-x` | Ctrl + Alt + x |

## Terminal Compatibility

Some key combinations may not work in all terminals:

- **Ctrl+\\** is often reported as `Ctrl+4` — use `conduit debug-keys` to check
- **Ctrl+Shift** combinations may not be recognized by all terminals
- **Alt** key behavior varies (some terminals use Escape prefix)

Run `conduit debug-keys` to see exactly what key events your terminal sends.
