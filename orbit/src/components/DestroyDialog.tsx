import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field } from './ui/Input'
import { Button } from './ui/Button'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'
import { refreshNow } from '../api/hooks'
import { cn } from '../lib/cn'

export function DestroyDialog() {
  const target = useUi((s) => s.destroyTarget)
  const close = useUi((s) => s.closeDestroy)
  const toast = useUi((s) => s.toast)

  const [typed, setTyped] = useState('')
  const [wipe, setWipe] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (target) {
      setTyped('')
      setWipe(false)
      setError(null)
      setSubmitting(false)
    }
  }, [target?.name])

  if (!target) return <Dialog open={false} onOpenChange={() => {}} title="" children={null} />

  const matches = typed === target.name

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (!matches || submitting) return
    setSubmitting(true)
    setError(null)
    try {
      await api.deleteVm(target!.name, wipe)
      toast('ok', `vessel "${target!.name}" destroyed${wipe ? ' (persistent wiped)' : ''}`)
      await refreshNow()
      close()
    } catch (err) {
      setError(isApiError(err) ? err.message : 'destroy failed')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Dialog
      open={!!target}
      onOpenChange={(o) => !o && close()}
      title="Destroy Vessel"
      subtitle="irreversible"
      maxWidth="max-w-lg"
    >
      <form onSubmit={submit} className="space-y-4">
        <div className="border border-red/40 bg-red/5 px-4 py-3">
          <div className="flex items-baseline gap-3">
            <span className="text-red text-xs tracking-widest">! ABORT WARNING</span>
            <span className="text-[10px] uppercase tracking-widest text-red/80">terminal</span>
          </div>
          <div className="mt-2 text-xs text-text/90 leading-relaxed">
            This will stop and remove vessel{' '}
            <span className="text-amber font-bold">{target.name}</span>, release its IP{' '}
            <span className="text-amber">{target.vm_ip}</span>, and delete the DB row.
          </div>
        </div>

        <Field
          label={`Type "${target.name}" to confirm`}
          name="confirm"
          placeholder={target.name}
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          autoFocus
        />

        <label
          className={cn(
            'flex items-start gap-3 border bg-surface-2/50 px-3 py-2.5 cursor-pointer',
            wipe ? 'border-red/40' : 'border-border hover:border-border-bright',
          )}
        >
          <input
            type="checkbox"
            checked={wipe}
            onChange={(e) => setWipe(e.target.checked)}
            className="mt-1 h-3 w-3 accent-red"
          />
          <div>
            <div className="text-[11px] uppercase tracking-wider text-text">
              Wipe persistent volume
            </div>
            <div className="text-[10px] text-dim mt-0.5">
              also remove /persistent/&lt;name&gt; — clones, credentials staged inside, todos, sessions. Cannot be recovered.
            </div>
          </div>
        </label>

        {error && (
          <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">
            {error}
          </div>
        )}

        <div className="flex items-center justify-between border-t border-border/60 pt-4">
          <Button type="button" variant="secondary" size="md" onClick={close}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant="danger"
            size="md"
            disabled={!matches}
            loading={submitting}
          >
            {submitting ? 'Destroying…' : wipe ? 'Destroy + Wipe' : 'Destroy'}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
