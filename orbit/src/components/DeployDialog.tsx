import { useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field } from './ui/Input'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'
import { refreshNow } from '../api/hooks'
import { cn } from '../lib/cn'

const NAME_RE = /^[a-z0-9-]{1,12}$/

export function DeployDialog() {
  const open = useUi((s) => s.deployOpen)
  const close = useUi((s) => s.closeDeploy)
  const toast = useUi((s) => s.toast)

  const [name, setName] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [branch, setBranch] = useState('main')
  const [showAdv, setShowAdv] = useState(false)
  const [vcpu, setVcpu] = useState(4)
  const [memGib, setMemGib] = useState(4)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function reset() {
    setName('')
    setRepoUrl('')
    setBranch('main')
    setVcpu(4)
    setMemGib(4)
    setShowAdv(false)
    setError(null)
    setSubmitting(false)
  }

  function handleOpenChange(o: boolean) {
    if (!o) {
      reset()
      close()
    }
  }

  const nameErr = name.length === 0 ? null : NAME_RE.test(name) ? null : 'a-z 0-9 - · max 12'
  const repoErr = repoUrl.length === 0 ? null : repoUrl.length < 4 ? 'too short' : null
  const formValid = NAME_RE.test(name) && repoUrl.length >= 4 && !!branch

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (!formValid || submitting) return
    setSubmitting(true)
    setError(null)
    try {
      const resp = await api.createVm({
        name,
        repo_url: repoUrl,
        branch,
        vcpu,
        mem_mb: memGib * 1024,
      })
      toast('ok', `vessel "${resp.name}" deployed at ${resp.vm_ip}`)
      await refreshNow()
      reset()
      close()
    } catch (err) {
      const msg = isApiError(err) ? err.message : 'deploy failed'
      setError(msg)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={handleOpenChange}
      title="Deploy Vessel"
      subtitle="new repo-vm provisioning"
      maxWidth="max-w-lg"
    >
      <form onSubmit={submit} className="space-y-4">
        <SectionHeader label="Identity" />
        <Field
          label="Callsign"
          name="name"
          placeholder="lgtest"
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          value={name}
          error={nameErr ?? undefined}
          hint="1–12 chars · a-z 0-9 -"
          onChange={(e) => setName(e.target.value.toLowerCase())}
          autoFocus
        />
        <Field
          label="Repo URL"
          name="repo_url"
          placeholder="git@github.com:org/repo.git"
          autoComplete="off"
          value={repoUrl}
          error={repoErr ?? undefined}
          hint="ssh or https"
          onChange={(e) => setRepoUrl(e.target.value.trim())}
        />
        <Field
          label="Branch"
          name="branch"
          value={branch}
          onChange={(e) => setBranch(e.target.value.trim())}
        />

        <div>
          <button
            type="button"
            onClick={() => setShowAdv((v) => !v)}
            className="text-[10px] uppercase tracking-widest text-dim hover:text-text"
          >
            {showAdv ? '▾' : '▸'} Advanced (vCPU / RAM)
          </button>
        </div>
        {showAdv && (
          <div className="grid grid-cols-2 gap-3 border border-border/60 bg-surface-2/50 p-3">
            <Field
              label="vCPU"
              name="vcpu"
              type="number"
              min={1}
              max={32}
              value={vcpu}
              hint="1–32"
              onChange={(e) => setVcpu(Math.max(1, Math.min(32, parseInt(e.target.value || '4', 10))))}
            />
            <Field
              label="Memory (GiB)"
              name="mem"
              type="number"
              min={1}
              max={128}
              value={memGib}
              hint="1–128"
              onChange={(e) => setMemGib(Math.max(1, Math.min(128, parseInt(e.target.value || '4', 10))))}
            />
          </div>
        )}

        {error && (
          <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red">
            <span className="text-[10px] uppercase tracking-widest text-red/80">! FAULT</span>
            <span className="ml-2 font-mono">{error}</span>
          </div>
        )}

        <div className={cn('flex items-center justify-between border-t border-border/60 pt-4')}>
          <div className="text-[10px] uppercase tracking-widest text-dimmer">
            create takes ~30–90s (git ls-remote + microvm start)
          </div>
          <div className="flex items-center gap-2">
            <Button type="button" variant="secondary" size="md" onClick={() => handleOpenChange(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="hero"
              size="md"
              disabled={!formValid}
              loading={submitting}
            >
              {submitting ? 'Launching…' : 'Launch ▸'}
            </Button>
          </div>
        </div>
      </form>
    </Dialog>
  )
}
