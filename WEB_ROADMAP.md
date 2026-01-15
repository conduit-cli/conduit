# Web Parity Roadmap (Sequential, Testable)

## Global Defaults
- **Persistence:** server-side, shared profile (`default`) for all clients
- **Auth:** none for now; keep a clean seam for future user IDs
- **Keyboard goal:** every milestone ships with a concrete keyboard checklist
- **Icon refresh:** update chat stream icons to semantic Lucide set with status coloring and subtle in-progress animation

## Milestone A — Shell & State Parity
**Scope (UI)**
- Session tabs UI: reorder, switch, active state
- Status strip in header: agent, model, token usage, git/PR, latency
- Sidebar toggle and focused navigation

**Server Persistence**
- Persist UI state: active session, tab order, sidebar visibility, last workspace

**API Contracts**
- `GET /ui/state` → `{ active_session_id, tab_order, sidebar_open, last_workspace_id }`
- `POST /ui/state` → update fields (partial updates)
- Extend `GET /sessions` to include tab metadata for ordering

**Keyboard Checklist**
- Open/close sidebar
- Switch tab forward/back
- Reorder tabs
- Jump to workspace list from header

## Milestone B — Chat Parity
**Scope (UI)**
- Tool blocks with collapsible output and exit status
- Streaming assistant message support
- Turn summaries and token usage line
- Inline prompt handling (AskUserQuestion + ExitPlanMode)
- Raw event viewer drawer
- Icon refresh for chat stream

**WS / API Contracts**
- Implement WS `ControlRequest` handling for `respond_to_control`
- Ensure `TurnCompleted` includes token usage info
- `GET /sessions/{id}/events` includes summaries and tool events (history parity)

**Keyboard Checklist**
- Focus input, send, cancel in-flight
- Navigate tool blocks, expand/collapse output
- Answer inline prompt via keyboard only
- Open/close raw events drawer

## Milestone C — Repo/Workspace Parity
**Scope (UI)**
- Add repo dialog with path validation
- Project picker (scan base path for git repos)
- Archive/remove/fork workspace flows
- Confirmation dialogs for PR/dangerous actions

**API Contracts**
- `GET /repositories/scan?basePath=...` → list git repos
- `POST /workspaces/{id}/archive` already exists
- `DELETE /workspaces/{id}` already exists
- Add PR preflight endpoint for confirmations

**Keyboard Checklist**
- Open add-repo dialog, complete via keyboard
- Select repo from picker, create workspace
- Trigger archive/remove, confirm/cancel

## Milestone D — Power Tools
**Scope (UI)**
- Command palette with fuzzy search
- Help panel with keybinding list and context hints

**API Contracts**
- `GET /commands` → available actions + shortcuts + contexts

**Keyboard Checklist**
- Open palette, run command, close
- Open help, navigate sections, close

## Milestone E — Migration & Configuration
**Scope (UI)**
- Session import picker (CLI artifacts)
- Model selector with defaults per agent
- Theme preview/restore parity

**API Contracts**
- `GET /sessions/import/scan` → discoverable sessions
- `POST /sessions/import` → import to workspace
- `GET /models` → per-agent available + defaults

**Keyboard Checklist**
- Select and import a session via keyboard
- Open model selector, switch model
- Preview theme, revert

## Testing Cadence
- Pause at the end of each milestone for manual testing before proceeding.
