# Build vs Plan Mode

Claude Code supports two operational modes.

## Build Mode (Default)

In Build mode, the agent has full capabilities:
- Read files
- Write and edit files
- Execute shell commands
- Make changes to your codebase

## Plan Mode

In Plan mode, the agent is read-only:
- Can read files
- Can search the codebase
- Can analyze code
- **Cannot** modify files
- **Cannot** execute commands

## When to Use Each

**Use Build Mode when:**
- Implementing features
- Fixing bugs
- Refactoring code
- Running tests

**Use Plan Mode when:**
- Exploring unfamiliar code
- Getting explanations
- Planning before implementation
- Reviewing architecture

## Toggling Modes

| Shortcut | Context |
|----------|---------|
| `Tab` | Chat or Sidebar mode |
| `Ctrl+4` | Global (Ctrl+\) |

## Visual Indicator

The status bar shows the current mode:
- **Build** — Full execution enabled
- **Plan** — Read-only mode

## Note

Plan mode only affects Claude Code. Codex CLI runs in full automation mode.
