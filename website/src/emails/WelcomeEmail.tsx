/**
 * Welcome Email Template
 * Sent after a user accepts their invite and gets repo access
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
  Button,
} from '@react-email/components'

interface WelcomeEmailProps {
  githubUsername: string
}

export default function WelcomeEmail({ githubUsername }: WelcomeEmailProps) {
  return (
    <Html>
      <Head />
      <Preview>Welcome to Conduit - Here's how to get started</Preview>
      <Body style={styles.body}>
        <Container style={styles.container}>
          {/* Logo/Header */}
          <Text style={styles.logo}>CONDUIT</Text>

          {/* Main content */}
          <Section style={styles.card}>
            <Text style={styles.heading}>Welcome, {githubUsername}!</Text>

            <Text style={styles.description}>
              You now have access to Conduit. Here's everything you need to get
              started with your AI agent team.
            </Text>

            <Hr style={styles.divider} />

            {/* Repository Access */}
            <Section style={styles.linkSection}>
              <Text style={styles.linkLabel}>📦 Repository</Text>
              <Text style={styles.linkDescription}>
                Clone the repo and follow the README to install Conduit
              </Text>
              <Button
                href="https://github.com/conduit-cli/conduit"
                style={styles.secondaryButton}
              >
                View Repository
              </Button>
            </Section>

            {/* Documentation */}
            <Section style={styles.linkSection}>
              <Text style={styles.linkLabel}>📚 Documentation</Text>
              <Text style={styles.linkDescription}>
                Learn how to configure and use Conduit effectively
              </Text>
              <Button
                href="https://getconduit.sh/docs"
                style={styles.secondaryButton}
              >
                Read the Docs
              </Button>
            </Section>

            {/* Discord */}
            <Section style={styles.linkSection}>
              <Text style={styles.linkLabel}>💬 Discord Community</Text>
              <Text style={styles.linkDescription}>
                Join other early adopters, get help, and share feedback
              </Text>
              <Button
                href="https://discord.gg/conduit"
                style={styles.button}
              >
                Join Discord
              </Button>
            </Section>

            <Hr style={styles.divider} />

            <Text style={styles.instructions}>
              Questions? Reply to this email or reach out on Discord.
              We'd love to hear what you build with Conduit!
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
    fontSize: '24px',
    fontWeight: '600' as const,
    margin: '0 0 20px 0',
    textAlign: 'center' as const,
  },
  description: {
    color: '#a0a0b0',
    fontSize: '14px',
    lineHeight: '1.6',
    margin: '0 0 24px 0',
    textAlign: 'center' as const,
  },
  divider: {
    borderColor: '#2a2a3a',
    borderWidth: '1px',
    margin: '24px 0',
  },
  linkSection: {
    marginBottom: '24px',
  },
  linkLabel: {
    color: '#e0e0e8',
    fontSize: '16px',
    fontWeight: '600' as const,
    margin: '0 0 8px 0',
  },
  linkDescription: {
    color: '#808090',
    fontSize: '13px',
    lineHeight: '1.5',
    margin: '0 0 12px 0',
  },
  button: {
    backgroundColor: '#00ff88',
    color: '#0a0a0f',
    padding: '12px 24px',
    borderRadius: '6px',
    fontSize: '13px',
    fontWeight: 'bold' as const,
    textDecoration: 'none',
    display: 'inline-block',
  },
  secondaryButton: {
    backgroundColor: 'transparent',
    color: '#00ff88',
    padding: '10px 20px',
    borderRadius: '6px',
    fontSize: '13px',
    fontWeight: '600' as const,
    textDecoration: 'none',
    display: 'inline-block',
    border: '1px solid #00ff88',
  },
  instructions: {
    color: '#808090',
    fontSize: '13px',
    lineHeight: '1.5',
    margin: 0,
    textAlign: 'center' as const,
  },
  footer: {
    color: '#606070',
    fontSize: '12px',
    textAlign: 'center' as const,
    marginTop: '30px',
  },
}
