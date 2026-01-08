/**
 * CRTBoot - CRT Monitor Power-On Animation
 *
 * Wraps content and reveals it with an authentic CRT turn-on effect:
 * 1. Bright dot appears at center (electron gun warming up)
 * 2. Horizontal expansion into a line
 * 3. Vertical expansion with glitch artifacts
 * 4. Stabilizes to show full content
 */

import { useEffect, useState } from 'react'

interface CRTBootProps {
  children: React.ReactNode
  duration?: number // Total boot duration in ms
  onComplete?: () => void
}

export default function CRTBoot({
  children,
  duration = 1800,
  onComplete,
}: CRTBootProps) {
  const [phase, setPhase] = useState<'booting' | 'complete'>('booting')

  useEffect(() => {
    const timer = setTimeout(() => {
      setPhase('complete')
      onComplete?.()
    }, duration)

    return () => clearTimeout(timer)
  }, [duration, onComplete])

  const isBooting = phase === 'booting'

  return (
    <div className={`crt-boot-wrapper ${isBooting ? 'crt-booting' : ''}`}>
      {/* Rolling scanline effect during boot */}
      {isBooting && <div className="crt-boot-scanline" aria-hidden="true" />}

      {/* Static noise overlay during boot */}
      {isBooting && <div className="crt-boot-noise" aria-hidden="true" />}

      {/* Content with clip-path reveal animation */}
      <div className="crt-boot-content">
        {children}
      </div>
    </div>
  )
}
