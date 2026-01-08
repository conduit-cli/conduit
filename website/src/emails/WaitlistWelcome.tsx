/**
 * Waitlist Welcome Email Template
 * Sent when a user joins the Conduit waitlist
 */
import {
  Html,
  Head,
  Body,
  Container,
  Text,
  Link,
  Preview,
  Section,
  Hr,
} from '@react-email/components'

interface WaitlistWelcomeProps {
  position: number
  twitterConnected: boolean
}

export default function WaitlistWelcome({ position, twitterConnected }: WaitlistWelcomeProps) {
  return (
    <Html>
      <Head />
      <Preview>You're #{position} on the Conduit waitlist</Preview>
      <Body style={styles.body}>
        <Container style={styles.container}>
          {/* Logo/Header */}
          <Text style={styles.logo}>CONDUIT</Text>

          {/* Main content */}
          <Section style={styles.card}>
            <Text style={styles.heading}>You're on the list!</Text>

            <Text style={styles.position}>#{position}</Text>

            <Text style={styles.description}>
              We'll notify you when Conduit is ready to launch.
            </Text>

            <Hr style={styles.divider} />

            {/* Skip the line CTA */}
            <Text style={styles.ctaHeading}>⚡ Want to skip the line?</Text>
            <Text style={styles.ctaText}>
              Follow{' '}
              <Link href="https://x.com/fcoury" style={styles.link}>
                @fcoury
              </Link>{' '}
              on X for priority access.
            </Text>
          </Section>

          {/* Footer */}
          <Text style={styles.footer}>
            Conduit - Run a team of AI agents in your terminal
          </Text>
        </Container>
      </Body>
    </Html>
  )
}

const styles = {
  body: {
    backgroundColor: '#0a0a0f',
    fontFamily:
      'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
    margin: 0,
    padding: 0,
  },
  container: {
    padding: '40px 20px',
    maxWidth: '600px',
    margin: '0 auto',
  },
  logo: {
    color: '#00ff88',
    fontSize: '24px',
    fontWeight: 'bold' as const,
    textAlign: 'center' as const,
    margin: '0 0 30px 0',
    letterSpacing: '4px',
  },
  card: {
    backgroundColor: '#111118',
    padding: '30px',
    borderRadius: '8px',
    border: '1px solid #2a2a3a',
  },
  heading: {
    color: '#e0e0e8',
    fontSize: '18px',
    fontWeight: '600' as const,
    margin: '0 0 10px 0',
  },
  position: {
    color: '#00d4ff',
    fontSize: '48px',
    fontWeight: 'bold' as const,
    textAlign: 'center' as const,
    margin: '20px 0',
  },
  description: {
    color: '#a0a0b0',
    fontSize: '14px',
    lineHeight: '1.5',
    margin: '0 0 10px 0',
  },
  divider: {
    borderColor: '#2a2a3a',
    borderWidth: '1px',
    margin: '20px 0',
  },
  ctaHeading: {
    color: '#ffaa00',
    fontSize: '14px',
    fontWeight: '600' as const,
    margin: '0 0 8px 0',
  },
  ctaText: {
    color: '#a0a0b0',
    fontSize: '14px',
    lineHeight: '1.5',
    margin: 0,
  },
  link: {
    color: '#00d4ff',
    textDecoration: 'none',
  },
  footer: {
    color: '#606070',
    fontSize: '12px',
    textAlign: 'center' as const,
    marginTop: '30px',
  },
}
