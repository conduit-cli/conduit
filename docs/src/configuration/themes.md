# Themes

Customize Conduit's appearance with themes.

## Built-in Themes

| Theme | Description |
|-------|-------------|
| `default-dark` | Dark theme (default) |
| `default-light` | Light theme |
| `catppuccin-mocha` | Catppuccin dark variant |
| `catppuccin-latte` | Catppuccin light variant |
| `tokyo-night` | Tokyo Night theme |
| `dracula` | Dracula theme |

## Selecting a Theme

### Via Config File

```toml
[theme]
name = "catppuccin-mocha"
```

### At Runtime

Press `Alt+T` to open the theme picker.

## Theme Discovery

Conduit auto-discovers:
- Built-in themes
- VS Code themes from `~/.vscode/extensions/`
- Custom themes in `~/.conduit/themes/`

## More Information

- [Built-in Themes](./themes/builtin.md) — Screenshots and details
- [Custom Themes](./themes/custom.md) — Create your own
- [VS Code Migration](./themes/vscode-migration.md) — Convert VS Code themes
