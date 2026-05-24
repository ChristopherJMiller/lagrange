import { useEffect, useState } from 'react'
import { useUi } from '../store/ui'
import { utcClock, relativeTime } from '../lib/time'
import { StatusDot } from './ui/StatusDot'
import { cn } from '../lib/cn'

export function Header() {
  const health = useUi((s) => s.health)
  const lastFetched = useUi((s) => s.lastFetched)
  const vmsError = useUi((s) => s.vmsError)

  const [, force] = useState(0)
  useEffect(() => {
    const id = window.setInterval(() => force((x) => x + 1), 1000)
    return () => window.clearInterval(id)
  }, [])

  const upDot = vmsError ? 'red' : health ? 'green' : 'amber'
  const upLabel = vmsError ? 'LINK DOWN' : health ? 'NOMINAL' : 'SYNCING'

  return (
    <header className="relative z-10 border-b border-border bg-bg/70 backdrop-blur-sm">
      <div className="mx-auto flex w-full max-w-[1600px] flex-wrap items-center gap-x-6 gap-y-2 px-6 lg:px-10 py-3">
        {/* Logo / wordmark */}
        <div className="flex items-baseline gap-3">
          <svg width="22" height="22" viewBox="0 0 64 64" className="shrink-0">
            <ellipse cx="32" cy="32" rx="26" ry="10" stroke="#ffb454" strokeWidth="2" fill="none" />
            <ellipse
              cx="32"
              cy="32"
              rx="10"
              ry="26"
              stroke="#56d3ff"
              strokeWidth="2"
              fill="none"
              transform="rotate(32 32 32)"
            />
            <circle cx="32" cy="32" r="3.5" fill="#ffb454" />
          </svg>
          <div className="flex items-baseline gap-2">
            <span className="text-base font-bold tracking-widest">ORBIT</span>
            <span className="text-cyan/60 text-xs">//</span>
            <span className="text-xs uppercase tracking-widest text-dim">
              LAGRANGE Mission Control
            </span>
          </div>
        </div>

        <div className="hidden sm:block divider-dotted h-px flex-1 self-center" />

        {/* Link status */}
        <div className="flex items-center gap-2">
          <StatusDot variant={upDot} pulse={upDot !== 'red'} />
          <span
            className={cn(
              'text-[10px] uppercase tracking-widest',
              upDot === 'red' ? 'text-red' : upDot === 'amber' ? 'text-amber' : 'text-green',
            )}
          >
            {upLabel}
          </span>
        </div>

        {/* Fleet counts */}
        <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-widest text-dim">
          <span>FLEET</span>
          <span className="font-bold text-text tabular-nums">
            {health ? `${health.vms_running}/${health.vms_total}` : '—/—'}
          </span>
        </div>

        {/* Last sync */}
        <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-widest text-dim">
          <span>SYNC</span>
          <span className="tabular-nums text-text/80">
            {lastFetched ? relativeTime(new Date(lastFetched).toISOString()) : '—'}
          </span>
        </div>

        {/* Clock */}
        <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-widest text-dim">
          <span>T+</span>
          <span className="tabular-nums text-text">{utcClock()}</span>
        </div>
      </div>
    </header>
  )
}
