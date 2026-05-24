import { useEffect, useMemo, useState } from 'react'
import { api } from '../api/client'
import type { GithubRepoDto } from '../api/types'
import { isApiError } from '../api/types'
import { cn } from '../lib/cn'

type Props = {
  value: string
  onChange: (url: string, defaultBranch: string | null) => void
  /**
   * Which github account's PAT to query. Required when more than one
   * account is staged. Changing this invalidates the cached list and
   * forces a re-fetch on next open.
   */
  account: string | null
  className?: string
}

/**
 * Combobox: pick from the operator's GitHub repos (server-side proxy
 * via the staged PAT) OR paste an arbitrary URL. The list is loaded
 * lazily on first focus to avoid the api.github.com hit on every page
 * load. Selecting a repo also auto-fills the default branch in the
 * parent form.
 */
export function RepoPicker({ value, onChange, account, className }: Props) {
  const [repos, setRepos] = useState<GithubRepoDto[] | null>(null)
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'ok' | 'err'>('idle')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [filter, setFilter] = useState('')

  useEffect(() => {
    setFilter(value)
  }, [value])

  // Reset cached repos whenever the selected account changes.
  useEffect(() => {
    setRepos(null)
    setLoadState('idle')
    setLoadError(null)
  }, [account])

  async function loadRepos() {
    if (loadState === 'loading' || loadState === 'ok') return
    setLoadState('loading')
    setLoadError(null)
    try {
      const r = await api.listGithubRepos(account ?? undefined)
      setRepos(r)
      setLoadState('ok')
    } catch (e) {
      const msg = isApiError(e) ? e.message : 'failed to list'
      setLoadError(msg)
      setLoadState('err')
    }
  }

  const filtered = useMemo(() => {
    if (!repos) return []
    const q = filter.trim().toLowerCase()
    if (!q) return repos.slice(0, 25)
    return repos
      .filter(
        (r) =>
          r.full_name.toLowerCase().includes(q) ||
          (r.description ?? '').toLowerCase().includes(q),
      )
      .slice(0, 25)
  }, [repos, filter])

  function pick(r: GithubRepoDto) {
    onChange(r.ssh_url, r.default_branch)
    setOpen(false)
    setFilter(r.ssh_url)
  }

  return (
    <div className={cn('relative', className)}>
      <div className="mb-1.5 flex items-baseline justify-between gap-2">
        <span className="text-[10px] uppercase tracking-widest text-dim">Repo</span>
        <button
          type="button"
          onClick={() => {
            setOpen((o) => !o)
            if (!open) loadRepos()
          }}
          className="text-[10px] uppercase tracking-widest text-cyan hover:text-cyan-hot"
        >
          {open ? 'close ↑' : 'browse mine ↓'}
        </button>
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
          name="repo_url"
          placeholder="git@github.com:org/repo.git"
          autoComplete="off"
          spellCheck={false}
          className="block w-full bg-transparent py-2 pr-3 font-mono text-sm text-text outline-none placeholder:text-dimmer"
          value={value}
          onFocus={() => {
            if (!open && repos === null) {
              // Don't auto-open, but prefetch in the background so the
              // browse list is warm when the operator clicks the toggle.
              loadRepos()
            }
          }}
          onChange={(e) => {
            onChange(e.target.value.trim(), null)
            setFilter(e.target.value)
          }}
        />
      </div>

      {open && (
        <div className="mt-1 max-h-72 overflow-y-auto border border-border bg-surface-2 shadow-lg">
          {loadState === 'loading' && (
            <div className="px-3 py-3 text-[11px] text-dim">
              loading from github{account ? ` as ${account}` : ''}…
            </div>
          )}
          {loadState === 'err' && (
            <div className="px-3 py-3 text-[11px] text-red">
              ! {loadError}
              <div className="mt-1 text-[10px] text-dim">
                pick a github account above, then try again
              </div>
            </div>
          )}
          {loadState === 'ok' && filtered.length === 0 && (
            <div className="px-3 py-3 text-[11px] text-dim">
              no matches · or paste a URL above
            </div>
          )}
          {loadState === 'ok' &&
            filtered.map((r) => (
              <button
                key={r.full_name}
                type="button"
                onClick={() => pick(r)}
                className="block w-full border-b border-border/40 px-3 py-2 text-left transition-colors hover:bg-surface-3"
              >
                <div className="flex items-baseline justify-between gap-2">
                  <span className="font-mono text-xs text-text truncate">{r.full_name}</span>
                  <span className="text-[10px] uppercase tracking-widest text-dimmer shrink-0">
                    {r.private ? 'private' : 'public'} · {r.default_branch}
                  </span>
                </div>
                {r.description && (
                  <div className="mt-0.5 text-[10px] text-dim truncate">{r.description}</div>
                )}
              </button>
            ))}
        </div>
      )}
    </div>
  )
}
