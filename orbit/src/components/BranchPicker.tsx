import { useEffect, useMemo, useState } from 'react'
import { api } from '../api/client'
import type { GithubBranchDto } from '../api/types'
import { isApiError } from '../api/types'
import { cn } from '../lib/cn'
import { parseGithubOwnerRepo } from '../lib/repo'

type Props = {
  value: string
  onChange: (branch: string) => void
  repoUrl: string
  account: string | null
}

/**
 * Branch input with a dropdown of remote branches when the repo URL
 * is github-shaped AND a PAT is staged. Otherwise it's a plain text
 * field — operator can type any value, supporting non-github remotes
 * or branches that haven't been pushed yet.
 */
export function BranchPicker({ value, onChange, repoUrl, account }: Props) {
  const ownerRepo = useMemo(() => parseGithubOwnerRepo(repoUrl), [repoUrl])
  const canList = !!ownerRepo

  const [branches, setBranches] = useState<GithubBranchDto[] | null>(null)
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'ok' | 'err'>('idle')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)

  // Invalidate when the repo (or account) changes; the next browse
  // re-fetches.
  useEffect(() => {
    setBranches(null)
    setLoadState('idle')
    setLoadError(null)
  }, [ownerRepo?.owner, ownerRepo?.repo, account])

  async function load() {
    if (!ownerRepo) return
    if (loadState === 'loading' || loadState === 'ok') return
    setLoadState('loading')
    setLoadError(null)
    try {
      const b = await api.listGithubBranches(
        ownerRepo.owner,
        ownerRepo.repo,
        account ?? undefined,
      )
      setBranches(b)
      setLoadState('ok')
    } catch (e) {
      setLoadError(isApiError(e) ? e.message : 'failed to list')
      setLoadState('err')
    }
  }

  const filter = value.toLowerCase()
  const filtered = useMemo(() => {
    if (!branches) return []
    if (!filter) return branches.slice(0, 50)
    return branches.filter((b) => b.name.toLowerCase().includes(filter)).slice(0, 50)
  }, [branches, filter])

  function pick(name: string) {
    onChange(name)
    setOpen(false)
  }

  return (
    <div className="relative">
      <div className="mb-1.5 flex items-baseline justify-between gap-2">
        <span className="text-[10px] uppercase tracking-widest text-dim">Branch</span>
        {canList ? (
          <button
            type="button"
            onClick={() => {
              setOpen((o) => !o)
              if (!open) load()
            }}
            className="text-[10px] uppercase tracking-widest text-cyan hover:text-cyan-hot"
          >
            {open ? 'close ↑' : 'browse ↓'}
          </button>
        ) : (
          <span className="text-[10px] uppercase tracking-wider text-dimmer">
            paste branch · not a github url
          </span>
        )}
      </div>
      <div
        className={cn(
          'group relative flex items-center border bg-bg/60',
          'transition-colors duration-150',
          'border-border focus-within:border-cyan',
        )}
      >
        <span className="px-2 text-cyan opacity-60 select-none">›</span>
        <input
          name="branch"
          autoComplete="off"
          spellCheck={false}
          className="block w-full bg-transparent py-2 pr-3 font-mono text-sm text-text outline-none placeholder:text-dimmer"
          value={value}
          placeholder="main"
          onFocus={() => {
            if (canList && branches === null) load()
          }}
          onChange={(e) => onChange(e.target.value.trim())}
        />
      </div>

      {open && canList && (
        <div className="mt-1 max-h-60 overflow-y-auto border border-border bg-surface-2 shadow-lg">
          {loadState === 'loading' && (
            <div className="px-3 py-3 text-[11px] text-dim">loading branches…</div>
          )}
          {loadState === 'err' && (
            <div className="px-3 py-3 text-[11px] text-red">
              ! {loadError}
              <div className="mt-1 text-[10px] text-dim">type the branch manually above</div>
            </div>
          )}
          {loadState === 'ok' && filtered.length === 0 && (
            <div className="px-3 py-3 text-[11px] text-dim">
              no matches · type to use anyway
            </div>
          )}
          {loadState === 'ok' &&
            filtered.map((b) => (
              <button
                key={b.name}
                type="button"
                onClick={() => pick(b.name)}
                className="block w-full border-b border-border/40 px-3 py-1.5 text-left transition-colors hover:bg-surface-3"
              >
                <div className="flex items-baseline justify-between gap-2">
                  <span className="font-mono text-xs text-text truncate">{b.name}</span>
                  {b.protected && (
                    <span className="text-[10px] uppercase tracking-widest text-amber shrink-0">
                      protected
                    </span>
                  )}
                </div>
              </button>
            ))}
        </div>
      )}
    </div>
  )
}
