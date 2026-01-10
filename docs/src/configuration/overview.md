# Configuration Overview

Conduit is configured via `~/.conduit/config.toml`.

## Config File Location

```
~/.conduit/config.toml
```

The file is created automatically on first run with default values.

## Configuration Sections

- [**Config File**](./config-file.md) — Full file reference
- [**Keybindings**](./keybindings.md) — Customize shortcuts
- [**Tool Paths**](./tool-paths.md) — Agent binary locations
- [**Themes**](./themes.md) — Visual customization

## Example Configuration

```toml
# Default agent
default_agent = "claude"

# Token display
show_token_usage = true
show_cost = true

# Theme
[theme]
name = "catppuccin-mocha"

# Custom tool paths
[tools]
claude = "/opt/homebrew/bin/claude"
```

## Reloading Config

Changes take effect on restart. Some settings (like theme) can be changed at runtime.
