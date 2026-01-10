# Installation

## Prerequisites

Before installing Conduit, ensure you have:

- **Rust 1.70+** — For building from source
- **Git** — Required for workspace and worktree management
- **At least one AI agent:**
  - [Claude Code](https://github.com/anthropics/claude-code) — `npm install -g @anthropic/claude-code`
  - [Codex CLI](https://github.com/openai/codex) — Follow OpenAI's installation guide

## Build from Source

Clone the repository and build with Cargo:

```bash
git clone https://github.com/conduit-cli/conduit.git
cd conduit
cargo build --release
```

The binary will be at `./target/release/conduit`.

### Add to PATH

```bash
# Copy to a directory in your PATH
cp ./target/release/conduit ~/.local/bin/

# Or create a symlink
ln -s $(pwd)/target/release/conduit ~/.local/bin/conduit
```

## Verify Installation

```bash
# Check Conduit is installed
conduit --help

# Start the TUI
conduit
```

## First Run

On first launch, Conduit will:

1. **Detect Git** — Shows an error dialog if Git is not found
2. **Detect Agents** — Searches for `claude` and `codex` binaries
3. **Create Config Directory** — Creates `~/.conduit/` for settings and data

If no agents are found, you'll be prompted to configure tool paths in the settings.

## Directory Structure

Conduit stores its data in `~/.conduit/`:

```
~/.conduit/
├── config.toml      # Configuration file
├── conduit.db       # SQLite database (sessions, workspaces)
├── logs/            # Application logs
├── workspaces/      # Workspace data
└── themes/          # Custom theme files
```

## Next Steps

- [Quick Start](./quick-start.md) — Get up and running in 5 minutes
- [Configuration](../configuration/overview.md) — Customize Conduit
