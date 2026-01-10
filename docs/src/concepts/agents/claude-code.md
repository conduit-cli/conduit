# Claude Code

[Claude Code](https://github.com/anthropics/claude-code) is Anthropic's official CLI for Claude.

## Features

- **Tool Execution** — Read, write, and execute commands
- **Build/Plan Modes** — Toggle between full execution and read-only
- **Multiple Models** — Opus, Sonnet, Haiku
- **200K Context** — Large context window

## Models

| Model | Best For |
|-------|----------|
| Opus | Complex reasoning, architecture |
| Sonnet | Balanced performance (default) |
| Haiku | Quick tasks, low latency |

## Build vs Plan Mode

- **Build Mode** (default) — Full capabilities
- **Plan Mode** — Read-only analysis, no file modifications

Toggle with `Tab` or `Ctrl+4`.

## Tools Available

- `Read` — Read file contents
- `Write` — Create or overwrite files
- `Edit` — Modify existing files
- `Bash` — Execute shell commands
- `Glob` — Find files by pattern
- `Grep` — Search file contents

## Installation

```bash
npm install -g @anthropic/claude-code
```
