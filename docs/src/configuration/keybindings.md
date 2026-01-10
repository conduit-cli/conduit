# Keybindings Configuration

Customize keyboard shortcuts in `~/.conduit/config.toml`.

## Key Notation

| Notation | Meaning |
|----------|---------|
| `C-x` | Ctrl + x |
| `M-x` | Alt/Meta + x |
| `S-x` | Shift + x |
| `C-S-x` | Ctrl + Shift + x |
| `C-M-x` | Ctrl + Alt + x |

## Contexts

Keybindings are organized by context:

- `[keys]` — Global (all modes)
- `[keys.chat]` — Chat input
- `[keys.scrolling]` — Scrolling mode
- `[keys.sidebar]` — Sidebar navigation
- `[keys.dialog]` — Dialog interactions
- `[keys.project_picker]` — Project picker
- `[keys.model_selector]` — Model selection

## Example Customization

```toml
# Global keybindings
[keys]
"C-s" = "Submit"           # Ctrl+S to submit
"M-q" = "Quit"             # Alt+Q to quit

# Chat-specific
[keys.chat]
"C-Enter" = "Submit"       # Ctrl+Enter to submit

# Sidebar
[keys.sidebar]
"a" = "AddRepository"      # Press 'a' to add repo
```

## Available Actions

See [Shortcuts Reference](../reference/shortcuts.md) for all available actions.

## Debugging

Use `conduit debug-keys` to see how your terminal reports key combinations.
