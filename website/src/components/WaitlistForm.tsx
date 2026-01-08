/**
 * WaitlistForm - Modal-based waitlist signup with X integration
 */

import { useState, useEffect, useRef } from 'react'

type FormState = 'idle' | 'submitting' | 'success' | 'error' | 'duplicate'

interface XUser {
  id: string
  handle: string
}

interface WaitlistFormProps {
  className?: string
}

export default function WaitlistForm({ className = '' }: WaitlistFormProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [email, setEmail] = useState('')
  const [formState, setFormState] = useState<FormState>('idle')
  const [errorMessage, setErrorMessage] = useState('')
  const [position, setPosition] = useState<number | null>(null)
  const [xUser, setXUser] = useState<XUser | null>(null)
  const emailInputRef = useRef<HTMLInputElement>(null)

  // Check for X user cookie and URL params on mount
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)

    // Check for X connection success with user data in URL
    if (urlParams.get('x_connected') === 'true') {
      const xUserParam = urlParams.get('x_user')
      if (xUserParam) {
        try {
          const userData = JSON.parse(decodeURIComponent(xUserParam))
          setXUser(userData)
          // Store in cookie for persistence
          document.cookie = `x_user=${encodeURIComponent(JSON.stringify(userData))}; path=/; max-age=3600; SameSite=Lax; Secure`
        } catch (e) {
          console.error('Failed to parse X user from URL:', e)
        }
      }
      // Clean URL
      window.history.replaceState({}, '', window.location.pathname)
      setIsOpen(true)
      // Focus email input after modal opens
      setTimeout(() => emailInputRef.current?.focus(), 100)
    }

    // Check for existing X user cookie
    const xUserCookie = document.cookie
      .split('; ')
      .find(row => row.startsWith('x_user='))

    if (xUserCookie && !urlParams.get('x_user')) {
      try {
        const userData = JSON.parse(decodeURIComponent(xUserCookie.split('=')[1]))
        setXUser(userData)
      } catch (e) {
        console.error('Failed to parse X user cookie:', e)
      }
    }

    // Check for errors
    const error = urlParams.get('error')
    if (error) {
      setErrorMessage('Failed to connect with X. Please try again.')
      setIsOpen(true)
      window.history.replaceState({}, '', window.location.pathname)
    }
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (!email.trim()) return

    setFormState('submitting')
    setErrorMessage('')

    try {
      const response = await fetch('/api/waitlist', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email: email.trim().toLowerCase(),
          twitter_handle: xUser?.handle || null,
          twitter_id: xUser?.id || null,
          twitter_verified: !!xUser
        })
      })

      const data = await response.json()

      if (response.status === 409) {
        // Already registered - show position anyway
        setFormState('duplicate')
        setPosition(data.position)
      } else if (!response.ok) {
        setFormState('error')
        setErrorMessage(data.error || 'Something went wrong')
      } else {
        setFormState('success')
        setPosition(data.position)
        setEmail('')
        // Clear X user cookie after successful signup
        document.cookie = 'x_user=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
      }
    } catch {
      setFormState('error')
      setErrorMessage('Network error. Please try again.')
    }
  }

  const handleXConnect = () => {
    // Store current email in session storage to restore after OAuth
    if (email.trim()) {
      sessionStorage.setItem('waitlist_email', email.trim())
    }
    // Redirect to X OAuth
    window.location.href = '/api/auth/x'
  }

  const disconnectX = () => {
    setXUser(null)
    document.cookie = 'x_user=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
  }

  // Restore email from session storage after OAuth
  useEffect(() => {
    const savedEmail = sessionStorage.getItem('waitlist_email')
    if (savedEmail) {
      setEmail(savedEmail)
      sessionStorage.removeItem('waitlist_email')
    }
  }, [])

  // Success state
  if (formState === 'success' || formState === 'duplicate') {
    return (
      <div className={className}>
        {/* Trigger button shows success */}
        <button
          onClick={() => setIsOpen(true)}
          className="btn-primary"
        >
          {formState === 'success' ? "You're on the list!" : 'View your position'}
        </button>

        {/* Modal */}
        {isOpen && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
            {/* Backdrop */}
            <div
              className="absolute inset-0 bg-black/70 backdrop-blur-sm"
              onClick={() => setIsOpen(false)}
            />

            {/* Modal content */}
            <div className="relative bg-[var(--color-bg-hero)] border border-[var(--color-border-default)] rounded-lg p-8 max-w-md w-full shadow-2xl">
              <button
                onClick={() => setIsOpen(false)}
                className="absolute top-4 right-4 text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)] text-xl"
              >
                ×
              </button>

              <div className="text-center">
                <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-[var(--color-accent-success)]/10 flex items-center justify-center">
                  <svg className="w-8 h-8 text-[var(--color-accent-success)]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                  </svg>
                </div>

                <h3 className="text-2xl font-display text-[var(--color-text-bright)] mb-2">
                  {formState === 'success' ? "You're in!" : "You're already in!"}
                </h3>

                {position && (
                  <div className="my-6">
                    <p className="text-[var(--color-text-muted)] text-sm mb-1">Your position</p>
                    <p className="text-5xl font-display text-[var(--color-accent-primary)]">
                      #{position}
                    </p>
                  </div>
                )}

                <p className="text-[var(--color-text-secondary)] text-sm mb-4">
                  We'll notify you when Conduit is ready.
                </p>

                {/* Skip the line CTA */}
                <div className="pt-4 border-t border-[var(--color-border-dim)]">
                  <p className="text-[var(--color-text-muted)] text-xs mb-3">
                    Want to skip the line?
                  </p>
                  <a
                    href="https://x.com/fcoury"
                    target="_blank"
                    rel="noopener"
                    className="inline-flex items-center gap-2 px-4 py-2 rounded-lg border border-[var(--color-border-default)] hover:border-[var(--color-accent-primary)] transition-colors text-sm text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
                  >
                    <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
                    </svg>
                    Follow @fcoury on X
                  </a>
                </div>

                {xUser && (
                  <p className="text-[var(--color-text-muted)] text-xs mt-4">
                    Connected as @{xUser.handle}
                  </p>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className={className}>
      {/* Trigger button */}
      <button
        onClick={() => setIsOpen(true)}
        className="btn-primary"
      >
        Join Waitlist
      </button>

      {/* Modal */}
      {isOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          {/* Backdrop */}
          <div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm"
            onClick={() => setIsOpen(false)}
          />

          {/* Modal content */}
          <div className="relative bg-[var(--color-bg-hero)] border border-[var(--color-border-default)] rounded-lg p-8 max-w-md w-full shadow-2xl">
            <button
              onClick={() => setIsOpen(false)}
              className="absolute top-4 right-4 text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)] text-xl"
            >
              ×
            </button>

            <h2 className="text-2xl font-display text-[var(--color-text-bright)] mb-6 text-center">
              Join the Waitlist
            </h2>

            <form onSubmit={handleSubmit} className="space-y-4">
              {/* Email field */}
              <div>
                <label className="block text-[var(--color-text-secondary)] text-sm mb-2">
                  Email <span className="text-[var(--color-accent-error)]">*</span>
                </label>
                <input
                  ref={emailInputRef}
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="your@email.com"
                  required
                  disabled={formState === 'submitting'}
                  className="waitlist-input w-full"
                />
              </div>

              {/* Priority Access Callout */}
              <div className="flex items-center gap-2 p-3 rounded-lg border border-[var(--color-accent-warning)]/30 bg-[var(--color-accent-warning)]/5">
                <span className="text-[var(--color-accent-warning)]">⚡</span>
                <span className="text-[var(--color-text-secondary)] text-sm">
                  <span className="font-semibold text-[var(--color-accent-warning)]">Priority access:</span>{' '}
                  Follow <a href="https://x.com/fcoury" target="_blank" rel="noopener" className="text-[var(--color-accent-primary)] hover:underline">@fcoury</a> on X to skip the line
                </span>
              </div>

              {/* X Connection */}
              <div className="pt-2">
                <div className="flex items-center gap-4 mb-3">
                  <div className="flex-1 h-px bg-[var(--color-border-default)]"></div>
                  <span className="text-[var(--color-text-muted)] text-xs">connect with X</span>
                  <div className="flex-1 h-px bg-[var(--color-border-default)]"></div>
                </div>

                {xUser ? (
                  <div className="flex items-center justify-between p-3 rounded-lg border border-[var(--color-accent-success)]/30 bg-[var(--color-accent-success)]/5">
                    <div className="flex items-center gap-3">
                      <svg className="w-5 h-5 text-[var(--color-text-secondary)]" fill="currentColor" viewBox="0 0 24 24">
                        <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
                      </svg>
                      <span className="text-[var(--color-accent-success)] font-mono text-sm">
                        @{xUser.handle}
                      </span>
                    </div>
                    <button
                      type="button"
                      onClick={disconnectX}
                      className="text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)] text-xs"
                    >
                      Disconnect
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={handleXConnect}
                    className="w-full flex items-center justify-center gap-3 px-4 py-3 rounded-lg border border-[var(--color-border-default)] hover:border-[var(--color-text-secondary)] transition-colors bg-[var(--color-bg-base)]"
                  >
                    <svg className="w-5 h-5 text-[var(--color-text-secondary)]" fill="currentColor" viewBox="0 0 24 24">
                      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
                    </svg>
                    <span className="text-[var(--color-text-secondary)] font-mono text-sm">
                      Connect with X
                    </span>
                  </button>
                )}
              </div>

              {/* Error message */}
              {formState === 'error' && (
                <p className="text-[var(--color-accent-error)] text-sm text-center">
                  {errorMessage}
                </p>
              )}

              {/* Submit button */}
              <button
                type="submit"
                disabled={formState === 'submitting' || !email.trim()}
                className="btn-primary w-full mt-4 disabled:opacity-30 disabled:cursor-not-allowed disabled:grayscale"
              >
                {formState === 'submitting' ? (
                  <span className="flex items-center justify-center gap-2">
                    <svg className="w-4 h-4 animate-spin" viewBox="0 0 24 24" fill="none">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                    </svg>
                    Joining...
                  </span>
                ) : !email.trim() ? (
                  'Enter email to join'
                ) : (
                  'Join Waitlist'
                )}
              </button>
            </form>
          </div>
        </div>
      )}
    </div>
  )
}
