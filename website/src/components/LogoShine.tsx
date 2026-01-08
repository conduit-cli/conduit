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

// Animation constants
const BAND_WIDTH = 5
const SPEED = 3 // diagonal units per frame (slower sweep)

// Colors - brighter base with enhanced shine effect
const COLORS = {
  SHINE_PEAK: [255, 255, 255] as const,
  SHINE_CENTER: [240, 245, 255] as const,
  SHINE_MID: [210, 220, 245] as const,
  SHINE_EDGE: [170, 185, 220] as const,
  TEXT_MUTED: [145, 160, 195] as const, // Brighter blue-gray base
}

// Calculate logo dimensions
const LOGO_WIDTH = Math.max(...LOGO_LINES.map(line => line.length))
const LOGO_HEIGHT = LOGO_LINES.length
const TOTAL_DIAGONAL = LOGO_WIDTH + LOGO_HEIGHT + BAND_WIDTH
const SWEEP_FRAMES = Math.ceil(TOTAL_DIAGONAL / SPEED)

// Timing (in ms)
const TICK_MS = 50
const MIN_INTERVAL_TICKS = 120 // ~6 seconds between shines
const MAX_INTERVAL_TICKS = 200 // ~10 seconds between shines
const INITIAL_DELAY_TICKS = 5 // ~0.25 seconds - shine appears quickly on load

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
    const colorStyle = rgbToStyle(color)

    // Add glow effect during shine (when distance is small)
    const isShining = distance <= BAND_WIDTH
    const glowIntensity = isShining ? Math.max(0, 1 - distance / BAND_WIDTH) : 0
    const glowColor = `rgba(180, 200, 255, ${glowIntensity * 0.6})`

    return (
      <span
        key={`${x}-${y}`}
        style={{
          color: colorStyle,
          textShadow: isShining
            ? `1px 0 0 ${colorStyle}, -1px 0 0 ${colorStyle}, 0 1px 0 ${colorStyle}, 0 -1px 0 ${colorStyle}, 0 0 ${8 + glowIntensity * 12}px ${glowColor}, 0 0 ${4 + glowIntensity * 8}px ${glowColor}`
            : `1px 0 0 ${colorStyle}, -1px 0 0 ${colorStyle}, 0 1px 0 ${colorStyle}, 0 -1px 0 ${colorStyle}`,
        }}
      >
        {char}
      </span>
    )
  }

  return (
    <div
      className={`font-mono leading-none select-none w-full flex justify-center ${className}`}
      aria-label="Conduit logo"
    >
      <div
        style={{
          fontSize: 'clamp(6px, 2vw, 16px)',
          whiteSpace: 'pre',
          letterSpacing: '-0.08em',
          lineHeight: '1.05',
          transform: 'scale(var(--logo-scale, 1))',
          transformOrigin: 'center center',
        }}
        className="logo-text"
      >
        {LOGO_LINES.map((line, y) => (
          <div key={y}>
            {line.split('').map((char, x) => renderChar(char, x, y))}
          </div>
        ))}
      </div>
    </div>
  )
}
