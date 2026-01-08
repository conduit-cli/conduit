/**
 * LogoShine - Port of Conduit's Rust logo_shine.rs animation to React
 *
 * Creates a periodic "metallic shine" effect that sweeps diagonally
 * across the ASCII logo from top-left to bottom-right.
 *
 * Animation timing and colors are matched exactly to the Rust TUI version.
 */

import { useEffect, useRef, useState, useCallback } from 'react'

// The Conduit logo - exact match from logo_shine.rs
const LOGO_LINES = [
  "  ░██████                               ░██            ░██   ░██   ",
  " ░██   ░██                              ░██                  ░██   ",
  "░██         ░███████  ░████████   ░████████ ░██    ░██ ░██░████████",
  "░██        ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██ ░██   ░██   ",
  "░██        ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██ ░██   ░██   ",
  " ░██   ░██ ░██    ░██ ░██    ░██ ░██   ░███ ░██   ░███ ░██   ░██   ",
  "  ░██████   ░███████  ░██    ░██  ░█████░██  ░█████░██ ░██    ░████",
]

// Animation constants - matched to Rust implementation
const BAND_WIDTH = 5
const SPEED = 4 // diagonal units per frame

// Colors from theme.rs (RGB values)
const COLORS = {
  SHINE_PEAK: [255, 255, 255] as const,
  SHINE_CENTER: [230, 230, 245] as const,
  SHINE_MID: [180, 180, 200] as const,
  SHINE_EDGE: [130, 130, 150] as const,
  TEXT_MUTED: [100, 100, 120] as const,
}

// Calculate logo dimensions
const LOGO_WIDTH = Math.max(...LOGO_LINES.map(line => line.length))
const LOGO_HEIGHT = LOGO_LINES.length
const TOTAL_DIAGONAL = LOGO_WIDTH + LOGO_HEIGHT + BAND_WIDTH
const SWEEP_FRAMES = Math.ceil(TOTAL_DIAGONAL / SPEED)

// Timing (in ms) - matched to Rust at ~50ms per tick
const TICK_MS = 50
const MIN_INTERVAL_TICKS = 60 // ~3 seconds
const MAX_INTERVAL_TICKS = 100 // ~5 seconds
const INITIAL_DELAY_TICKS = 20 // ~1 second

function randomInterval(): number {
  return MIN_INTERVAL_TICKS + Math.floor(Math.random() * (MAX_INTERVAL_TICKS - MIN_INTERVAL_TICKS + 1))
}

function rgbToStyle(rgb: readonly [number, number, number]): string {
  return `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})`
}

function getColorForDistance(distance: number): readonly [number, number, number] {
  if (distance > BAND_WIDTH) return COLORS.TEXT_MUTED
  if (distance < 1) return COLORS.SHINE_PEAK
  if (distance < 2) return COLORS.SHINE_CENTER
  if (distance < 3) return COLORS.SHINE_MID
  return COLORS.SHINE_EDGE
}

interface LogoShineProps {
  className?: string
}

export default function LogoShine({ className = '' }: LogoShineProps) {
  const [frame, setFrame] = useState(0)
  const [intervalFrames, setIntervalFrames] = useState(randomInterval)
  const animationRef = useRef<number | null>(null)
  const lastTickRef = useRef<number>(0)

  // Initialize with short delay before first shine
  useEffect(() => {
    const initialFrame = SWEEP_FRAMES + intervalFrames - INITIAL_DELAY_TICKS
    setFrame(initialFrame)
  }, [])

  // Animation loop
  const tick = useCallback(() => {
    setFrame(prevFrame => {
      const totalFrames = SWEEP_FRAMES + intervalFrames
      const nextFrame = (prevFrame + 1) % totalFrames

      // When cycle completes, randomize the next interval
      if (nextFrame === 0) {
        setIntervalFrames(randomInterval())
      }

      return nextFrame
    })
  }, [intervalFrames])

  useEffect(() => {
    const animate = (timestamp: number) => {
      if (timestamp - lastTickRef.current >= TICK_MS) {
        tick()
        lastTickRef.current = timestamp
      }
      animationRef.current = requestAnimationFrame(animate)
    }

    animationRef.current = requestAnimationFrame(animate)

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current)
      }
    }
  }, [tick])

  // Calculate shine position
  const shinePosition = frame < SWEEP_FRAMES
    ? (frame / SWEEP_FRAMES) * (LOGO_WIDTH + LOGO_HEIGHT)
    : null

  // Render a single character with the appropriate color
  const renderChar = (char: string, x: number, y: number) => {
    // Space characters don't get shine effect
    if (char === ' ') {
      return (
        <span key={`${x}-${y}`} style={{ color: rgbToStyle(COLORS.TEXT_MUTED) }}>
          {char}
        </span>
      )
    }

    // Calculate diagonal and distance from shine
    const diagonal = x + y
    const distance = shinePosition !== null
      ? Math.abs(diagonal - shinePosition)
      : BAND_WIDTH + 1

    const color = getColorForDistance(distance)

    return (
      <span
        key={`${x}-${y}`}
        style={{
          color: rgbToStyle(color),
          transition: 'color 0.05s ease-out',
        }}
      >
        {char}
      </span>
    )
  }

  return (
    <div
      className={`font-mono leading-none select-none ${className}`}
      style={{
        fontSize: 'clamp(6px, 1.2vw, 14px)',
        whiteSpace: 'pre',
      }}
      aria-label="Conduit logo"
    >
      {LOGO_LINES.map((line, y) => (
        <div key={y} className="flex">
          {line.split('').map((char, x) => renderChar(char, x, y))}
        </div>
      ))}
    </div>
  )
}
