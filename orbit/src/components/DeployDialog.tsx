import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field } from './ui/Input'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { RepoPicker } from './RepoPicker'
import { BranchPicker } from './BranchPicker'
import { CapacityBar } from './CapacityBar'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError, type PermissionMode } from '../api/types'
import { refreshNow } from '../api/hooks'
import { formatMem } from '../lib/time'
import { cn } from '../lib/cn'
import { TIERS, type TierId, tierForSize } from '../lib/tiers'

const NAME_RE = /^[a-z0-9-]{1,12}$/

export function DeployDialog() {
  const open = useUi((s) => s.deployOpen)
  const close = useUi((s) => s.closeDeploy)
  const toast = useUi((s) => s.toast)
  const capacity = useUi((s) => s.capacity)
  const credentials = useUi((s) => s.credentials)
  const credExpired = credentials?.expired === true
  const accounts = useUi((s) => s.githubAccounts) ?? []
  const presentAccounts = accounts.filter((a) => a.present)

  const [name, setName] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [branch, setBranch] = useState('main')
  const [tier, setTier] = useState<TierId>('medium')
  const [vcpu, setVcpu] = useState(2)
  const [memGib, setMemGib] = useState(8)
  const [permissionMode, setPermissionMode] = useState<PermissionMode>('auto')
  const [githubAccount, setGithubAccount] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    if (githubAccount && presentAccounts.some((a) => a.alias === githubAccount)) return
    if (presentAccounts.length === 1) {
      setGithubAccount(presentAccounts[0].alias)
    } else if (presentAccounts.length === 0) {
      setGithubAccount(null)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, presentAccounts.length])

  // Keep tier in sync if vcpu/memGib were nudged by something else
  // (e.g. selecting a tier — round-trip should match).
  useEffect(() => {
    if (tier === 'custom') return
    const inferred = tierForSize(vcpu, memGib * 1024)
    if (inferred !== tier) setTier(inferred)
  }, [vcpu, memGib, tier])

  function pickTier(id: TierId) {
    setTier(id)
    if (id !== 'custom') {
      const t = TIERS.find((tt) => tt.id === id)!
      setVcpu(t.vcpu)
      setMemGib(t.memGib)
    }
  }

  function reset() {
    setName('')
    setRepoUrl('')
    setBranch('main')
    setTier('medium')
    setVcpu(2)
    setMemGib(8)
    setPermissionMode('auto')
    setGithubAccount(presentAccounts.length === 1 ? presentAccounts[0].alias : null)
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

  const memMb = memGib * 1024
  const proposed = { name: name || 'new', vcpu, mem_mb: memMb }
  const memHeadroom = capacity
    ? capacity.host.assignable_mem_mb - capacity.allocated.mem_mb - memMb
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
        github_account: githubAccount,
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
      maxWidth="max-w-2xl"
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
        <div>
          <div className="mb-1.5 flex items-baseline justify-between gap-2">
            <span className="text-[10px] uppercase tracking-widest text-dim">GitHub Account</span>
            <span className="text-[10px] uppercase tracking-wider text-dimmer">
              {presentAccounts.length === 0
                ? 'none staged — paste a URL or stage a PAT'
                : `used for repo list & git push`}
            </span>
          </div>
          <div className="flex border border-border bg-bg/60">
            <span className="px-2 text-cyan opacity-60 select-none self-center">›</span>
            <select
              value={githubAccount ?? ''}
              onChange={(e) => setGithubAccount(e.target.value || null)}
              className="block w-full bg-transparent py-2 pr-3 font-mono text-sm text-text outline-none"
            >
              <option value="">— none —</option>
              {presentAccounts.map((a) => (
                <option key={a.alias} value={a.alias}>
                  {a.alias}
                </option>
              ))}
            </select>
          </div>
        </div>
        <RepoPicker
          value={repoUrl}
          account={githubAccount}
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
        <BranchPicker
          value={branch}
          onChange={setBranch}
          repoUrl={repoUrl}
          account={githubAccount}
        />

        <SectionHeader label="Size" />
        <div className="grid grid-cols-3 gap-2 sm:grid-cols-6">
          {TIERS.map((t) => (
            <TierTile
              key={t.id}
              active={tier === t.id}
              onClick={() => pickTier(t.id)}
              label={t.label}
              vcpu={t.vcpu}
              memGib={t.memGib}
              blurb={t.blurb}
            />
          ))}
          <TierTile
            active={tier === 'custom'}
            onClick={() => pickTier('custom')}
            label="custom"
            blurb="dial it in"
            custom
          />
        </div>
        {tier === 'custom' && (
          <div className="grid grid-cols-2 gap-3 border border-border/60 bg-surface-2/50 p-3">
            <Field
              label="vCPU"
              name="vcpu"
              type="number"
              min={1}
              max={32}
              value={vcpu}
              hint={capacity ? `host has ${capacity.host.cpus}` : '1–32'}
              onChange={(e) =>
                setVcpu(Math.max(1, Math.min(32, parseInt(e.target.value || '4', 10))))
              }
            />
            <Field
              label="Memory (GiB)"
              name="mem"
              type="number"
              min={1}
              max={128}
              value={memGib}
              hint={capacity ? `host has ${formatMem(capacity.host.mem_mb)}` : '1–128'}
              onChange={(e) =>
                setMemGib(Math.max(1, Math.min(128, parseInt(e.target.value || '4', 10))))
              }
            />
          </div>
        )}

        {/* gparted-style live preview */}
        {capacity && (
          <div className="border border-border/60 bg-surface-2/40 p-3 space-y-3">
            <div className="text-[10px] uppercase tracking-widest text-dim">
              After this deploy
            </div>
            <CapacityBar cap={capacity} resource="mem" proposed={proposed} compact />
            <CapacityBar cap={capacity} resource="vcpu" proposed={proposed} compact />
          </div>
        )}

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

        {memWouldOverfill && (
          <div className="border border-amber/40 bg-amber/[0.05] px-3 py-2 text-xs text-amber">
            <span className="text-[10px] uppercase tracking-widest">! capacity</span>
            <span className="ml-2 font-mono">
              would exceed assignable memory by {formatMem(-memHeadroom!)} — destroy a vessel,
              shrink the tier, or bump reservedMemMb in the module
            </span>
          </div>
        )}

        {credExpired && (
          <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red">
            <span className="text-[10px] uppercase tracking-widest text-red/80">! credentials expired</span>
            <span className="ml-2 font-mono">
              new vessels can't register with claude.ai/code right now. Re-stage from your
              laptop (`scripts/restage-claude-credentials.sh` after `claude auth login`),
              then retry.
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
              disabled={!formValid || memWouldOverfill || credExpired}
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

function TierTile({
  active,
  onClick,
  label,
  vcpu,
  memGib,
  blurb,
  custom,
}: {
  active: boolean
  onClick: () => void
  label: string
  vcpu?: number
  memGib?: number
  blurb: string
  custom?: boolean
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'border px-2.5 py-2 text-left transition-colors',
        active
          ? 'border-cyan bg-cyan/[0.05]'
          : 'border-border bg-surface-2/40 hover:border-border-bright',
      )}
    >
      <div
        className={cn(
          'text-[11px] uppercase tracking-widest',
          active ? 'text-cyan' : 'text-text',
        )}
      >
        {label}
      </div>
      {!custom && (
        <div className="mt-0.5 font-mono text-[11px] tabular-nums text-text">
          {vcpu} <span className="text-dimmer">·</span> {memGib} GiB
        </div>
      )}
      <div className="mt-0.5 text-[10px] text-dim leading-tight truncate">{blurb}</div>
    </button>
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
