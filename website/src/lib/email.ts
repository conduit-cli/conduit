/**
 * Email sending utilities using Resend
 */
import { Resend } from 'resend'
import WaitlistWelcome from '../emails/WaitlistWelcome'
import InviteEmail from '../emails/InviteEmail'
import WelcomeEmail from '../emails/WelcomeEmail'

export async function sendWaitlistWelcomeEmail(
  email: string,
  position: number,
  twitterConnected: boolean
): Promise<{ success: boolean; error?: string }> {
  const apiKey = import.meta.env.RESEND_API_KEY

  if (!apiKey) {
    console.error('RESEND_API_KEY not configured')
    return { success: false, error: 'Email service not configured' }
  }

  const resend = new Resend(apiKey)

  try {
    const { error } = await resend.emails.send({
      from: 'Felipe Coury <felipe@getconduit.sh>',
      to: email,
      subject: `You're #${position} on the Conduit waitlist`,
      react: WaitlistWelcome({ position, twitterConnected }),
    })

    if (error) {
      console.error('Resend error:', error)
      return { success: false, error: error.message }
    }

    console.log(`Welcome email sent to ${email} (position #${position})`)
    return { success: true }
  } catch (err) {
    console.error('Email send error:', err)
    return { success: false, error: 'Failed to send email' }
  }
}

export async function sendInviteEmail(
  email: string,
  inviteUrl: string
): Promise<{ success: boolean; error?: string }> {
  const apiKey = import.meta.env.RESEND_API_KEY

  if (!apiKey) {
    console.error('RESEND_API_KEY not configured')
    return { success: false, error: 'Email service not configured' }
  }

  const resend = new Resend(apiKey)

  try {
    const { error } = await resend.emails.send({
      from: 'Felipe Coury <felipe@getconduit.sh>',
      to: email,
      subject: "You're invited to access Conduit",
      react: InviteEmail({ inviteUrl }),
    })

    if (error) {
      console.error('Resend error:', error)
      return { success: false, error: error.message }
    }

    console.log(`Invite email sent to ${email}`)
    return { success: true }
  } catch (err) {
    console.error('Email send error:', err)
    return { success: false, error: 'Failed to send email' }
  }
}

export async function sendWelcomeEmail(
  email: string,
  githubUsername: string
): Promise<{ success: boolean; error?: string }> {
  const apiKey = import.meta.env.RESEND_API_KEY

  if (!apiKey) {
    console.error('RESEND_API_KEY not configured')
    return { success: false, error: 'Email service not configured' }
  }

  const resend = new Resend(apiKey)

  try {
    const { error } = await resend.emails.send({
      from: 'Felipe Coury <felipe@getconduit.sh>',
      to: email,
      subject: 'Welcome to Conduit - Getting Started',
      react: WelcomeEmail({ githubUsername }),
    })

    if (error) {
      console.error('Resend error:', error)
      return { success: false, error: error.message }
    }

    console.log(`Welcome email sent to ${email} (${githubUsername})`)
    return { success: true }
  } catch (err) {
    console.error('Email send error:', err)
    return { success: false, error: 'Failed to send email' }
  }
}
