// Option C: Floating Panels - Novel Customizable Layout
// Draggable panels, glassmorphism, agent-specific theming

import { useState } from 'react'

// Sample data
const sampleMessages = [
  {
    role: 'user' as const,
    content: 'Can you help me add authentication to my Express app using JWT?',
  },
  {
    role: 'assistant' as const,
    content: `I'll help you add JWT authentication to your Express app. Let me first look at your current setup.`,
    tools: ['Read src/index.ts', 'Read package.json'],
  },
  {
    role: 'assistant' as const,
    content: `I can see you have a basic Express setup. I'll add the necessary authentication middleware and routes.

**What I'll do:**
1. Install required packages
2. Create auth middleware
3. Add login/register routes`,
    tools: ['Bash npm install...', 'Write auth.ts', 'Write routes.ts', 'Edit index.ts'],
  },
  {
    role: 'assistant' as const,
    content: `Done! I've added JWT authentication. Test with:

\`\`\`bash
curl -X POST http://localhost:3000/auth/register
\`\`\``,
  },
]

const sessions = [
  { id: 1, name: 'feature/auth', agent: 'Claude', isActive: true, hasActivity: false },
  { id: 2, name: 'main', agent: 'Claude', isActive: false, hasActivity: true },
  { id: 3, name: 'dashboard', agent: 'Codex', isActive: false, hasActivity: false },
]

const workspaces = [
  { name: 'express-api', branch: 'feature/auth', isActive: true },
  { name: 'express-api', branch: 'main', isActive: false },
  { name: 'react-dashboard', branch: 'main', isActive: false },
]

const toolsExecuted = [
  { name: 'Read', target: 'src/index.ts', time: '2:34:12' },
  { name: 'Read', target: 'package.json', time: '2:34:14' },
  { name: 'Bash', target: 'npm install...', time: '2:34:18' },
  { name: 'Write', target: 'src/middleware/auth.ts', time: '2:34:25' },
  { name: 'Write', target: 'src/routes/auth.ts', time: '2:34:32' },
  { name: 'Edit', target: 'src/index.ts', time: '2:34:38' },
]

// Panel wrapper with glassmorphism
function Panel({
  title,
  children,
  className = '',
  onClose,
  glow,
}: {
  title: string
  children: React.ReactNode
  className?: string
  onClose?: () => void
  glow?: string
}) {
  return (
    <div
      className={`flex flex-col rounded-xl overflow-hidden ${className}`}
      style={{
        background: 'rgba(22, 22, 30, 0.85)',
        backdropFilter: 'blur(12px)',
        border: '1px solid rgba(255, 255, 255, 0.08)',
        boxShadow: glow
          ? `0 0 40px ${glow}, 0 8px 32px rgba(0, 0, 0, 0.5)`
          : '0 8px 32px rgba(0, 0, 0, 0.5)',
      }}
    >
      {/* Panel Title Bar */}
      <div
        className="flex items-center justify-between px-4 py-2 cursor-move"
        style={{
          background: 'rgba(255, 255, 255, 0.03)',
          borderBottom: '1px solid rgba(255, 255, 255, 0.06)',
        }}
      >
        <div className="flex items-center gap-2">
          <div className="flex gap-1.5">
            <span className="w-3 h-3 rounded-full bg-[#ff5f57] cursor-pointer hover:opacity-80" onClick={onClose} />
            <span className="w-3 h-3 rounded-full bg-[#febc2e] cursor-pointer hover:opacity-80" />
            <span className="w-3 h-3 rounded-full bg-[#28c840] cursor-pointer hover:opacity-80" />
          </div>
          <span className="text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wider ml-2" style={{ fontFamily: 'Inter, sans-serif' }}>
            {title}
          </span>
        </div>
        <div className="flex items-center gap-1 text-[var(--text-muted)]">
          <span className="text-xs cursor-pointer hover:text-[var(--text-primary)]">⤢</span>
        </div>
      </div>
      {/* Panel Content */}
      <div className="flex-1 overflow-auto">{children}</div>
    </div>
  )
}

function SessionsPanel() {
  return (
    <Panel title="Sessions" className="w-64">
      <div className="p-2 space-y-1">
        {sessions.map((session) => (
          <div
            key={session.id}
            className={`flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-all ${
              session.isActive
                ? 'bg-[var(--accent-blue)]/20 border border-[var(--accent-blue)]/40'
                : 'hover:bg-white/5'
            }`}
          >
            {session.hasActivity && (
              <span className="w-2 h-2 rounded-full bg-[var(--accent-yellow)] animate-pulse" />
            )}
            <span
              className={`w-2.5 h-2.5 rounded-full ${
                session.agent === 'Claude'
                  ? 'bg-[var(--accent-blue)]'
                  : session.agent === 'Codex'
                  ? 'bg-[var(--accent-purple)]'
                  : 'bg-[var(--accent-cyan)]'
              }`}
            />
            <div className="flex-1 min-w-0">
              <div className="text-sm text-[var(--text-primary)] truncate">{session.name}</div>
              <div className="text-xs text-[var(--text-muted)]">{session.agent}</div>
            </div>
            {session.isActive && (
              <span className="text-[var(--accent-blue)] text-xs">●</span>
            )}
          </div>
        ))}
        <button className="w-full flex items-center justify-center gap-2 px-3 py-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-white/5 rounded-lg transition-colors text-sm mt-2">
          <span>+</span>
          <span>New Session</span>
        </button>
      </div>
    </Panel>
  )
}

function WorkspacesPanel() {
  return (
    <Panel title="Workspaces">
      <div className="p-2 space-y-1">
        {workspaces.map((ws, i) => (
          <div
            key={i}
            className={`flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all ${
              ws.isActive ? 'bg-[var(--accent-blue)]/20' : 'hover:bg-white/5'
            }`}
          >
            <span className="text-[var(--text-muted)] text-xs">⎇</span>
            <span className="text-sm text-[var(--text-primary)]">{ws.name}</span>
            <span className="text-xs text-[var(--text-muted)]">/{ws.branch}</span>
          </div>
        ))}
      </div>
    </Panel>
  )
}

function AgentInfoPanel() {
  return (
    <Panel title="Agent Info" glow="rgba(130, 170, 255, 0.15)">
      <div className="p-4 space-y-4">
        {/* Agent Type */}
        <div className="flex items-center gap-3">
          <div
            className="w-10 h-10 rounded-xl flex items-center justify-center text-lg"
            style={{
              background: 'linear-gradient(135deg, var(--accent-blue), var(--accent-purple))',
              boxShadow: '0 0 20px rgba(130, 170, 255, 0.3)',
            }}
          >
            C
          </div>
          <div>
            <div className="text-sm font-medium text-[var(--text-primary)]" style={{ fontFamily: 'Inter, sans-serif' }}>Claude Code</div>
            <div className="text-xs text-[var(--text-muted)]">claude-sonnet-4</div>
          </div>
        </div>

        {/* Token Usage */}
        <div>
          <div className="flex items-center justify-between text-xs text-[var(--text-muted)] mb-2" style={{ fontFamily: 'Inter, sans-serif' }}>
            <span>Context Usage</span>
            <span>45.2k / 200k</span>
          </div>
          <div className="h-2 bg-[var(--bg-base)] rounded-full overflow-hidden">
            <div
              className="h-full rounded-full"
              style={{
                width: '22.6%',
                background: 'linear-gradient(90deg, var(--accent-blue), var(--accent-cyan))',
                boxShadow: '0 0 10px var(--accent-blue)',
              }}
            />
          </div>
        </div>

        {/* Mode */}
        <div className="flex items-center gap-2">
          <span className="px-2 py-1 rounded bg-[var(--accent-green)]/20 text-[var(--accent-green)] text-xs font-medium">
            Build Mode
          </span>
          <span className="text-xs text-[var(--text-muted)]">~$0.14 spent</span>
        </div>

        {/* PR Status */}
        <div className="p-3 rounded-lg bg-white/5">
          <div className="flex items-center justify-between">
            <span className="text-xs text-[var(--text-muted)]" style={{ fontFamily: 'Inter, sans-serif' }}>PR #142</span>
            <span className="px-2 py-0.5 rounded text-[10px] bg-[var(--accent-green)]/20 text-[var(--accent-green)]">
              Ready to merge
            </span>
          </div>
          <div className="flex items-center gap-3 mt-2 text-xs">
            <span className="text-[var(--accent-green)]">+87</span>
            <span className="text-[var(--accent-red)]">-23</span>
            <span className="text-[var(--text-muted)]">4 files</span>
          </div>
        </div>
      </div>
    </Panel>
  )
}

function ToolsPanel() {
  return (
    <Panel title="Tools Executed">
      <div className="p-2">
        <div className="space-y-1">
          {toolsExecuted.map((tool, i) => (
            <div
              key={i}
              className="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-white/5 cursor-pointer text-xs"
            >
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
              <span className="font-medium text-[var(--text-secondary)]">{tool.name}</span>
              <span className="flex-1 text-[var(--text-muted)] truncate">{tool.target}</span>
              <span className="text-[var(--text-muted)]">{tool.time}</span>
            </div>
          ))}
        </div>
      </div>
    </Panel>
  )
}

function ChatMessage({ message }: { message: (typeof sampleMessages)[0] }) {
  const isUser = message.role === 'user'

  return (
    <div className={`flex gap-3 ${isUser ? 'flex-row-reverse' : ''}`}>
      <div
        className={`w-8 h-8 rounded-lg flex items-center justify-center text-sm flex-shrink-0 ${
          isUser ? 'bg-[var(--accent-blue)]/30' : 'bg-[var(--accent-purple)]/30'
        }`}
        style={{
          boxShadow: isUser
            ? '0 0 15px rgba(130, 170, 255, 0.3)'
            : '0 0 15px rgba(199, 146, 234, 0.3)',
        }}
      >
        {isUser ? 'U' : 'C'}
      </div>
      <div
        className={`flex-1 max-w-[80%] rounded-xl px-4 py-3 ${
          isUser ? 'bg-[var(--accent-blue)]/10' : 'bg-white/5'
        }`}
        style={{
          border: isUser
            ? '1px solid rgba(130, 170, 255, 0.2)'
            : '1px solid rgba(255, 255, 255, 0.05)',
        }}
      >
        <div className="text-sm text-[var(--text-primary)] whitespace-pre-wrap leading-relaxed">
          {message.content}
        </div>
        {message.tools && (
          <div className="flex flex-wrap gap-1.5 mt-3 pt-3 border-t border-white/10">
            {message.tools.map((tool, i) => (
              <span
                key={i}
                className="px-2 py-1 rounded text-[10px] bg-white/5 text-[var(--text-muted)]"
              >
                {tool}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function ChatPanel() {
  const [inputValue, setInputValue] = useState('')

  return (
    <Panel title="Chat" className="flex-1" glow="rgba(199, 146, 234, 0.1)">
      <div className="flex flex-col h-full">
        {/* Messages */}
        <div className="flex-1 overflow-auto p-4 space-y-4">
          {sampleMessages.map((msg, i) => (
            <ChatMessage key={i} message={msg} />
          ))}
        </div>

        {/* Input */}
        <div
          className="p-3"
          style={{
            background: 'rgba(255, 255, 255, 0.02)',
            borderTop: '1px solid rgba(255, 255, 255, 0.06)',
          }}
        >
          <div
            className="flex items-end gap-3 rounded-xl px-4 py-3"
            style={{
              background: 'rgba(0, 0, 0, 0.3)',
              border: '1px solid rgba(255, 255, 255, 0.08)',
            }}
          >
            <textarea
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              placeholder="Type a message..."
              className="flex-1 bg-transparent text-[var(--text-primary)] placeholder-[var(--text-muted)] resize-none outline-none text-sm"
              rows={2}
            />
            <button
              className="px-4 py-2 rounded-lg text-sm font-medium transition-all"
              style={{
                background: 'linear-gradient(135deg, var(--accent-blue), var(--accent-purple))',
                boxShadow: '0 0 20px rgba(130, 170, 255, 0.3)',
                fontFamily: 'Inter, sans-serif',
              }}
            >
              Send
            </button>
          </div>
        </div>
      </div>
    </Panel>
  )
}

export default function OptionC() {
  return (
    <div
      className="h-full p-6 overflow-auto"
      style={{
        background: `
          radial-gradient(ellipse at 20% 20%, rgba(130, 170, 255, 0.08) 0%, transparent 50%),
          radial-gradient(ellipse at 80% 80%, rgba(199, 146, 234, 0.06) 0%, transparent 50%),
          #0c0c10
        `,
      }}
    >
      {/* Conduit Logo */}
      <div className="absolute top-8 left-1/2 -translate-x-1/2 text-center">
        <span
          className="text-2xl font-bold tracking-tight"
          style={{
            fontFamily: 'Inter, sans-serif',
            background: 'linear-gradient(135deg, var(--accent-blue), var(--accent-purple))',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            textShadow: '0 0 40px rgba(130, 170, 255, 0.5)',
          }}
        >
          CONDUIT
        </span>
      </div>

      {/* Floating Panels Layout */}
      <div className="flex gap-6 h-full pt-12">
        {/* Left Column */}
        <div className="flex flex-col gap-4 w-64">
          <SessionsPanel />
          <WorkspacesPanel />
          <AgentInfoPanel />
        </div>

        {/* Center - Chat */}
        <div className="flex-1 flex flex-col min-w-0">
          <ChatPanel />
        </div>

        {/* Right Column */}
        <div className="w-72">
          <ToolsPanel />
        </div>
      </div>
    </div>
  )
}
