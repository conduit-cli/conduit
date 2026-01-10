# Tool Paths

Configure paths to external tools.

## Default Detection

Conduit searches your `PATH` for:
- `git` — Required
- `gh` — GitHub CLI (optional)
- `claude` — Claude Code agent
- `codex` — Codex CLI agent

## Custom Paths

If tools aren't in your PATH, specify them:

```toml
[tools]
git = "/usr/bin/git"
gh = "/usr/local/bin/gh"
claude = "/opt/homebrew/bin/claude"
codex = "/home/user/.local/bin/codex"
```

## Verifying Paths

Check tool detection:

```bash
# Should show tool locations
which claude codex git gh
```

## Missing Tools

If a required tool is missing, Conduit shows a dialog on startup with options to:
- Configure the path manually
- Skip (if optional)
