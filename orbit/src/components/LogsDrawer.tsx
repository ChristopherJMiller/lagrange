import { useEffect, useRef, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'

export function LogsDrawer() {
  const target = useUi((s) => s.logsTarget)
  const close = useUi((s) => s.closeLogs)

  const [text, setText] = useState<string>('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [lines, setLines] = useState(200)
  const [autoTail, setAutoTail] = useState(true)
  const preRef = useRef<HTMLPreElement>(null)

  useEffect(() => {
    if (!target) {
      setText('')
      setError(null)
      return
    }
    let cancelled = false
    let timer: number | undefined
    async function fetchOnce() {
      try {
        setLoading(true)
        const t = await api.getLogs(target!.name, lines)
        if (cancelled) return
        setText(t)
        setError(null)
        requestAnimationFrame(() => {
          if (autoTail && preRef.current) {
            preRef.current.scrollTop = preRef.current.scrollHeight
          }
        })
      } catch (e) {
        if (cancelled) return
        setError(isApiError(e) ? e.message : 'fetch failed')
      } finally {
        if (!cancelled) {
          setLoading(false)
          timer = window.setTimeout(fetchOnce, 5000)
        }
      }
    }
    fetchOnce()
    return () => {
      cancelled = true
      if (timer) window.clearTimeout(timer)
    }
  }, [target?.name, lines, autoTail])

  if (!target) return null

  return (
    <Dialog
      open={!!target}
      onOpenChange={(o) => !o && close()}
      title={`Logs · ${target.name}`}
      subtitle={`journal · last ${lines} lines · ${target.vm_ip}`}
      maxWidth="max-w-4xl"
    >
      <div className="flex flex-wrap items-center gap-2 mb-3">
        <SectionHeader
          label="Telemetry Stream"
          meta={loading ? 'reading…' : 'live'}
          className="flex-1 min-w-[200px]"
        />
        <select
          value={lines}
          onChange={(e) => setLines(parseInt(e.target.value, 10))}
          className="border border-border bg-surface-2 px-2 py-1 text-[11px] uppercase tracking-wider text-text outline-none focus:border-cyan"
        >
          {[100, 200, 500, 1000].map((n) => (
            <option key={n} value={n}>
              last {n}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1.5 text-[10px] uppercase tracking-widest text-dim cursor-pointer">
          <input type="checkbox" checked={autoTail} onChange={(e) => setAutoTail(e.target.checked)} />
          tail
        </label>
        <Button variant="secondary" size="sm" onClick={close}>
          Close
        </Button>
      </div>

      <pre
        ref={preRef}
        className="h-[65vh] overflow-auto border border-border bg-bg/80 px-3 py-2 font-mono text-[11px] leading-relaxed text-dim whitespace-pre-wrap break-words"
      >
        {error ? (
          <span className="text-red">! {error}</span>
        ) : text ? (
          text
        ) : (
          <span className="text-dimmer">awaiting telemetry…</span>
        )}
      </pre>
    </Dialog>
  )
}
