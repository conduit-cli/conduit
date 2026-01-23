# AGENTS.md

Project-specific instructions for agents working in this repo.

## Commit Message Guidelines

- Use Angular-style commit messages (e.g., `feat: ...`, `chore: ...`) and include a short detail summary in the body.

## Changes routine

- Based on complexity and affected surface, decide whether to follow the TDD red-green approach or not.
- Always run `cargo check --all` after making changes and before the last test cycle.
- Always run all tests after finishing the changes with `cargo nextest run --all`. If execution fails, also try `cargo test --all` before reporting the test failure.
- Always run the Code Hygiene routine after finishing the changes.

## Code hygiene

- After each feature implementation, run `cargo fmt --all` and `cargo clippy -- -D warnings`.
  - Fix all the reported issues.

## Web interface testing (agent-browser)

- Start the server in tmux (background) and keep logs visible:
  - `tmux new -d -s codex-shell -n shell`
  - `tmux send-keys -t codex-shell:0.0 -- 'RUST_LOG=conduit=debug cargo run -- --data-dir /tmp/test-conduit-1 serve --host 0.0.0.0' Enter`
  - Logs: `tmux capture-pane -p -J -t codex-shell:0.0 -S -200`
- Use `agent-browser` (not Playwright directly) for UI automation:
  - `agent-browser open http://0.0.0.0:3000/`
  - `agent-browser snapshot -i` to get refs, then `agent-browser click @eX`, `fill`, `press Enter`.
- Suggested flow for OpenCode sessions:
  - Create a new workspace, open model picker, choose `opencode/glm-4.7`, send prompts.
  - Check message ordering and tool rendering live.
  - Reload (`agent-browser reload`) and compare ordering/history to the live view.
  - If ordering or duplication issues appear live but not after reload, inspect SSE handling in `src/agent/opencode.rs`.

## Error handling

- Do not swallow errors. Avoid `let _ =` or `_ =` to discard `Result` values; handle them explicitly or log the failure with context.
