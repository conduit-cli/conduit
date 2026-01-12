# Config File Reference

Complete reference for `~/.conduit/config.toml`.

## Application Settings

```toml
# Default agent: "claude" or "codex"
default_agent = "claude"

# Working directory for agents (defaults to current directory)
# working_dir = "/path/to/default"

# Maximum concurrent tabs
max_tabs = 10

# Token usage display
show_token_usage = true
show_cost = true

# Cost calculation (per million tokens)
claude_input_cost_per_million = 3.0
claude_output_cost_per_million = 15.0
```

## Theme Configuration

```toml
[theme]
# Built-in theme name
name = "default-dark"

# Or custom theme path
# path = "~/.conduit/themes/my-theme.toml"
```

Built-in themes: `default-dark`, `default-light`, `catppuccin-mocha`, `catppuccin-latte`, `tokyo-night`, `dracula`

## Tool Paths

```toml
[tools]
git = "/usr/bin/git"
gh = "/usr/local/bin/gh"
claude = "/opt/homebrew/bin/claude"
codex = "/usr/local/bin/codex"
```

## Selection & Clipboard

```toml
[selection]
# Copy immediately when mouse selection ends
auto_copy_selection = true
# Clear the selection after copying
clear_selection_after_copy = true
```

## Keybindings

See [Keybindings](./keybindings.md) for customization.
