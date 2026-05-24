import { useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field } from './ui/Input'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { RepoPicker } from './RepoPicker'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError, type PermissionMode } from '../api/types'
import { refreshNow } from '../api/hooks'
import { formatMem } from '../lib/time'
import { cn } from '../lib/cn'

const NAME_RE = /^[a-z0-9-]{1,12}$/

export function DeployDialog() {
  const open = useUi((s) => s.deployOpen)
  const close = useUi((s) => s.closeDeploy)
  const toast = useUi((s) => s.toast)
  const capacity = useUi((s) => s.capacity)

  const [name, setName] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [branch, setBranch] = useState('main')
  const [showAdv, setShowAdv] = useState(false)
  const [vcpu, setVcpu] = useState(4)
  const [memGib, setMemGib] = useState(4)
  const [permissionMode, setPermissionMode] = useState<PermissionMode>('auto')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function reset() {
    setName('')
    setRepoUrl('')
    setBranch('main')
    setVcpu(4)
    setMemGib(4)
    setPermissionMode('auto')
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

  // Capacity headroom warning (memory only — vCPU is allowed to overcommit).
  const memMb = memGib * 1024
  const memHeadroom = capacity
    ? capacity.host.mem_mb - capacity.allocated.mem_mb - memMb
    : null
  const memWouldOverfill = memHeadroom !== null && memHeadroom < 0

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
        mem_mb: memMb,
        permission_mode: permissionMode,
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
        <RepoPicker
          value={repoUrl}
          onChange={(url, defaultBranch) => {
            setRepoUrl(url)
            if (defaultBranch) setBranch(defaultBranch)
          }}
        />
        {repoErr && (
          <div className="text-[10px] uppercase tracking-wider text-red -mt-3">
            {repoErr}
          </div>
        )}
        <Field
          label="Branch"
          name="branch"
          value={branch}
          onChange={(e) => setBranch(e.target.value.trim())}
        />

        <SectionHeader label="Permissions" />
        <div className="grid grid-cols-2 gap-2">
          <PermissionTile
            active={permissionMode === 'auto'}
            onClick={() => setPermissionMode('auto')}
            label="Auto"
            sub="classifier-mediated approval (recommended)"
            accent="cyan"
          />
          <PermissionTile
            active={permissionMode === 'dangerously-skip'}
            onClick={() => setPermissionMode('dangerously-skip')}
            label="Dangerously Skip"
            sub="no approval gate · full autonomy"
            accent="red"
          />
        </div>

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
              hint={capacity ? `host has ${capacity.host.cpus}` : '1–32'}
              onChange={(e) => setVcpu(Math.max(1, Math.min(32, parseInt(e.target.value || '4', 10))))}
            />
            <Field
              label="Memory (GiB)"
              name="mem"
              type="number"
              min={1}
              max={128}
              value={memGib}
              hint={capacity ? `host has ${formatMem(capacity.host.mem_mb)}` : '1–128'}
              onChange={(e) => setMemGib(Math.max(1, Math.min(128, parseInt(e.target.value || '4', 10))))}
            />
          </div>
        )}

        {memWouldOverfill && (
          <div className="border border-amber/40 bg-amber/[0.05] px-3 py-2 text-xs text-amber">
            <span className="text-[10px] uppercase tracking-widest">! capacity</span>
            <span className="ml-2 font-mono">
              would exceed host memory by {formatMem(-memHeadroom!)} — destroy a vessel or pick smaller
            </span>
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
            create takes ~30–90s
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

function PermissionTile({
  active,
  onClick,
  label,
  sub,
  accent,
}: {
  active: boolean
  onClick: () => void
  label: string
  sub: string
  accent: 'cyan' | 'red'
}) {
  const accentClass = accent === 'red' ? 'border-red bg-red/[0.05]' : 'border-cyan bg-cyan/[0.04]'
  const labelClass = accent === 'red' ? 'text-red' : 'text-cyan'
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'border px-3 py-2.5 text-left transition-colors',
        active ? accentClass : 'border-border bg-surface-2/40 hover:border-border-bright',
      )}
    >
      <div className={cn('text-[11px] uppercase tracking-widest', active ? labelClass : 'text-dim')}>
        {label}
      </div>
      <div className="mt-1 text-[10px] text-dim leading-tight">{sub}</div>
    </button>
  )
}
