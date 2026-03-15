# Codex CLI

[Codex CLI](https://github.com/openai/codex) is OpenAI's command-line coding assistant.

## Features

- **Full Automation** — Executes code autonomously
- **272K Context** — Extended context window
- **GPT-5.4** — Latest frontier agentic coding model

## Models

| Model | Best For |
|-------|----------|
| GPT-5.4 | Latest frontier agentic coding model (default) |
| GPT-5.3-Codex | Frontier Codex-optimized agentic coding model |
| GPT-5.3-Codex-Spark | Ultra-fast coding model |
| GPT-5.2-Codex | Frontier agentic coding model |
| GPT-5.1-Codex-Max | Deep and fast reasoning |
| GPT-5.2 | Frontier general model for knowledge, reasoning, and coding |
| GPT-5.1-Codex-Mini | Cheaper/faster Codex option |

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
