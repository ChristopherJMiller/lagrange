import * as RDialog from '@radix-ui/react-dialog'
import { type ReactNode } from 'react'
import { cn } from '../../lib/cn'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  subtitle?: string
  children: ReactNode
  maxWidth?: string
}

export function Dialog({ open, onOpenChange, title, subtitle, children, maxWidth = 'max-w-xl' }: Props) {
  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <RDialog.Portal>
        <RDialog.Overlay
          className={cn(
            'fixed inset-0 z-50 bg-bg/85 backdrop-blur-sm',
            'data-[state=open]:animate-fade-in',
          )}
        />
        <RDialog.Content
          className={cn(
            'fixed left-1/2 top-1/2 z-50 w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2',
            'border border-border-bright bg-surface text-text shadow-2xl',
            'data-[state=open]:animate-slide-up',
            maxWidth,
          )}
          onOpenAutoFocus={(e) => {
            // let the first input focus itself
            e.preventDefault()
          }}
        >
          {/* Title bar */}
          <div className="flex items-center justify-between border-b border-border bg-surface-2 px-4 py-2.5">
            <div className="flex items-baseline gap-3">
              <span className="text-cyan text-[10px] tracking-widest">[ DIALOG ]</span>
              <RDialog.Title className="text-[11px] uppercase tracking-widest text-text">
                {title}
              </RDialog.Title>
              {subtitle && (
                <RDialog.Description className="text-[10px] tracking-wider text-dim">
                  {subtitle}
                </RDialog.Description>
              )}
            </div>
            <RDialog.Close
              aria-label="Close"
              className="flex h-6 w-6 items-center justify-center text-dim hover:text-text hover:bg-surface-3 transition-colors"
            >
              ✕
            </RDialog.Close>
          </div>

          <div className="p-5">{children}</div>
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  )
}
