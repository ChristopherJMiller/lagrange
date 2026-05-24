import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'
import { relativeTime } from '../lib/time'

/**
 * Edit the shared CLAUDE.md mounted into every guest at
 * /shared/CLAUDE.md (symlinked to ~/.claude/CLAUDE.md). Changes
 * propagate live via virtiofs, but `claude` reads it once at session
 * start — the operator must restart a vessel for the agent to pick
 * the new prompt up.
 */
export function ClaudeMdEditor() {
  const open = useUi((s) => s.claudeMdOpen)
  const close = useUi((s) => s.closeClaudeMd)
  const toast = useUi((s) => s.toast)

  const [content, setContent] = useState('')
  const [setAt, setSetAt] = useState<string | null>(null)
  const [origContent, setOrigContent] = useState('')
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    let cancelled = false
    setLoading(true)
    setError(null)
    api
      .getAgentClaudeMd()
      .then((s) => {
        if (cancelled) return
        setContent(s.content)
        setOrigContent(s.content)
        setSetAt(s.set_at)
      })
      .catch((e) => {
        if (cancelled) return
        setError(isApiError(e) ? e.message : 'load failed')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open])

  const dirty = content !== origContent

  async function save() {
    setBusy(true)
    setError(null)
    try {
      await api.putAgentClaudeMd(content)
      setOrigContent(content)
      toast('ok', 'shared CLAUDE.md saved · restart vessels to apply')
    } catch (e) {
      setError(isApiError(e) ? e.message : 'save failed')
    } finally {
      setBusy(false)
    }
  }

  function discard() {
    setContent(origContent)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      title="Shared CLAUDE.md"
      subtitle="user-level prompt for every vessel"
      maxWidth="max-w-4xl"
    >
      <div className="space-y-3">
        <SectionHeader
          label="Editor"
          meta={
            loading
              ? 'reading…'
              : setAt
                ? `${content.length} bytes · saved ${relativeTime(setAt)}`
                : `${content.length} bytes`
          }
        />

        <div className="text-[11px] text-dim leading-relaxed">
          Lives at <span className="font-mono text-cyan">/shared/CLAUDE.md</span> in every
          guest, symlinked to <span className="font-mono text-cyan">~/.claude/CLAUDE.md</span>.
          Seeded from <span className="font-mono">shared-agent-state/CLAUDE.md</span> on first
          boot. Edits here win going forward — running vessels need a restart for the agent
          to re-read.
        </div>

        <textarea
          value={content}
          spellCheck={false}
          onChange={(e) => setContent(e.target.value)}
          className="block w-full resize-y border border-border bg-bg/80 px-3 py-2.5 font-mono text-[12px] leading-relaxed text-text outline-none focus:border-cyan min-h-[400px] max-h-[60vh]"
          placeholder={loading ? 'loading…' : '# Lagrange compute satellite environment\n\n…'}
        />

        {error && (
          <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">
            ! {error}
          </div>
        )}

        <div className="flex items-center justify-between border-t border-border/60 pt-4">
          <div className="text-[10px] uppercase tracking-widest text-dimmer">
            {dirty ? 'unsaved changes' : 'in sync'}
          </div>
          <div className="flex gap-2">
            {dirty && (
              <Button variant="secondary" size="md" onClick={discard} disabled={busy}>
                Discard
              </Button>
            )}
            <Button variant="hero" size="md" onClick={save} disabled={!dirty || busy} loading={busy}>
              Save
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  )
}
