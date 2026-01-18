# Introduction

Conduit is a multi-agent terminal user interface (TUI) that lets you run AI coding assistants side-by-side. Orchestrate Claude Code and Codex CLI in a tabbed interface with full session management, git integration, and real-time token tracking.

## Key Features

- **Multi-Agent Tabs** — Run up to 10 concurrent AI sessions in tabs, switch instantly with `Alt+1-9`
- **Session Persistence** — All conversations are saved and can be resumed later
- **Git Integration** — Automatic worktree management, branch status, and PR tracking
- **Build & Plan Modes** — Toggle between full execution and read-only analysis
- **Customizable** — Configure keybindings, themes, and tool paths
- **Token Tracking** — Real-time usage and cost estimation in the status bar

## Supported Agents

| Agent | Description |
|-------|-------------|
| [Claude Code](https://github.com/anthropics/claude-code) | Anthropic's official CLI for Claude with tool execution |
| [Codex CLI](https://github.com/openai/codex) | OpenAI's command-line coding assistant |

## Quick Example

```bash
# Start Conduit
conduit

# In the TUI:
# Ctrl+N    → Open project picker
# Enter     → Select a repository
# Type      → Send prompts to the agent
# Alt+2     → Open a second tab
# Ctrl+Q    → Quit
```

## Getting Started

1. [Install Conduit](./getting-started/installation.md)
2. [Quick Start Guide](./getting-started/quick-start.md)
3. [Your First Session](./getting-started/first-session.md)

## Requirements

- **Git** — Required for workspace management
- **At least one agent** — Claude Code or Codex CLI installed and configured
- **Terminal** — A terminal emulator with good Unicode and color support

## Navigation

Use the sidebar to browse documentation by topic, or use the search (`/` or `s`) to find specific information.

---

*Conduit is currently in early access. Join our [Discord](https://discord.gg/conduit) for support and updates.*
