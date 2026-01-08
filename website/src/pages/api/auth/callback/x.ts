/**
 * X/Twitter OAuth 2.0 Callback Handler
 * Exchanges code for token and gets user info
 */
import type { APIRoute } from 'astro'

export const GET: APIRoute = async ({ url, cookies, redirect }) => {
  const code = url.searchParams.get('code')
  const state = url.searchParams.get('state')
  const error = url.searchParams.get('error')

  // Handle errors from X
  if (error) {
    console.error('X OAuth error:', error)
    return redirect('/?error=x_auth_failed')
  }

  // Verify state
  const storedState = cookies.get('x_oauth_state')?.value
  if (!state || state !== storedState) {
    console.error('State mismatch')
    return redirect('/?error=invalid_state')
  }

  // Get code verifier
  const codeVerifier = cookies.get('x_code_verifier')?.value
  if (!codeVerifier) {
    console.error('Missing code verifier')
    return redirect('/?error=missing_verifier')
  }

  try {
    // Exchange code for token
    const tokenResponse = await fetch('https://api.twitter.com/2/oauth2/token', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        'Authorization': `Basic ${btoa(`${import.meta.env.X_CLIENT_ID}:${import.meta.env.X_CLIENT_SECRET}`)}`
      },
      body: new URLSearchParams({
        code: code!,
        grant_type: 'authorization_code',
        redirect_uri: `${url.origin}/api/auth/callback/x`,
        code_verifier: codeVerifier
      })
    })

    if (!tokenResponse.ok) {
      const errorText = await tokenResponse.text()
      console.error('Token exchange failed:', tokenResponse.status, errorText)
      return redirect('/?error=token_exchange_failed')
    }

    const tokenData = await tokenResponse.json()
    console.log('Token exchange successful, got token data')
    const accessToken = tokenData.access_token

    if (!accessToken) {
      console.error('No access token in response:', tokenData)
      return redirect('/?error=no_access_token')
    }

    // Get user info from X
    console.log('Fetching user info from X API...')
    const userResponse = await fetch('https://api.twitter.com/2/users/me', {
      headers: {
        'Authorization': `Bearer ${accessToken}`
      }
    })

    if (!userResponse.ok) {
      const errorText = await userResponse.text()
      console.error('Failed to get user info:', userResponse.status, errorText)
      return redirect('/?error=user_fetch_failed')
    }

    console.log('Got user info from X API')

    const userData = await userResponse.json()
    const twitterId = userData.data.id
    const twitterHandle = userData.data.username

    // Clear OAuth cookies
    cookies.delete('x_oauth_state', { path: '/' })
    cookies.delete('x_code_verifier', { path: '/' })

    // Pass X user data in URL params for frontend to handle
    const userDataParam = encodeURIComponent(JSON.stringify({
      id: twitterId,
      handle: twitterHandle
    }))

    // Redirect back to home with success and user data
    return redirect(`/?x_connected=true&x_user=${userDataParam}`)

  } catch (err) {
    console.error('OAuth error:', err)
    return redirect('/?error=oauth_error')
  }
}
