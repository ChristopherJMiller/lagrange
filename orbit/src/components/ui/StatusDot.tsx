import { cn } from '../../lib/cn'

type Variant = 'green' | 'amber' | 'red' | 'cyan' | 'dim'

const colorMap: Record<Variant, { bg: string; shadow: string }> = {
  green: { bg: 'bg-green', shadow: 'shadow-glow-green' },
  amber: { bg: 'bg-amber', shadow: 'shadow-glow-amber' },
  red: { bg: 'bg-red', shadow: 'shadow-glow-red' },
  cyan: { bg: 'bg-cyan', shadow: 'shadow-glow-cyan' },
  dim: { bg: 'bg-dimmer', shadow: '' },
}

export function StatusDot({
  variant,
  pulse = false,
  size = 8,
}: {
  variant: Variant
  pulse?: boolean
  size?: number
}) {
  const { bg, shadow } = colorMap[variant]
  return (
    <span
      style={{ width: size, height: size }}
      className={cn(
        'inline-block rounded-full',
        bg,
        shadow,
        pulse && 'animate-pulse-soft',
      )}
    />
  )
}
