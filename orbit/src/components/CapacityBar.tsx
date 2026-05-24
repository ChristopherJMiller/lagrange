import { useMemo } from 'react'
import type { CapacityDto } from '../api/types'
import { cn } from '../lib/cn'
import { formatMem } from '../lib/time'

type Resource = 'mem' | 'vcpu'

type Props = {
  cap: CapacityDto
  resource: Resource
  /** Optional phantom segment for a vessel the operator is about to deploy. */
  proposed?: { name: string; vcpu: number; mem_mb: number } | null
  /** Optional: highlight one existing vessel. */
  highlightName?: string | null
  /** Compact mode (smaller bar) for inline embeds like the deploy dialog. */
  compact?: boolean
}

type SegmentKind = 'vessel' | 'proposed' | 'free' | 'reserved' | 'overshoot'
type Segment = {
  key: string
  size: number
  label: string
  title: string
  kind: SegmentKind
  highlight?: boolean
  /** Index into VESSEL_PALETTE — only meaningful when kind === 'vessel'. */
  paletteIdx?: number
}

/**
 * gparted-style segmented bar.
 *
 *   [ vessel-a | vessel-b | ▸proposed | …free…             | reserved ]
 *
 * Memory bar: vessels + proposed + free + reserved sum to host.mem_mb.
 *   Over-allocation: the "free" region disappears and an OVER segment
 *   renders striped red so the operator can't miss it.
 *
 * vCPU bar uses the same layout against host.cpus. Overcommit on vCPU
 * is tolerated in reality (time-sliced) but visualized identically.
 */
export function CapacityBar({
  cap,
  resource,
  proposed,
  highlightName,
  compact,
}: Props) {
  const isMem = resource === 'mem'
  const total = isMem ? cap.host.mem_mb : cap.host.cpus
  const reserved = isMem ? cap.host.reserved_mem_mb : cap.host.reserved_vcpu
  const proposedSize = proposed ? (isMem ? proposed.mem_mb : proposed.vcpu) : 0
  const vessels = cap.vessels

  const vesselSum = vessels.reduce(
    (acc, v) => acc + (isMem ? v.mem_mb : v.vcpu),
    0,
  )
  const usedAfterProposed = vesselSum + proposedSize + reserved
  const overcommit = total > 0 && usedAfterProposed > total
  const overshootBy = overcommit ? usedAfterProposed - total : 0
  const free = Math.max(0, total - vesselSum - proposedSize - reserved)

  const segments: Segment[] = useMemo(() => {
    const out: Segment[] = vessels.map((v, i) => ({
      key: `vm-${v.name}`,
      size: isMem ? v.mem_mb : v.vcpu,
      label: v.name,
      title: `${v.name} · ${v.vcpu} vCPU · ${formatMem(v.mem_mb)}`,
      kind: 'vessel',
      highlight: highlightName === v.name,
      paletteIdx: nameToPalette(v.name, i),
    }))
    if (proposed) {
      out.push({
        key: 'proposed',
        size: proposedSize,
        label: proposed.name || 'new',
        title: `proposed · ${proposed.vcpu} vCPU · ${formatMem(proposed.mem_mb)}`,
        kind: 'proposed',
      })
    }
    if (!overcommit) {
      out.push({
        key: 'free',
        size: free,
        label: 'free',
        title: `free · ${isMem ? formatMem(free) : `${free} vCPU`}`,
        kind: 'free',
      })
    }
    out.push({
      key: 'reserved',
      size: reserved,
      label: 'host',
      title: `reserved for host · ${isMem ? formatMem(reserved) : `${reserved} vCPU`}`,
      kind: 'reserved',
    })
    if (overcommit) {
      out.push({
        key: 'over',
        size: overshootBy,
        label: 'OVER',
        title: `over capacity by ${isMem ? formatMem(overshootBy) : `${overshootBy} vCPU`}`,
        kind: 'overshoot',
      })
    }
    return out
  }, [
    vessels,
    proposed,
    proposedSize,
    free,
    reserved,
    overshootBy,
    overcommit,
    isMem,
    highlightName,
  ])

  // Normalize: when over, expand denominator so OVER segment is visible.
  const denominator = overcommit ? usedAfterProposed : total

  const barHeight = compact ? 'h-5' : 'h-7'
  return (
    <div className="w-full">
      <div
        className={cn(
          'relative flex w-full overflow-hidden border bg-bg/60',
          barHeight,
          overcommit ? 'border-red/60' : 'border-border',
        )}
        role="img"
        aria-label={`${isMem ? 'memory' : 'vCPU'} allocation`}
      >
        {segments.map((seg, idx) => {
          const pct = denominator > 0 ? (seg.size / denominator) * 100 : 0
          if (pct <= 0) return null
          return (
            <div
              key={seg.key}
              title={seg.title}
              className={cn(
                'flex items-center justify-center text-[9px] uppercase tracking-widest select-none overflow-hidden whitespace-nowrap relative',
                segmentClass(seg),
                idx > 0 && 'border-l border-bg/80',
              )}
              style={{ width: `${pct}%` }}
            >
              <span className="px-1 truncate">{seg.label}</span>
            </div>
          )
        })}
        {/* Total-capacity tick on overcommit */}
        {overcommit && (
          <div
            className="absolute top-0 bottom-0 w-px bg-text/60 pointer-events-none"
            style={{ left: `${(total / denominator) * 100}%` }}
            title="host capacity"
            aria-hidden
          />
        )}
      </div>

      <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-0.5 text-[10px] uppercase tracking-widest tabular-nums text-dim">
        <span>
          <span className="text-text">
            {isMem ? formatMem(vesselSum + proposedSize) : `${vesselSum + proposedSize}`}
          </span>{' '}
          / <span className="text-text">{isMem ? formatMem(total) : `${total}`}</span>
        </span>
        <span className="text-cyan/70">
          {vessels.length} vessel{vessels.length === 1 ? '' : 's'}
        </span>
        {proposed && (
          <span className="text-amber">
            + proposed {isMem ? formatMem(proposed.mem_mb) : `${proposed.vcpu} vCPU`}
          </span>
        )}
        <span className="text-dimmer">
          host reserves {isMem ? formatMem(reserved) : `${reserved} vCPU`}
        </span>
        {overcommit && (
          <span className="text-red">
            ! over by {isMem ? formatMem(overshootBy) : `${overshootBy} vCPU`}
          </span>
        )}
      </div>
    </div>
  )
}

const VESSEL_PALETTE = [
  'bg-cyan/75 text-bg',
  'bg-magenta/70 text-bg',
  'bg-green/70 text-bg',
  'bg-amber/55 text-bg',
  'bg-cyan-hot/70 text-bg',
]

function nameToPalette(name: string, fallbackIdx: number): number {
  // Cheap stable hash: sum char codes mod palette length.
  if (!name) return fallbackIdx % VESSEL_PALETTE.length
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h + name.charCodeAt(i) * (i + 1)) | 0
  return Math.abs(h) % VESSEL_PALETTE.length
}

function segmentClass(seg: Segment): string {
  switch (seg.kind) {
    case 'vessel': {
      const base = VESSEL_PALETTE[seg.paletteIdx ?? 0]
      return cn(base, 'font-bold', seg.highlight && 'ring-1 ring-inset ring-cyan-hot')
    }
    case 'proposed':
      return 'bg-amber/80 text-bg font-bold animate-pulse-soft'
    case 'free':
      return 'bg-transparent text-dimmer'
    case 'reserved':
      return 'bg-dimmer/40 hatch text-dim'
    case 'overshoot':
      return 'bg-red/80 text-bg font-bold'
  }
}
