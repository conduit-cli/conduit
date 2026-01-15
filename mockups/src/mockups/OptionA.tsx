// Option A: Terminal Native - Developer Productivity Focus
// High density, keyboard-first, TUI-authentic design

import { useState } from 'react'

// Sample data for the mockup
const sampleMessages = [
  {
    role: 'user' as const,
    content: 'Can you help me add authentication to my Express app using JWT?',
  },
  {
    role: 'assistant' as const,
    content: `I'll help you add JWT authentication to your Express app. Let me first look at your current setup.`,
    tools: [
      { name: 'Read', target: 'src/index.ts', status: 'success' },
      { name: 'Read', target: 'package.json', status: 'success' },
    ],
  },
  {
    role: 'assistant' as const,
    content: `I can see you have a basic Express setup. I'll add the necessary authentication middleware and routes. Here's what I'll do:

1. Install required packages (\`jsonwebtoken\`, \`bcryptjs\`)
2. Create auth middleware
3. Add login/register routes

Let me implement this:`,
    tools: [
      { name: 'Bash', target: 'npm install jsonwebtoken bcryptjs', status: 'success', output: 'added 2 packages' },
      { name: 'Write', target: 'src/middleware/auth.ts', status: 'success' },
      { name: 'Write', target: 'src/routes/auth.ts', status: 'success' },
      { name: 'Edit', target: 'src/index.ts', status: 'success' },
    ],
  },
  {
    role: 'assistant' as const,
    content: `Done! I've added JWT authentication to your Express app. Here's a summary:

**Files created:**
- \`src/middleware/auth.ts\` - JWT verification middleware
- \`src/routes/auth.ts\` - Login and register endpoints

**Modified:**
- \`src/index.ts\` - Added auth routes

You can test it with:
\`\`\`bash
curl -X POST http://localhost:3000/auth/register \\
  -H "Content-Type: application/json" \\
  -d '{"email": "test@example.com", "password": "secret"}'
\`\`\``,
  },
]

const workspaces = [
  { name: 'express-api', branch: 'main', isActive: false },
  { name: 'express-api', branch: 'feature/auth', isActive: true },
  { name: 'react-dashboard', branch: 'main', isActive: false },
]

const tabs = [
  { id: 1, name: 'feature/auth', isActive: true, hasActivity: false },
  { id: 2, name: 'main', isActive: false, hasActivity: true },
  { id: 3, name: 'dashboard', isActive: false, hasActivity: false },
]

function TabBar() {
  return (
    <div className="flex items-center h-9 bg-[var(--bg-surface)] border-b border-[var(--border-subtle)]">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={`flex items-center gap-2 px-4 h-full border-r border-[var(--border-subtle)] cursor-pointer ${
            tab.isActive
              ? 'bg-[var(--bg-base)] text-[var(--text-bright)] border-b-2 border-b-[var(--accent-blue)]'
              : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-base)]/50'
          }`}
        >
          {tab.hasActivity && (
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent-yellow)] animate-pulse" />
          )}
          <span className="text-sm">{tab.name}</span>
          {tab.isActive && <span className="text-[var(--text-muted)] text-xs">*</span>}
        </div>
      ))}
      <div className="flex items-center justify-center w-9 h-full text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-base)]/50 cursor-pointer">
        +
      </div>
    </div>
  )
}

function StatusBar() {
  return (
    <div className="flex items-center justify-between h-7 px-3 bg-[var(--bg-surface)] border-b border-[var(--border-subtle)] text-xs">
      <div className="flex items-center gap-4">
        <select className="bg-transparent text-[var(--accent-blue)] border-none outline-none cursor-pointer">
          <option>Claude</option>
          <option>Codex</option>
          <option>Gemini</option>
        </select>
        <select className="bg-transparent text-[var(--text-secondary)] border-none outline-none cursor-pointer">
          <option>claude-sonnet-4-20250514</option>
          <option>claude-opus-4-20250514</option>
        </select>
        <span className="text-[var(--accent-green)]">Build</span>
      </div>
      <div className="flex items-center gap-4 text-[var(--text-muted)]">
        <span>45.2k tokens</span>
        <span className="text-[var(--text-secondary)]">~$0.14</span>
        <span className="text-[var(--accent-green)]">PR #142</span>
        <span className="text-[var(--accent-green)]">+87</span>
        <span className="text-[var(--accent-red)]">-23</span>
      </div>
    </div>
  )
}

function Sidebar() {
  const [collapsed, setCollapsed] = useState(false)

  if (collapsed) {
    return (
      <div
        className="w-8 bg-[var(--bg-surface)] border-r border-[var(--border-subtle)] cursor-pointer flex items-start justify-center pt-3"
        onClick={() => setCollapsed(false)}
      >
        <span className="text-[var(--text-muted)] text-xs">{'>'}</span>
      </div>
    )
  }

  return (
    <div className="w-60 bg-[var(--bg-surface)] border-r border-[var(--border-subtle)] flex flex-col">
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--border-subtle)]">
        <span className="text-[var(--text-muted)] text-xs uppercase tracking-wider">Workspaces</span>
        <button
          onClick={() => setCollapsed(true)}
          className="text-[var(--text-muted)] hover:text-[var(--text-primary)] text-xs"
        >
          {'<'}
        </button>
      </div>
      <div className="flex-1 overflow-auto p-2">
        {workspaces.map((ws, i) => (
          <div
            key={i}
            className={`flex items-center gap-2 px-2 py-1 cursor-pointer ${
              ws.isActive
                ? 'bg-[var(--bg-base)] text-[var(--accent-blue)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--bg-base)]/50'
            }`}
          >
            <span className="text-[var(--text-muted)]">{ws.isActive ? '>' : ' '}</span>
            <span className="truncate">{ws.name}</span>
            <span className="text-[var(--text-muted)] text-xs">/{ws.branch}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function ToolBlock({ tool }: { tool: { name: string; target: string; status: string; output?: string } }) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="my-2">
      <div
        className="flex items-center gap-2 cursor-pointer group"
        onClick={() => setExpanded(!expanded)}
      >
        <span className="text-[var(--accent-cyan)]">|</span>
        <span className="text-[var(--text-muted)]">#</span>
        <span className="text-[var(--accent-purple)]">{tool.name}</span>
        <span className="text-[var(--text-secondary)]">{tool.target}</span>
        <span
          className={`text-xs ${
            tool.status === 'success' ? 'text-[var(--accent-green)]' : 'text-[var(--accent-red)]'
          }`}
        >
          {tool.status === 'success' ? '✓' : '✗'}
        </span>
        <span className="text-[var(--text-muted)] text-xs opacity-0 group-hover:opacity-100">
          {expanded ? '▼' : '▶'}
        </span>
      </div>
      {expanded && tool.output && (
        <div className="ml-4 mt-1 pl-3 border-l border-[var(--border-subtle)] text-[var(--text-muted)] text-xs">
          {tool.output}
        </div>
      )}
    </div>
  )
}

function Message({ message }: { message: (typeof sampleMessages)[0] }) {
  return (
    <div className={`py-3 ${message.role === 'user' ? 'border-l-2 border-l-[var(--accent-blue)] pl-3' : ''}`}>
      <div className="flex items-center gap-2 mb-1">
        <span
          className={`text-xs font-medium ${
            message.role === 'user' ? 'text-[var(--accent-blue)]' : 'text-[var(--accent-purple)]'
          }`}
        >
          {message.role === 'user' ? 'You' : 'Claude'}
        </span>
      </div>
      <div className="text-[var(--text-primary)] whitespace-pre-wrap leading-relaxed">
        {message.content}
      </div>
      {message.tools && (
        <div className="mt-2 ml-2 pl-2 border-l border-[var(--bg-tool)]">
          {message.tools.map((tool, i) => (
            <ToolBlock key={i} tool={tool} />
          ))}
        </div>
      )}
    </div>
  )
}

function ChatView() {
  return (
    <div className="flex-1 overflow-auto px-4 py-2">
      <div className="max-w-3xl">
        {sampleMessages.map((msg, i) => (
          <Message key={i} message={msg} />
        ))}
      </div>
    </div>
  )
}

function InputBox() {
  const [value, setValue] = useState('')

  return (
    <div className="border-t border-[var(--border-subtle)] bg-[var(--bg-surface)]">
      <textarea
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Type a message... (Enter to submit, Shift+Enter for newline)"
        className="w-full bg-transparent px-4 py-3 text-[var(--text-primary)] placeholder-[var(--text-muted)] resize-none outline-none"
        rows={3}
      />
    </div>
  )
}

function KeyHints() {
  return (
    <div className="flex items-center gap-6 px-4 py-1 bg-[var(--bg-elevated)] border-t border-[var(--border-subtle)] text-xs text-[var(--text-muted)]">
      <span>
        <kbd className="text-[var(--text-secondary)]">Ctrl+N</kbd> New
      </span>
      <span>
        <kbd className="text-[var(--text-secondary)]">Enter</kbd> Submit
      </span>
      <span>
        <kbd className="text-[var(--text-secondary)]">Shift+Enter</kbd> Newline
      </span>
      <span>
        <kbd className="text-[var(--text-secondary)]">/</kbd> Commands
      </span>
      <span>
        <kbd className="text-[var(--text-secondary)]">?</kbd> Help
      </span>
    </div>
  )
}

export default function OptionA() {
  return (
    <div className="flex flex-col h-full font-mono">
      <TabBar />
      <StatusBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <div className="flex flex-col flex-1">
          <ChatView />
          <InputBox />
          <KeyHints />
        </div>
      </div>
    </div>
  )
}
