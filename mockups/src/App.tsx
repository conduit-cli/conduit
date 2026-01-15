import { useState } from 'react'
import OptionA from './mockups/OptionA'
import OptionB from './mockups/OptionB'
import OptionC from './mockups/OptionC'

type MockupOption = 'A' | 'B' | 'C'

function App() {
  const [activeOption, setActiveOption] = useState<MockupOption>('A')

  return (
    <div className="flex flex-col h-screen">
      {/* Mockup Switcher */}
      <div className="flex items-center gap-4 p-3 bg-[var(--bg-elevated)] border-b border-[var(--border-subtle)]">
        <span className="text-[var(--text-muted)] text-xs uppercase tracking-wider">Mockup:</span>
        <div className="flex gap-2">
          {(['A', 'B', 'C'] as const).map((option) => (
            <button
              key={option}
              onClick={() => setActiveOption(option)}
              className={`px-3 py-1 text-sm transition-colors ${
                activeOption === option
                  ? 'bg-[var(--accent-blue)] text-[var(--bg-base)]'
                  : 'bg-[var(--bg-surface)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
              }`}
            >
              Option {option}
            </button>
          ))}
        </div>
        <span className="text-[var(--text-muted)] text-xs ml-4">
          {activeOption === 'A' && 'Terminal Native - Developer Productivity'}
          {activeOption === 'B' && 'Professional Studio - Clarity & Density'}
          {activeOption === 'C' && 'Floating Panels - Customizable Layout'}
        </span>
      </div>

      {/* Mockup Content */}
      <div className="flex-1 overflow-hidden">
        {activeOption === 'A' && <OptionA />}
        {activeOption === 'B' && <OptionB />}
        {activeOption === 'C' && <OptionC />}
      </div>
    </div>
  )
}

export default App
