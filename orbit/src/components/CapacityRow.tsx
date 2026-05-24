import { useUi } from '../store/ui'
import { SectionHeader } from './ui/Bracket'
import { CapacityBar } from './CapacityBar'

/**
 * Two stacked capacity bars (memory + vCPU) above the fleet grid.
 * Same component the deploy dialog uses to preview the proposed
 * vessel — here without a phantom segment.
 */
export function CapacityRow() {
  const cap = useUi((s) => s.capacity)
  const initialLoad = useUi((s) => s.initialLoad)

  if (initialLoad && !cap) {
    return (
      <section className="mt-6">
        <SectionHeader label="Host Capacity" meta="reading…" />
        <div className="mt-3 h-12 border border-border/40 bg-surface/40 animate-pulse-soft" />
      </section>
    )
  }
  if (!cap) return null

  return (
    <section className="mt-6">
      <SectionHeader
        label="Host Capacity"
        meta={`${cap.allocated.vms} vessel${cap.allocated.vms === 1 ? '' : 's'} assigned`}
      />
      <div className="mt-3 space-y-3">
        <CapacityBar cap={cap} resource="mem" />
        <CapacityBar cap={cap} resource="vcpu" />
      </div>
    </section>
  )
}
