# Sessions

A **session** is a conversation thread with an AI agent.

## What is a Session?

- Sessions contain the full conversation history
- Each tab has one active session
- Sessions are persisted to the database

## Session Persistence

All sessions are automatically saved to `~/.conduit/conduit.db`. You can:

- Close Conduit and resume later
- Switch between workspaces
- Import external sessions

## Session History

The session maintains:
- All messages (prompts and responses)
- Tool executions and outputs
- Token usage statistics
- Timestamps

## Resuming Sessions

When you open a workspace, the previous session continues automatically.

## Importing Sessions

Press `Alt+I` to import sessions from:
- Claude Code (`~/.claude/`)
- Codex CLI

Use `Tab` to filter by agent type.
