import { type ReactNode } from 'react'
import { cn } from '../../lib/cn'

/**
 * Section header in the "instrument panel" style:
 *
 *   ┌─ LABEL ─────────────────────────┐
 *
 * Single line, uppercase tracked label, dotted rule filling remainder.
 */
export function SectionHeader({
  label,
  meta,
  className,
}: {
  label: string
  meta?: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex items-center gap-3', className)}>
      <span className="text-cyan opacity-70 text-xs select-none">[</span>
      <span className="text-[10px] uppercase tracking-widest text-dim">{label}</span>
      <div className="divider-dotted h-px flex-1" />
      {meta && <span className="text-[10px] uppercase tracking-wider text-dimmer">{meta}</span>}
      <span className="text-cyan opacity-70 text-xs select-none">]</span>
    </div>
  )
}

/**
 * A small "key value" row used in vessel cards and dialogs.
 *
 *   LABEL   value
 */
export function DataRow({
  label,
  value,
  accent,
}: {
  label: string
  value: ReactNode
  accent?: boolean
}) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] items-baseline gap-3 text-[12px]">
      <span className="text-[10px] uppercase tracking-widest text-dimmer">{label}</span>
      <span className={cn('font-mono truncate', accent ? 'text-amber' : 'text-text')}>{value}</span>
    </div>
  )
}
