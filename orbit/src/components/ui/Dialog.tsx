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
        {/*
          Centering strategy: a viewport-sized scrollable wrapper does
          the centering with flex, not the dialog Content itself. This
          keeps tall dialogs (eg. credential wizard with two 80KB
          textareas) visually centered when they fit, and scrollable
          when they don't — vs. fixed top-1/2 -translate-y-1/2 which
          pushes the top of an oversized dialog off-screen with no way
          to scroll up to it.
        */}
        <div
          className="fixed inset-0 z-50 overflow-y-auto"
          // Radix Dialog needs a Content child to manage focus; the
          // wrapper itself isn't focusable.
        >
          <div className="flex min-h-full items-center justify-center p-4">
            <RDialog.Content
              className={cn(
                'relative w-full',
                'border border-border-bright bg-surface text-text shadow-2xl',
                'data-[state=open]:animate-slide-up',
                'max-h-[calc(100vh-2rem)] flex flex-col',
                maxWidth,
              )}
              onOpenAutoFocus={(e) => {
                // let the first input focus itself
                e.preventDefault()
              }}
            >
              {/* Title bar (sticky inside the dialog) */}
              <div className="flex items-center justify-between border-b border-border bg-surface-2 px-4 py-2.5 shrink-0">
                <div className="flex items-baseline gap-3 min-w-0">
                  <span className="text-cyan text-[10px] tracking-widest shrink-0">[ DIALOG ]</span>
                  <RDialog.Title className="text-[11px] uppercase tracking-widest text-text truncate">
                    {title}
                  </RDialog.Title>
                  {subtitle && (
                    <RDialog.Description className="text-[10px] tracking-wider text-dim truncate">
                      {subtitle}
                    </RDialog.Description>
                  )}
                </div>
                <RDialog.Close
                  aria-label="Close"
                  className="flex h-6 w-6 items-center justify-center text-dim hover:text-text hover:bg-surface-3 transition-colors shrink-0"
                >
                  ✕
                </RDialog.Close>
              </div>

              {/* Scrollable body */}
              <div className="overflow-y-auto p-5">{children}</div>
            </RDialog.Content>
          </div>
        </div>
      </RDialog.Portal>
    </RDialog.Root>
  )
}
