# Conduit

A multi-agent Terminal User Interface (TUI) for orchestrating AI coding assistants. Run Claude Code and Codex CLI side-by-side with tab-based session management.

## Features

- **Multi-Agent Support** - Seamlessly switch between Claude Code and Codex CLI
- **Tab-Based Sessions** - Run multiple concurrent agent sessions (up to 10 tabs)
- **Real-Time Streaming** - Watch agent responses as they're generated
- **Token Usage Tracking** - Monitor input/output tokens and estimated costs
- **Session Persistence** - Resume previous sessions with their full context
- **Rich Terminal UI** - Markdown rendering, syntax highlighting, and animations

## Installation

### Prerequisites

- Rust 1.70+ (for building from source)
- At least one supported agent installed:
  - [Claude Code](https://github.com/anthropics/claude-code) (`claude` binary)
  - [Codex CLI](https://github.com/openai/codex) (`codex` binary)

### Build from Source

```bash
git clone https://github.com/fcoury/conduit.git
cd conduit
cargo build --release
```

The binary will be available at `target/release/conduit`.

## Usage

```bash
# Start the TUI
conduit

# Debug keyboard input (useful for troubleshooting keybindings)
conduit debug-keys
```

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New project (opens project picker) |
| `Alt+Shift+W` | Close current tab |
| `Tab` / `Shift+Tab` | Switch to next/previous tab |
| `Alt+1-9` | Jump to specific tab |
| `Alt+Shift+F`† | Fork current session |
| `Enter` | Submit prompt |
| `Shift+Enter` or `Alt+Enter` | Add newline in input |
| `Ctrl+C` | Interrupt agent |
| `Ctrl+Q` | Quit |
| `Ctrl+T` | Toggle sidebar |
| `Ctrl+G` | Toggle view mode (Chat/Raw Events) |
| `Ctrl+O` | Show model selector |
| `Ctrl+\` | Toggle Build/Plan mode* |
| `Alt+I` | Import session |
| `?` or `:help` | Show help |

\* **Note on `Ctrl+\`**: Terminal emulators vary in how they report this key combination. Some terminals send it as `Ctrl+4`. Use `conduit debug-keys` to verify how your terminal reports this shortcut. If it doesn't work, you can customize the keybinding in your config.
† **Note on `Alt+Shift+F`**: Some terminals don't emit distinct Alt+Shift combos. Use `conduit debug-keys` to verify how your terminal reports this shortcut and override the keybinding in your config if needed.

## Architecture

```
src/
├── main.rs              # Entry point
├── lib.rs               # Library exports
├── agent/               # Agent integration layer
│   ├── runner.rs        # AgentRunner trait
│   ├── events.rs        # Unified event types
│   ├── stream.rs        # JSONL stream parser
│   ├── claude.rs        # Claude Code implementation
│   ├── codex.rs         # Codex CLI implementation
│   ├── models.rs        # Model registry and pricing
│   ├── session.rs       # Session metadata
│   └── history.rs       # History loading utilities
├── config/              # Configuration
│   ├── settings.rs      # App settings and pricing
│   ├── keys.rs          # Keybinding types and parsing
│   └── default_keys.rs  # Default keybindings
├── data/                # Data persistence
│   ├── database.rs      # SQLite database
│   ├── repository.rs    # Data access layer
│   └── workspace.rs     # Workspace management
├── session/             # Session management
│   ├── cache.rs         # Session caching
│   └── import.rs        # Session import from agents
├── git/                 # Git integration
│   ├── pr.rs            # PR operations
│   └── worktree.rs      # Worktree utilities
├── util/                # Utilities
│   ├── paths.rs         # Path helpers
│   └── names.rs         # Name generation
└── ui/                  # Terminal UI
    ├── app.rs           # Main event loop
    ├── action.rs        # Action definitions
    ├── events.rs        # Input mode handling
    ├── tab_manager.rs   # Tab orchestration
    ├── session.rs       # Per-tab state
    └── components/      # UI components
        ├── chat_view.rs
        ├── input_box.rs
        ├── sidebar.rs
        ├── status_bar.rs
        ├── tab_bar.rs
        └── ...
```

## Supported Agents

### Claude Code

Spawns the `claude` binary in headless mode with streaming JSON output:
- Real-time event streaming
- Tool execution (Read, Edit, Write, Bash, Glob, Grep)
- Session resumption

### Codex CLI

Spawns the `codex` binary with structured JSON output:
- Full automation mode
- Session persistence
- Event-based communication

## Configuration

Default settings in `src/config/settings.rs`:

| Setting | Default |
|---------|---------|
| Default agent | Claude Code |
| Max tabs | 10 |
| Show token usage | Yes |
| Show cost | Yes |

### Pricing (Claude Sonnet)

- Input tokens: $3.00 / 1M tokens
- Output tokens: $15.00 / 1M tokens

## Website

The landing page at [getconduit.sh](https://getconduit.sh) is built with Astro.

### Development

```bash
cd website
npm install
npm run dev
```

The dev server runs at http://localhost:4321

### Build

```bash
cd website
npm run build
```

Output is in `website/dist/`.

### Deploy

The site is static and can be deployed to any hosting provider:

**GitHub Pages:**
```bash
cd website
npm run build
# Push dist/ to gh-pages branch or configure GitHub Actions
```

**Netlify/Vercel:**
- Connect repo and set build command to `cd website && npm run build`
- Set publish directory to `website/dist`

**Manual:**
```bash
cd website
npm run build
# Upload contents of dist/ to your server
```

## License

MIT
