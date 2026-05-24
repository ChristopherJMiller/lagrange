import { forwardRef, type ButtonHTMLAttributes } from 'react'
import { cn } from '../../lib/cn'

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'hero'
type Size = 'sm' | 'md' | 'lg'

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant
  size?: Size
  loading?: boolean
}

const variantClasses: Record<Variant, string> = {
  primary:
    'border border-border-bright bg-surface-2 text-text hover:bg-surface-3 hover:border-border-hot active:translate-y-px',
  secondary:
    'border border-border bg-transparent text-dim hover:text-text hover:border-border-bright',
  ghost:
    'border border-transparent bg-transparent text-dim hover:text-text hover:bg-surface-2',
  danger:
    'border border-red/40 bg-transparent text-red hover:bg-red/10 hover:border-red',
  hero:
    'group relative border border-amber/40 bg-amber/[0.08] text-amber hover:bg-amber/[0.12] hover:border-amber hover:shadow-glow-amber',
}

const sizeClasses: Record<Size, string> = {
  sm: 'h-7 px-2.5 text-[10px] tracking-widest uppercase',
  md: 'h-9 px-3.5 text-[11px] tracking-widest uppercase',
  lg: 'h-11 px-5 text-xs tracking-widest uppercase',
}

export const Button = forwardRef<HTMLButtonElement, Props>(function Button(
  { variant = 'primary', size = 'md', loading, className, children, disabled, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      disabled={disabled || loading}
      className={cn(
        'inline-flex select-none items-center justify-center gap-2 font-mono font-medium',
        'transition-all duration-150 ease-out',
        'disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent',
        'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-cyan focus-visible:ring-offset-2 focus-visible:ring-offset-bg',
        variantClasses[variant],
        sizeClasses[size],
        className,
      )}
      {...rest}
    >
      {loading ? (
        <span className="inline-flex items-center gap-2">
          <span className="inline-block h-1.5 w-1.5 animate-pulse-soft rounded-full bg-current" />
          <span>{children}</span>
        </span>
      ) : (
        children
      )}
    </button>
  )
})
