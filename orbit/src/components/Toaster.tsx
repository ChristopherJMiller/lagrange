import * as Toast from '@radix-ui/react-toast'
import { useEffect } from 'react'
import { useUi } from '../store/ui'
import { cn } from '../lib/cn'

const KIND_STYLE = {
  ok: 'border-green/40 text-green',
  err: 'border-red/40 text-red',
  info: 'border-cyan/40 text-cyan',
} as const

const KIND_TAG = {
  ok: '◉ OK',
  err: '! FAULT',
  info: '› INFO',
} as const

export function Toaster() {
  const toasts = useUi((s) => s.toasts)
  const dismiss = useUi((s) => s.dismissToast)

  // Auto-dismiss after 4.5s
  useEffect(() => {
    const timers = toasts.map((t) =>
      window.setTimeout(() => dismiss(t.id), 4500),
    )
    return () => timers.forEach(window.clearTimeout)
  }, [toasts, dismiss])

  return (
    <Toast.Provider swipeDirection="right" duration={4500}>
      {toasts.map((t) => (
        <Toast.Root
          key={t.id}
          onOpenChange={(open) => !open && dismiss(t.id)}
          className={cn(
            'group pointer-events-auto relative flex items-start gap-3 border bg-surface px-4 py-3 shadow-lg',
            'data-[state=open]:animate-slide-up data-[state=closed]:animate-fade-in',
            KIND_STYLE[t.kind],
          )}
        >
          <Toast.Title className={cn('text-[10px] uppercase tracking-widest shrink-0 mt-px', KIND_STYLE[t.kind])}>
            {KIND_TAG[t.kind]}
          </Toast.Title>
          <Toast.Description className="text-[11px] text-text leading-relaxed flex-1 break-words">
            {t.msg}
          </Toast.Description>
          <Toast.Close
            aria-label="dismiss"
            className="text-dim hover:text-text text-xs leading-none"
          >
            ✕
          </Toast.Close>
        </Toast.Root>
      ))}
      <Toast.Viewport className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[min(28rem,calc(100vw-2rem))] flex-col gap-2 outline-none" />
    </Toast.Provider>
  )
}
