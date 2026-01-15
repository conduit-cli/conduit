// Option B: Professional Studio - Clarity and Information Density
// Clean cards, accessible, responsive, light/dark themes

import { useState } from 'react'

// Sample data
const sampleMessages = [
  {
    role: 'user' as const,
    content: 'Can you help me add authentication to my Express app using JWT?',
    timestamp: '2:34 PM',
  },
  {
    role: 'assistant' as const,
    content: `I'll help you add JWT authentication to your Express app. Let me first look at your current setup.`,
    timestamp: '2:34 PM',
    tokens: 847,
    tools: [
      { name: 'Read', target: 'src/index.ts' },
      { name: 'Read', target: 'package.json' },
    ],
  },
  {
    role: 'assistant' as const,
    content: `I can see you have a basic Express setup. I'll add the necessary authentication middleware and routes.

**What I'll do:**
1. Install required packages (\`jsonwebtoken\`, \`bcryptjs\`)
2. Create auth middleware
3. Add login/register routes`,
    timestamp: '2:35 PM',
    tokens: 1243,
    tools: [
      { name: 'Bash', target: 'npm install jsonwebtoken bcryptjs' },
      { name: 'Write', target: 'src/middleware/auth.ts' },
      { name: 'Write', target: 'src/routes/auth.ts' },
      { name: 'Edit', target: 'src/index.ts' },
    ],
  },
  {
    role: 'assistant' as const,
    content: `Done! I've added JWT authentication to your Express app.

**Files created:**
- \`src/middleware/auth.ts\` - JWT verification middleware
- \`src/routes/auth.ts\` - Login and register endpoints

**Modified:**
- \`src/index.ts\` - Added auth routes

Test with:
\`\`\`bash
curl -X POST http://localhost:3000/auth/register \\
  -H "Content-Type: application/json" \\
  -d '{"email": "test@example.com", "password": "secret"}'
\`\`\``,
    timestamp: '2:36 PM',
    tokens: 2156,
  },
]

const repositories = [
  {
    name: 'express-api',
    workspaces: [
      { name: 'main', isDefault: true },
      { name: 'feature/auth', isActive: true },
    ],
  },
  {
    name: 'react-dashboard',
    workspaces: [{ name: 'main', isDefault: true }],
  },
]

const tabs = [
  { id: 1, name: 'feature/auth', agent: 'Claude', isActive: true },
  { id: 2, name: 'main', agent: 'Claude', isActive: false, hasActivity: true },
  { id: 3, name: 'dashboard', agent: 'Codex', isActive: false },
]

function Header() {
  return (
    <div className="flex items-center justify-between h-12 px-4 bg-[var(--bg-elevated)] border-b border-[var(--border-subtle)]">
      <div className="flex items-center gap-4">
        <span className="text-[var(--accent-blue)] font-semibold tracking-tight" style={{ fontFamily: 'Inter, sans-serif' }}>
          CONDUIT
        </span>
        <select className="bg-[var(--bg-surface)] text-[var(--text-primary)] border border-[var(--border-subtle)] rounded px-2 py-1 text-sm outline-none cursor-pointer" style={{ fontFamily: 'Inter, sans-serif' }}>
          <option>Claude Code</option>
          <option>Codex CLI</option>
          <option>Gemini CLI</option>
        </select>
        <div className="relative">
          <input
            type="text"
            placeholder="Search..."
            className="bg-[var(--bg-surface)] text-[var(--text-primary)] border border-[var(--border-subtle)] rounded px-3 py-1 pl-8 text-sm w-48 outline-none focus:border-[var(--accent-blue)]"
            style={{ fontFamily: 'Inter, sans-serif' }}
          />
          <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--text-muted)] text-xs">⌘K</span>
        </div>
      </div>
      <div className="flex items-center gap-3">
        <button className="w-8 h-8 rounded-full bg-[var(--bg-surface)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] flex items-center justify-center">
          @
        </button>
        <button className="w-8 h-8 rounded-full bg-[var(--bg-surface)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] flex items-center justify-center">
          ?
        </button>
        <button className="w-8 h-8 rounded-full bg-[var(--bg-surface)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] flex items-center justify-center">
          ≡
        </button>
      </div>
    </div>
  )
}

function TabBar() {
  return (
    <div className="flex items-center h-10 px-2 bg-[var(--bg-surface)] border-b border-[var(--border-subtle)] gap-1">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={`flex items-center gap-2 px-3 py-1.5 rounded text-sm cursor-pointer transition-colors ${
            tab.isActive
              ? 'bg-[var(--bg-base)] text-[var(--text-bright)] shadow-sm'
              : 'text-[var(--text-secondary)] hover:bg-[var(--bg-base)]/50'
          }`}
          style={{ fontFamily: 'Inter, sans-serif' }}
        >
          {tab.hasActivity && (
            <span className="w-2 h-2 rounded-full bg-[var(--accent-yellow)]" />
          )}
          <span
            className={`w-2 h-2 rounded-full ${
              tab.agent === 'Claude'
                ? 'bg-[var(--accent-blue)]'
                : tab.agent === 'Codex'
                ? 'bg-[var(--accent-purple)]'
                : 'bg-[var(--accent-cyan)]'
            }`}
          />
          <span>{tab.name}</span>
          {tab.isActive && (
            <span className="text-[var(--text-muted)] hover:text-[var(--text-primary)]">×</span>
          )}
        </div>
      ))}
      <button className="w-8 h-8 rounded text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-base)]/50 flex items-center justify-center text-lg">
        +
      </button>
    </div>
  )
}

function Sidebar() {
  const [collapsed, setCollapsed] = useState(false)

  if (collapsed) {
    return (
      <div
        className="w-12 bg-[var(--bg-surface)] border-r border-[var(--border-subtle)] cursor-pointer flex flex-col items-center pt-4 gap-4"
        onClick={() => setCollapsed(false)}
      >
        <span className="text-[var(--text-muted)]">📁</span>
        <span className="text-[var(--text-muted)]">⚙️</span>
      </div>
    )
  }

  return (
    <div className="w-64 bg-[var(--bg-surface)] border-r border-[var(--border-subtle)] flex flex-col" style={{ fontFamily: 'Inter, sans-serif' }}>
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <span className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">Workspaces</span>
        <button
          onClick={() => setCollapsed(true)}
          className="text-[var(--text-muted)] hover:text-[var(--text-primary)] text-xs"
        >
          ◀
        </button>
      </div>
      <div className="flex-1 overflow-auto p-3">
        {repositories.map((repo) => (
          <div key={repo.name} className="mb-4">
            <div className="flex items-center gap-2 text-[var(--text-secondary)] text-sm font-medium mb-2">
              <span>▼</span>
              <span>{repo.name}</span>
            </div>
            <div className="ml-4 space-y-1">
              {repo.workspaces.map((ws) => (
                <div
                  key={ws.name}
                  className={`flex items-center gap-2 px-2 py-1.5 rounded text-sm cursor-pointer ${
                    ws.isActive
                      ? 'bg-[var(--accent-blue)]/10 text-[var(--accent-blue)] border border-[var(--accent-blue)]/30'
                      : 'text-[var(--text-secondary)] hover:bg-[var(--bg-base)]'
                  }`}
                >
                  <span className="text-xs">⎇</span>
                  <span>{ws.name}</span>
                  {ws.isDefault && (
                    <span className="text-[10px] bg-[var(--bg-elevated)] text-[var(--text-muted)] px-1.5 py-0.5 rounded">
                      default
                    </span>
                  )}
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
      <div className="p-3 border-t border-[var(--border-subtle)]">
        <button className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-[var(--bg-base)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] rounded text-sm transition-colors">
          <span>+</span>
          <span>Add repository</span>
        </button>
      </div>
    </div>
  )
}

function SessionHeader() {
  return (
    <div className="flex items-center justify-between px-4 py-2 bg-[var(--bg-surface)] border-b border-[var(--border-subtle)]" style={{ fontFamily: 'Inter, sans-serif' }}>
      <div className="flex items-center gap-3">
        <span className="text-[var(--text-primary)] font-medium">Session: feature/auth</span>
        <span className="text-[var(--text-muted)] text-sm">branch: feature/auth</span>
        <span className="text-[var(--accent-green)] text-sm">+87</span>
        <span className="text-[var(--accent-red)] text-sm">-23</span>
      </div>
      <div className="flex items-center gap-2">
        <span className="px-2 py-1 bg-[var(--accent-green)]/10 text-[var(--accent-green)] text-xs rounded font-medium">
          PR #142 Ready
        </span>
      </div>
    </div>
  )
}

function ToolPill({ tool }: { tool: { name: string; target: string } }) {
  return (
    <span className="inline-flex items-center gap-1.5 px-2 py-1 bg-[var(--bg-elevated)] rounded text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-base)] cursor-pointer transition-colors">
      <span
        className={`w-1.5 h-1.5 rounded-full ${
          tool.name === 'Read'
            ? 'bg-[var(--accent-cyan)]'
            : tool.name === 'Write'
            ? 'bg-[var(--accent-green)]'
            : tool.name === 'Edit'
            ? 'bg-[var(--accent-yellow)]'
            : 'bg-[var(--accent-purple)]'
        }`}
      />
      <span className="font-medium">{tool.name}</span>
      <span className="text-[var(--text-muted)] truncate max-w-[150px]">{tool.target}</span>
    </span>
  )
}

function MessageCard({ message }: { message: (typeof sampleMessages)[0] }) {
  const isUser = message.role === 'user'

  return (
    <div
      className={`rounded-lg border ${
        isUser
          ? 'bg-[var(--bg-elevated)] border-[var(--border-default)]'
          : 'bg-[var(--bg-surface)] border-[var(--border-subtle)]'
      } mb-4`}
    >
      {/* Card Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2" style={{ fontFamily: 'Inter, sans-serif' }}>
          <span
            className={`w-2 h-2 rounded-full ${
              isUser ? 'bg-[var(--accent-blue)]' : 'bg-[var(--accent-purple)]'
            }`}
          />
          <span className="font-medium text-sm text-[var(--text-primary)]">
            {isUser ? 'You' : 'Claude'}
          </span>
          <span className="text-[var(--text-muted)] text-xs">{message.timestamp}</span>
        </div>
        {message.tokens && (
          <span className="text-[var(--text-muted)] text-xs" style={{ fontFamily: 'Inter, sans-serif' }}>
            {message.tokens.toLocaleString()} tokens
          </span>
        )}
      </div>

      {/* Card Content */}
      <div className="px-4 py-3">
        <div className="text-[var(--text-primary)] whitespace-pre-wrap leading-relaxed text-sm">
          {message.content}
        </div>

        {/* Tools */}
        {message.tools && (
          <div className="flex flex-wrap gap-2 mt-3 pt-3 border-t border-[var(--border-subtle)]">
            {message.tools.map((tool, i) => (
              <ToolPill key={i} tool={tool} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function ChatView() {
  return (
    <div className="flex-1 overflow-auto px-6 py-4">
      <div className="max-w-3xl mx-auto">
        {sampleMessages.map((msg, i) => (
          <MessageCard key={i} message={msg} />
        ))}
      </div>
    </div>
  )
}

function InputBox() {
  const [value, setValue] = useState('')

  return (
    <div className="border-t border-[var(--border-subtle)] bg-[var(--bg-surface)] px-6 py-4">
      <div className="max-w-3xl mx-auto">
        <div className="flex items-end gap-3 bg-[var(--bg-base)] rounded-lg border border-[var(--border-default)] p-3">
          <textarea
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder="Type a message..."
            className="flex-1 bg-transparent text-[var(--text-primary)] placeholder-[var(--text-muted)] resize-none outline-none text-sm"
            style={{ fontFamily: 'IBM Plex Mono, monospace' }}
            rows={2}
          />
          <div className="flex items-center gap-2">
            <button className="w-8 h-8 rounded bg-[var(--bg-surface)] text-[var(--text-muted)] hover:text-[var(--text-primary)] flex items-center justify-center text-sm">
              📎
            </button>
            <button className="px-4 py-2 rounded bg-[var(--accent-blue)] text-[var(--bg-base)] font-medium text-sm hover:opacity-90 transition-opacity" style={{ fontFamily: 'Inter, sans-serif' }}>
              Send
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

function Footer() {
  return (
    <div className="flex items-center justify-between px-6 py-2 bg-[var(--bg-elevated)] border-t border-[var(--border-subtle)] text-xs" style={{ fontFamily: 'Inter, sans-serif' }}>
      <div className="flex items-center gap-4 text-[var(--text-muted)]">
        <span className="text-[var(--accent-green)]">Build Mode</span>
        <span>claude-sonnet-4</span>
        <span>~$0.14 this session</span>
      </div>
      <div className="text-[var(--text-muted)]">
        <kbd className="px-1.5 py-0.5 bg-[var(--bg-surface)] rounded text-[10px]">Ctrl</kbd>
        <span className="mx-1">+</span>
        <kbd className="px-1.5 py-0.5 bg-[var(--bg-surface)] rounded text-[10px]">?</kbd>
        <span className="ml-2">for help</span>
      </div>
    </div>
  )
}

export default function OptionB() {
  return (
    <div className="flex flex-col h-full">
      <Header />
      <TabBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <div className="flex flex-col flex-1">
          <SessionHeader />
          <ChatView />
          <InputBox />
          <Footer />
        </div>
      </div>
    </div>
  )
}
