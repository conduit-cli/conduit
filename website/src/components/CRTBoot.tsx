/**
 * CRTBoot - CRT Monitor Power-On Animation
 *
 * Single unified animation where the content itself transforms from
 * a bright dot → line → rectangle → stable image.
 *
 * No separate overlay - the content IS the CRT beam.
 */

import { useEffect, useState } from 'react'

interface CRTBootProps {
  children: React.ReactNode
  duration?: number // Total animation duration (ms)
  onComplete?: () => void
}

type Phase = 'booting' | 'complete'

export default function CRTBoot({
  children,
  duration = 2500,
  onComplete,
}: CRTBootProps) {
  const [phase, setPhase] = useState<Phase>('booting')

  useEffect(() => {
    const timer = setTimeout(() => {
      setPhase('complete')
      onComplete?.()
    }, duration)

    return () => clearTimeout(timer)
  }, [duration, onComplete])

  const isBooting = phase === 'booting'

  return (
    <div className="crt-boot-wrapper">
      {/* Scanline sweep effect during boot */}
      {isBooting && <div className="crt-boot-scanline" aria-hidden="true" />}

      {/* Content with unified CRT boot animation */}
      <div className={`crt-boot-content ${isBooting ? 'crt-unified-boot' : ''}`}>
        {children}
      </div>
    </div>
  )
}
