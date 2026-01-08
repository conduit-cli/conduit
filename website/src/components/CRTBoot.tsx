/**
 * CRTBoot - CRT Monitor Power-On Animation
 *
 * Two-phase animation:
 * Phase 1 (Ignition): Full-page overlay with dot → line → square
 * Phase 2 (Reveal): Terminal frame content reveal
 */

import { useEffect, useState } from 'react'

interface CRTBootProps {
  children: React.ReactNode
  ignitionDuration?: number // Phase 1: dot → line → square (ms)
  revealDuration?: number   // Phase 2: terminal reveal (ms)
  onComplete?: () => void
}

type Phase = 'ignition' | 'reveal' | 'complete'

export default function CRTBoot({
  children,
  ignitionDuration = 1800,
  revealDuration = 1500,
  onComplete,
}: CRTBootProps) {
  const [phase, setPhase] = useState<Phase>('ignition')

  useEffect(() => {
    // Phase 1: Ignition (dot → line → square)
    const ignitionTimer = setTimeout(() => {
      setPhase('reveal')
    }, ignitionDuration)

    // Phase 2: Reveal (terminal content)
    const revealTimer = setTimeout(() => {
      setPhase('complete')
      onComplete?.()
    }, ignitionDuration + revealDuration)

    return () => {
      clearTimeout(ignitionTimer)
      clearTimeout(revealTimer)
    }
  }, [ignitionDuration, revealDuration, onComplete])

  const isIgnition = phase === 'ignition'
  const isRevealing = phase === 'reveal'
  const isBooting = isIgnition || isRevealing

  return (
    <>
      {/* Phase 1: Full-page ignition overlay */}
      {isIgnition && (
        <div className="crt-ignition-overlay" aria-hidden="true">
          {/* The bright dot/line/square */}
          <div className="crt-ignition-beam" />
          {/* Noise that appears as square forms */}
          <div className="crt-ignition-noise" />
        </div>
      )}

      {/* Terminal content wrapper */}
      <div className={`crt-boot-wrapper ${isRevealing ? 'crt-revealing' : ''} ${isBooting ? 'crt-booting' : ''}`}>
        {/* Rolling scanline effect during reveal */}
        {isRevealing && <div className="crt-boot-scanline" aria-hidden="true" />}

        {/* Static noise overlay during reveal */}
        {isRevealing && <div className="crt-boot-noise" aria-hidden="true" />}

        {/* Content - hidden during ignition, revealed during reveal phase */}
        <div className={`crt-boot-content ${isIgnition ? 'crt-hidden' : ''}`}>
          {children}
        </div>
      </div>
    </>
  )
}
