# First Session

A detailed walkthrough of your first Conduit session.

## Launch and Initial Setup

Start Conduit from your terminal:

```bash
conduit
```

### The Interface

You'll see:

- **Sidebar** (left) — Projects and workspaces tree
- **Tab Bar** (top) — Open sessions
- **Chat Area** (center) — Conversation with the agent
- **Input Box** (bottom) — Where you type prompts
- **Status Bar** (bottom) — Token usage, mode, branch info

### First-Time Detection

On first run, Conduit checks for:

1. **Git** — Required for all operations
2. **Agents** — Looks for `claude` and `codex` in your PATH

If an agent isn't found, you can configure its path in settings (`s` from sidebar).

## Creating Your First Project

### Open the Project Picker

Press `Ctrl+N`. The picker shows:

- Recent repositories you've worked with
- Fuzzy search as you type
- Option to add a new repository

### Select or Add a Repository

**For an existing repo:**
- Type to filter the list
- Use `Ctrl+K` / `Ctrl+J` to navigate
- Press `Enter` to select

**To add a new repo:**
- Press `Ctrl+A`
- Enter the path to your repository
- The repo is added to your projects

### Workspace Creation

When you select a project, Conduit creates a workspace tied to your current git branch. This workspace stores:

- Session history
- Branch association
- Metadata

## Interacting with the Agent

### Send Your First Prompt

Type a message and press `Enter`:

```
What files are in this project?
```

The agent will:
1. Analyze your request
2. Execute tools (file reads, searches, etc.)
3. Stream the response to you

### Understanding the Response

You'll see:
- **Tool Calls** — Commands the agent executes (file reads, writes, bash commands)
- **Thinking** — The agent's reasoning (if visible)
- **Response** — The final answer

### Follow-Up Questions

Continue the conversation naturally:

```
Can you explain how the main function works?
```

The agent maintains context from previous messages.

## Managing Your Session

### Scroll Through History

- `Page Up` / `Page Down` — Scroll the chat
- `g` / `G` — Jump to top/bottom (in scroll mode)
- `Esc` — Return to the bottom

### Interrupt the Agent

If an agent is taking too long:
- Press `Ctrl+C` once to clear the input
- Press `Ctrl+C` twice quickly to interrupt the agent

### Switch Modes (Claude Only)

Press `Tab` to toggle between:

- **Build Mode** — Full capabilities
- **Plan Mode** — Read-only analysis

## Saving and Resuming

### Automatic Saving

Sessions are automatically saved to the database. When you close Conduit, your conversation is preserved.

### Resume a Session

1. Open Conduit
2. Navigate to your project in the sidebar
3. Select the workspace — your session continues where you left off

### Import External Sessions

Press `Alt+I` to import sessions from:
- Claude Code sessions (from `~/.claude/`)
- Codex sessions

## Closing Up

### Close a Tab

Press `Alt+Shift+W` to close the current tab.

### Quit Conduit

Press `Ctrl+Q` to exit. Your session is automatically saved.

## Tips for Effective Sessions

1. **Be specific** — Clear prompts get better results
2. **Use Plan mode first** — Analyze before making changes
3. **Check the status bar** — Monitor token usage and costs
4. **Multiple tabs** — Run different tasks in parallel
5. **Use the command palette** — `Ctrl+P` for quick access to actions

## Next Steps

- [Core Concepts](../concepts/projects.md) — Understand projects, workspaces, sessions
- [Keyboard Shortcuts](../shortcuts/quick-reference.md) — Master the interface
- [Configuration](../configuration/overview.md) — Customize your experience
