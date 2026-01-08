/**
 * WaitlistForm - Email capture form for pre-launch teaser
 *
 * Collects email and submits directly to Supabase.
 * Uses client-side Supabase calls for simplicity (no server needed).
 */

import { useState } from 'react'
import { createClient } from '@supabase/supabase-js'

type FormState = 'idle' | 'submitting' | 'success' | 'error' | 'duplicate'

// Supabase client - uses public anon key (safe for client-side)
const supabaseUrl = import.meta.env.PUBLIC_SUPABASE_URL
const supabaseAnonKey = import.meta.env.PUBLIC_SUPABASE_ANON_KEY

const supabase = supabaseUrl && supabaseAnonKey
  ? createClient(supabaseUrl, supabaseAnonKey)
  : null

interface WaitlistFormProps {
  className?: string
}

export default function WaitlistForm({ className = '' }: WaitlistFormProps) {
  const [email, setEmail] = useState('')
  const [formState, setFormState] = useState<FormState>('idle')
  const [errorMessage, setErrorMessage] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (!email.trim()) return

    // If Supabase isn't configured, show success anyway (dev mode)
    if (!supabase) {
      console.log('Waitlist signup (Supabase not configured):', email)
      setFormState('success')
      setEmail('')
      return
    }

    setFormState('submitting')
    setErrorMessage('')

    try {
      const { error } = await supabase
        .from('waitlist')
        .insert({ email: email.trim().toLowerCase() })

      if (error) {
        // Duplicate email (unique constraint violation)
        if (error.code === '23505') {
          setFormState('duplicate')
        } else {
          setFormState('error')
          setErrorMessage(error.message || 'Something went wrong')
        }
      } else {
        setFormState('success')
        setEmail('')
      }
    } catch {
      setFormState('error')
      setErrorMessage('Network error. Please try again.')
    }
  }

  if (formState === 'success') {
    return (
      <div className={`text-center ${className}`}>
        <div className="inline-flex items-center gap-2 px-6 py-3 rounded-lg bg-[var(--color-accent-success)]/10 border border-[var(--color-accent-success)]/30">
          <svg className="w-5 h-5 text-[var(--color-accent-success)]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
          </svg>
          <span className="text-[var(--color-accent-success)] font-mono">You're on the list!</span>
        </div>
        <p className="text-[var(--color-text-muted)] text-sm mt-3 font-mono">
          We'll notify you when Conduit launches.
        </p>
      </div>
    )
  }

  return (
    <form onSubmit={handleSubmit} className={`w-full max-w-md ${className}`}>
      <div className="flex flex-col sm:flex-row gap-3">
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="Enter your email..."
          required
          disabled={formState === 'submitting'}
          className="waitlist-input flex-1"
        />
        <button
          type="submit"
          disabled={formState === 'submitting' || !email.trim()}
          className="btn-primary whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {formState === 'submitting' ? (
            <span className="flex items-center gap-2">
              <svg className="w-4 h-4 animate-spin" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
              </svg>
              Joining...
            </span>
          ) : (
            'Join Waitlist'
          )}
        </button>
      </div>

      {/* Error states */}
      {formState === 'duplicate' && (
        <p className="text-[var(--color-accent-warning)] text-sm mt-3 font-mono text-center">
          This email is already on the waitlist!
        </p>
      )}
      {formState === 'error' && (
        <p className="text-[var(--color-accent-error)] text-sm mt-3 font-mono text-center">
          {errorMessage}
        </p>
      )}
    </form>
  )
}
