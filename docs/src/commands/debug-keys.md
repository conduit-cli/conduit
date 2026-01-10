# conduit debug-keys

Debug keyboard input to troubleshoot keybinding issues.

## Usage

```bash
conduit debug-keys
```

## Purpose

Different terminals report key combinations differently. Use this command to:
- See exactly what key events your terminal sends
- Troubleshoot keybindings that don't work
- Find the correct notation for custom keybindings

## Output

The command shows:
- Key code
- Modifier keys pressed
- The Conduit key notation

## Example

```
Press keys to see their codes. Press Ctrl+C to exit.

Key: Char('4'), Modifiers: CONTROL
  → Notation: C-4

Key: Tab, Modifiers: ALT
  → Notation: M-Tab
```

## Common Issues

| Expected | May Report As |
|----------|---------------|
| `Ctrl+\` | `Ctrl+4` |
| `Ctrl+Backspace` | `Ctrl+H` |
| `Alt+Shift+Tab` | `BackTab` with ALT |

Use the reported notation in your `config.toml` keybindings.
