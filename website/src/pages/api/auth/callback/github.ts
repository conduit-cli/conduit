/**
 * GitHub OAuth 2.0 Callback Handler
 * Exchanges code for token, gets user info, and processes invite acceptance
 */
import type { APIRoute } from 'astro'
import { createClient } from '@supabase/supabase-js'
import { sendWelcomeEmail } from '../../../../lib/email'

export const GET: APIRoute = async ({ url, cookies, redirect }) => {
  const code = url.searchParams.get('code')
  const state = url.searchParams.get('state')
  const error = url.searchParams.get('error')

  // Handle errors from GitHub
  if (error) {
    console.error('GitHub OAuth error:', error)
    return redirect('/invite/error?reason=github_auth_failed')
  }

  // Verify state
  const storedState = cookies.get('github_oauth_state')?.value
  if (!state || state !== storedState) {
    console.error('State mismatch')
    return redirect('/invite/error?reason=invalid_state')
  }

  // Get code verifier
  const codeVerifier = cookies.get('github_code_verifier')?.value
  if (!codeVerifier) {
    console.error('Missing code verifier')
    return redirect('/invite/error?reason=missing_verifier')
  }

  // Get invite token
  const inviteToken = cookies.get('invite_token')?.value
  if (!inviteToken) {
    console.error('Missing invite token')
    return redirect('/invite/error?reason=missing_invite')
  }

  const clientId = import.meta.env.GITHUB_CLIENT_ID
  const clientSecret = import.meta.env.GITHUB_CLIENT_SECRET

  try {
    // Exchange code for token
    const tokenResponse = await fetch(
      'https://github.com/login/oauth/access_token',
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'application/json',
        },
        body: JSON.stringify({
          client_id: clientId,
          client_secret: clientSecret,
          code,
          redirect_uri: `${url.origin}/api/auth/callback/github`,
          code_verifier: codeVerifier,
        }),
      }
    )

    if (!tokenResponse.ok) {
      const errorText = await tokenResponse.text()
      console.error(
        'Token exchange failed:',
        tokenResponse.status,
        errorText
      )
      return redirect('/invite/error?reason=token_exchange_failed')
    }

    const tokenData = await tokenResponse.json()

    if (tokenData.error) {
      console.error('Token error:', tokenData.error)
      return redirect('/invite/error?reason=token_error')
    }

    const accessToken = tokenData.access_token

    if (!accessToken) {
      console.error('No access token in response:', tokenData)
      return redirect('/invite/error?reason=no_access_token')
    }

    // Get user info from GitHub
    const userResponse = await fetch('https://api.github.com/user', {
      headers: {
        Authorization: `Bearer ${accessToken}`,
        Accept: 'application/vnd.github+json',
        'User-Agent': 'Conduit-Invite',
      },
    })

    if (!userResponse.ok) {
      const errorText = await userResponse.text()
      console.error(
        'Failed to get user info:',
        userResponse.status,
        errorText
      )
      return redirect('/invite/error?reason=user_fetch_failed')
    }

    const userData = await userResponse.json()
    const githubUsername = userData.login

    // Clear OAuth cookies
    cookies.delete('github_oauth_state', { path: '/' })
    cookies.delete('github_code_verifier', { path: '/' })
    cookies.delete('invite_token', { path: '/' })

    // Now process the invite acceptance
    const supabaseUrl = import.meta.env.PUBLIC_SUPABASE_URL
    const supabaseKey = import.meta.env.PUBLIC_SUPABASE_ANON_KEY
    const githubPat = import.meta.env.GITHUB_PAT

    if (!supabaseUrl || !supabaseKey) {
      console.error('Supabase not configured')
      return redirect('/invite/error?reason=server_error')
    }

    const supabase = createClient(supabaseUrl, supabaseKey)

    // Validate invite token
    const { data: invite, error: inviteError } = await supabase
      .from('invite_tokens')
      .select('*, waitlist(*)')
      .eq('token', inviteToken)
      .single()

    if (inviteError || !invite) {
      console.error('Invalid invite token:', inviteError)
      return redirect('/invite/error?reason=invalid_token')
    }

    // Check if already used
    if (invite.used_at) {
      return redirect('/invite/error?reason=already_used')
    }

    // Check if expired
    if (new Date(invite.expires_at) < new Date()) {
      return redirect('/invite/error?reason=expired')
    }

    // Add user to GitHub repository
    if (githubPat) {
      const repoResponse = await fetch(
        `https://api.github.com/repos/conduit-cli/conduit/collaborators/${githubUsername}`,
        {
          method: 'PUT',
          headers: {
            Authorization: `Bearer ${githubPat}`,
            Accept: 'application/vnd.github+json',
            'User-Agent': 'Conduit-Invite',
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            permission: 'pull',
          }),
        }
      )

      if (!repoResponse.ok && repoResponse.status !== 201 && repoResponse.status !== 204) {
        const errorText = await repoResponse.text()
        console.error(
          'Failed to add collaborator:',
          repoResponse.status,
          errorText
        )
        return redirect(`/invite/error?reason=repo_access_failed&status=${repoResponse.status}&detail=${encodeURIComponent(errorText.slice(0, 200))}`)
      }

      console.log(`Added ${githubUsername} as collaborator to conduit-cli/conduit`)
    } else {
      console.warn('GITHUB_PAT not configured, skipping repo access')
      return redirect('/invite/error?reason=repo_access_failed&status=0&detail=GITHUB_PAT_not_configured')
    }

    // Mark invite as used
    const now = new Date().toISOString()
    await supabase
      .from('invite_tokens')
      .update({ used_at: now })
      .eq('id', invite.id)

    // Update waitlist entry
    await supabase
      .from('waitlist')
      .update({
        accepted_at: now,
        github_username: githubUsername,
      })
      .eq('id', invite.waitlist_id)

    // Send welcome email with getting started links
    const userEmail = invite.waitlist?.email
    if (userEmail) {
      const emailResult = await sendWelcomeEmail(userEmail, githubUsername)
      if (!emailResult.success) {
        console.error('Failed to send welcome email:', emailResult.error)
        // Don't fail the flow, just log the error
      }
    }

    // Redirect to success page
    return redirect(`/invite/success?username=${encodeURIComponent(githubUsername)}`)
  } catch (err) {
    console.error('OAuth error:', err)
    return redirect('/invite/error?reason=oauth_error')
  }
}
