/**
 * Waitlist API endpoint
 * Handles signups and returns position
 */
import type { APIRoute } from 'astro'
import { createClient } from '@supabase/supabase-js'
import { sendWaitlistWelcomeEmail } from '../../lib/email'

const supabaseUrl = import.meta.env.PUBLIC_SUPABASE_URL
const supabaseKey = import.meta.env.PUBLIC_SUPABASE_ANON_KEY

export const POST: APIRoute = async ({ request }) => {
  if (!supabaseUrl || !supabaseKey) {
    return new Response(JSON.stringify({ error: 'Server configuration error' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json' }
    })
  }

  const supabase = createClient(supabaseUrl, supabaseKey)

  try {
    const body = await request.json()
    const { email, twitter_handle, twitter_id, twitter_verified } = body

    if (!email || !email.trim()) {
      return new Response(JSON.stringify({ error: 'Email is required' }), {
        status: 400,
        headers: { 'Content-Type': 'application/json' }
      })
    }

    // Insert into waitlist
    const { data, error } = await supabase
      .from('waitlist')
      .insert({
        email: email.trim().toLowerCase(),
        twitter_handle: twitter_handle || null,
        twitter_id: twitter_id || null,
        twitter_verified: twitter_verified || false
      })
      .select('id, created_at')
      .single()

    if (error) {
      // Duplicate email
      if (error.code === '23505') {
        // Get existing entry's position
        const { data: existing } = await supabase
          .from('waitlist')
          .select('id, created_at')
          .eq('email', email.trim().toLowerCase())
          .single()

        if (existing) {
          const position = await getPosition(supabase, existing.created_at)
          return new Response(JSON.stringify({
            error: 'already_registered',
            message: 'This email is already on the waitlist!',
            position
          }), {
            status: 409,
            headers: { 'Content-Type': 'application/json' }
          })
        }
      }

      console.error('Supabase error:', error)
      return new Response(JSON.stringify({ error: error.message }), {
        status: 500,
        headers: { 'Content-Type': 'application/json' }
      })
    }

    // Calculate position
    const position = await getPosition(supabase, data.created_at)

    // Send welcome email (fire and forget - don't block the response)
    sendWaitlistWelcomeEmail(
      email.trim().toLowerCase(),
      position,
      !!twitter_verified
    ).catch(err => console.error('Failed to send welcome email:', err))

    return new Response(JSON.stringify({
      success: true,
      position,
      twitter_connected: !!twitter_verified
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' }
    })

  } catch (err) {
    console.error('API error:', err)
    return new Response(JSON.stringify({ error: 'Internal server error' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json' }
    })
  }
}

async function getPosition(supabase: any, createdAt: string): Promise<number> {
  // Count how many entries were created before this one
  const { count } = await supabase
    .from('waitlist')
    .select('*', { count: 'exact', head: true })
    .lte('created_at', createdAt)

  return count || 1
}
