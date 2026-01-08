/**
 * CRTBoot - CRT Monitor Power-On Animation
 *
 * Unified animation where content is always visible but transforms
 * from "phosphor warming up" (bright/washed out) to stable image.
 *
 * The ignition beam overlay fades out as content "burns in".
 */

import { useEffect, useState } from 'react'

interface CRTBootProps {
  children: React.ReactNode
  ignitionDuration?: number // Phase 1: dot → line → square (ms)
  revealDuration?: number   // Phase 2: overlay fades, content stabilizes (ms)
  onComplete?: () => void
}

type Phase = 'ignition' | 'reveal' | 'complete'

export default function CRTBoot({
  children,
  ignitionDuration = 1800,
  revealDuration = 1200,
  onComplete,
}: CRTBootProps) {
  const [phase, setPhase] = useState<Phase>('ignition')

  useEffect(() => {
    // Phase 1→2: Ignition complete, start reveal
    const ignitionTimer = setTimeout(() => {
      setPhase('reveal')
    }, ignitionDuration)

    // Phase 2→3: Animation complete
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
  const isComplete = phase === 'complete'

  return (
    <>
      {/* Ignition overlay - fades out during reveal phase */}
      {!isComplete && (
        <div
          className={`crt-ignition-overlay ${isRevealing ? 'crt-ignition-fading' : ''}`}
          aria-hidden="true"
        >
          {/* The bright dot/line/square beam */}
          <div className={`crt-ignition-beam ${isRevealing ? 'crt-beam-fading' : ''}`} />
          {/* Noise texture */}
          <div className={`crt-ignition-noise ${isRevealing ? 'crt-noise-fading' : ''}`} />
        </div>
      )}

      {/* Terminal content - always rendered, with burn-in effect during ignition */}
      <div className={`crt-boot-wrapper ${isIgnition ? 'crt-burnin-phase' : ''} ${isRevealing ? 'crt-reveal-phase' : ''}`}>
        {/* Scanline sweep during reveal */}
        {isRevealing && <div className="crt-boot-scanline" aria-hidden="true" />}

        {/* Content with burn-in animation */}
        <div className={`crt-boot-content ${isIgnition ? 'crt-content-burnin' : ''} ${isRevealing ? 'crt-content-stabilizing' : ''}`}>
          {children}
        </div>
      </div>
    </>
  )
}
