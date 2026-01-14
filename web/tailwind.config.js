/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        // Professional Studio palette from mockups
        background: '#0a0a0f',
        surface: '#12121a',
        'surface-elevated': '#1a1a24',
        border: '#2a2a3a',
        'border-subtle': '#1f1f2e',
        text: '#e4e4eb',
        'text-muted': '#8b8b9e',
        accent: '#6366f1',
        'accent-hover': '#818cf8',
        success: '#22c55e',
        warning: '#f59e0b',
        error: '#ef4444',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'Menlo', 'monospace'],
      },
    },
  },
  plugins: [],
}
