# User Interface Overview

Conduit's TUI is designed for efficient keyboard-driven workflows.

## Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Tab 1  │  Tab 2  │  Tab 3                                  │ Tab Bar
├─────────┬───────────────────────────────────────────────────┤
│         │                                                   │
│ Projects│               Chat Area                           │
│         │                                                   │
│ > repo1 │  Messages, tool outputs, and responses            │
│   main  │                                                   │
│   feat  │                                                   │
│         │                                                   │
│ > repo2 │                                                   │
│   main  │                                                   │
│         │                                                   │
├─────────┴───────────────────────────────────────────────────┤
│                     Input Box                               │
├─────────────────────────────────────────────────────────────┤
│ Tokens: 1.2K/500  │  $0.02  │  main  │  Build  │  Claude   │ Status
└─────────────────────────────────────────────────────────────┘
```

## Components

- [**Tab Bar**](./tab-bar.md) — Open sessions
- [**Sidebar**](./sidebar.md) — Project tree
- [**Chat View**](./chat-view.md) — Conversation
- [**Status Bar**](./status-bar.md) — Info display
- [**Command Palette**](./command-palette.md) — Quick actions

## Keyboard-First Design

All interactions are keyboard-driven. Use `?` or `:help` to see available shortcuts.
