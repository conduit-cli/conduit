# Web UI Testing Checklist (OpenCode Integration)

This checklist documents the manual web UI test loop for OpenCode sessions. Use it to reproduce ordering, tool rendering, and model error handling issues, and to confirm fixes.

## Setup
- [ ] Start the server in tmux:
  - `tmux new -d -s codex-shell -n shell`
  - `tmux send-keys -t codex-shell:0.0 -- 'RUST_LOG=conduit=debug cargo run -- --data-dir /tmp/test-conduit-1 serve --host 0.0.0.0' Enter`
- [ ] Watch logs as needed:
  - `tmux capture-pane -p -J -t codex-shell:0.0 -S -200`
- [ ] Open the web UI with agent-browser:
  - `agent-browser open http://0.0.0.0:3000/`
  - `agent-browser snapshot -i`

## Session Creation
- [ ] Create a new workspace/session (picocode project).
- [ ] Pick model `opencode/glm-4.7`.
- [ ] Verify the model badge shows in the header and input is enabled.

## Live Streaming + Ordering
- [ ] Send prompt Q1: “Does this project have any tests?”
- [ ] Immediately send Q2: “Where are they located?” (before Q1 finishes).
- [ ] Observe live order: Q1 should precede Q2, and A1 should precede A2.
- [ ] Refresh mid‑stream (while text is still arriving).
- [ ] After reload, verify the order and contents match the live view.

## Tool Rendering Stability
- [ ] Send a tool‑triggering prompt (example):
  - “Open README and summarize the testing section.”
- [ ] Verify tool blocks render once and remain visible.
- [ ] Reload and confirm tool blocks rehydrate correctly.

## Error Handling (Model Removed)
- [ ] Switch to a removed model (example: `opencode/minimax-m2.1-free`).
- [ ] Send any prompt.
- [ ] Expect a visible error in the UI and model cleared on the session.
- [ ] Confirm cache invalidation in logs.
- [ ] Reopen model picker; ensure the removed model no longer appears.
- [ ] Select a valid model and confirm the session works again.

## SSE Reconnect / Server Exit
- [ ] Wait for OpenCode to go idle and/or exit (SIGTERM) and note log lines.
- [ ] Send a new prompt.
- [ ] Expect reconnect and successful response without a user‑visible error.

## Multi‑Session Isolation
- [ ] Open a second session in another tab.
- [ ] Send prompts in both sessions quickly.
- [ ] Confirm responses do not leak across sessions.

## Busy State / Spinner
- [ ] Trigger a prompt that fails fast (e.g., invalid model).
- [ ] Ensure the busy spinner stops and input is re‑enabled after failure.

## Expected Findings to Record
- [ ] Any ordering issues (live vs reload).
- [ ] Any tool block duplication or disappearance.
- [ ] Model picker state after errors and reloads.
- [ ] Missing or duplicated messages after reload.
- [ ] Stuck spinners or tools_in_flight not decrementing.

## Notes
- Use `agent-browser snapshot -i` after each interaction or page change.
- Prefer `agent-browser eval 'document.body.innerText'` for quick text inspection.

