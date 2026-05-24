import { useUi } from '../store/ui'
import { SectionHeader } from './ui/Bracket'
import { cn } from '../lib/cn'
import { formatMem } from '../lib/time'

/**
 * Two utilization bars (vCPU + memory). vCPU is intentionally allowed
 * to exceed 100% — host cores back many idle guest cores via systemd
 * fair-share. Memory is the real ceiling.
 */
export function CapacityRow() {
  const cap = useUi((s) => s.capacity)
  const initialLoad = useUi((s) => s.initialLoad)

  if (initialLoad && !cap) {
    return (
      <section className="mt-6">
        <SectionHeader label="Host Capacity" meta="reading…" />
        <div className="mt-3 h-10 border border-border/40 bg-surface/40 animate-pulse-soft" />
      </section>
    )
  }
  if (!cap) return null

  const cpuPct = cap.host.cpus > 0 ? (cap.allocated.vcpu / cap.host.cpus) * 100 : 0
  const memPct = cap.host.mem_mb > 0 ? (cap.allocated.mem_mb / cap.host.mem_mb) * 100 : 0

  return (
    <section className="mt-6">
      <SectionHeader
        label="Host Capacity"
        meta={`${cap.allocated.vms} vessel${cap.allocated.vms === 1 ? '' : 's'} assigned`}
      />
      <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Bar
          label="vCPU"
          assigned={`${cap.allocated.vcpu} / ${cap.host.cpus}`}
          pct={cpuPct}
          note={cpuPct > 100 ? `overcommit ${cpuPct.toFixed(0)}%` : `${cpuPct.toFixed(0)}%`}
          allowOver={true}
        />
        <Bar
          label="Memory"
          assigned={`${formatMem(cap.allocated.mem_mb)} / ${formatMem(cap.host.mem_mb)}`}
          pct={memPct}
          note={`${memPct.toFixed(0)}%`}
          allowOver={false}
        />
      </div>
    </section>
  )
}

function Bar({
  label,
  assigned,
  pct,
  note,
  allowOver,
}: {
  label: string
  assigned: string
  pct: number
  note: string
  allowOver: boolean
}) {
  // Memory: hot zones light up early. vCPU: more tolerant.
  const danger = allowOver ? pct >= 200 : pct >= 95
  const warn = allowOver ? pct >= 100 : pct >= 80
  const fillColor = danger ? 'bg-red' : warn ? 'bg-amber' : 'bg-cyan'
  const textColor = danger ? 'text-red' : warn ? 'text-amber' : 'text-text'

  // Visual cap so we still see the bar at extreme over-commit
  const visualPct = Math.min(100, pct)

  return (
    <div className="relative border border-border bg-surface/60 px-3.5 py-2.5">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[10px] uppercase tracking-widest text-dim">{label}</span>
        <span className={cn('text-[10px] uppercase tracking-widest tabular-nums', textColor)}>
          {note}
        </span>
      </div>
      <div className="mt-1 text-[13px] font-mono tabular-nums text-text">{assigned}</div>
      <div className="mt-2 relative h-1.5 border border-border/60 bg-bg/60 overflow-hidden">
        <div
          className={cn('absolute left-0 top-0 h-full transition-all duration-500', fillColor)}
          style={{ width: `${visualPct}%` }}
        />
        {/* 100% tick */}
        {allowOver && (
          <div
            className="absolute top-0 h-full w-px bg-dim/40"
            style={{ left: '100%' }}
            aria-hidden
          />
        )}
      </div>
    </div>
  )
}
