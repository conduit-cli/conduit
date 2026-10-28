# Codex CLI

[Codex CLI](https://github.com/openai/codex) is OpenAI's command-line coding assistant.

## Features

- **Full Automation** — Executes code autonomously
- **Dynamic models** — Discovers the catalog from your installed Codex CLI
- **Shared model selection** — Uses the discovered models in the terminal and web interfaces

## Models

Conduit queries Codex's `app-server` using `model/list` and uses the returned
model names, descriptions, and default selection. An explicitly configured
Conduit model takes precedence over the catalog default.

Discovery runs in the background at startup and refreshes on catalog access
after an hour. The last successful catalog is saved as `codex_models.json` in
Conduit's data directory and remains available if discovery fails. Failed
refreshes are retried on catalog access after a minute. Until the first successful
discovery, Conduit uses its bundled fallback list.

The catalog does not guarantee access to every listed model. Context limits use
runtime observations when available, then known model limits or a fallback
estimate, because `model/list` does not supply context-window sizes.

## Automation Mode

Codex runs in full automation mode, executing actions without confirmation prompts.

## Tools Available

Similar capabilities to Claude Code:
- File reading and writing
- Shell command execution
- Code search and analysis

## Installation

Follow the [official installation guide](https://github.com/openai/codex).

## Configuration

Ensure Codex is in your PATH, or configure the path:

```toml
[tools]
codex = "/path/to/codex"
```
