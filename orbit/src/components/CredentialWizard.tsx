import { useEffect, useMemo, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field, Textarea } from './ui/Input'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { StatusDot } from './ui/StatusDot'
import { useUi } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'
import { refreshNow } from '../api/hooks'
import { relativeTime } from '../lib/time'
import { cn } from '../lib/cn'

type StepId = 'credentials' | 'github'

const STEPS: { id: StepId; label: string; hint: string }[] = [
  {
    id: 'credentials',
    label: 'Claude Session',
    hint: 'Full-scope login. Required for Remote Control. Paste two files from your local machine.',
  },
  {
    id: 'github',
    label: 'GitHub Accounts',
    hint: 'One or more fine-grained PATs. Pick one per vessel at deploy time.',
  },
]

export function CredentialWizard() {
  const open = useUi((s) => s.wizardOpen)
  const forced = useUi((s) => s.wizardForced)
  const close = useUi((s) => s.closeWizard)
  const credentials = useUi((s) => s.credentials)
  const accounts = useUi((s) => s.githubAccounts)

  const stepStatus: Record<StepId, 'ok' | 'missing' | 'unknown'> = useMemo(() => {
    const c =
      credentials === null
        ? 'unknown'
        : credentials.present
          ? 'ok'
          : 'missing'
    const g =
      accounts === null
        ? 'unknown'
        : accounts.some((a) => a.present)
          ? 'ok'
          : 'missing'
    return { credentials: c, github: g }
  }, [credentials, accounts])

  // Start on the first incomplete step.
  const firstIncomplete = useMemo(
    () => Math.max(0, STEPS.findIndex((s) => stepStatus[s.id] !== 'ok')),
    [stepStatus],
  )
  const [stepIdx, setStepIdx] = useState(firstIncomplete)

  useEffect(() => {
    if (open) setStepIdx(firstIncomplete)
  }, [open, firstIncomplete])

  const step = STEPS[stepIdx]

  function metaFor(s: StepId): string {
    if (s === 'credentials') {
      if (credentials === null) return 'unknown'
      if (!credentials.present) return 'absent'
      return `staged · ${relativeTime(credentials.set_at)}`
    }
    if (accounts === null) return 'unknown'
    const n = accounts.filter((a) => a.present).length
    return n === 0 ? 'absent' : `${n} staged`
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      title="Credential Brief"
      subtitle={forced ? 'first-run setup' : 'manage staged credentials'}
      maxWidth="max-w-2xl"
    >
      {/* Stepper */}
      <div className="mb-5 flex items-center gap-1">
        {STEPS.map((s, i) => {
          const st = stepStatus[s.id]
          const variant = st === 'ok' ? 'green' : st === 'missing' ? 'red' : 'dim'
          const active = i === stepIdx
          return (
            <button
              key={s.id}
              onClick={() => setStepIdx(i)}
              className={cn(
                'flex flex-1 items-center gap-2 border px-3 py-2 text-left transition-colors',
                active
                  ? 'border-cyan bg-cyan/[0.04] text-text'
                  : 'border-border hover:border-border-bright text-dim',
              )}
            >
              <span className="text-cyan/60 font-bold tabular-nums text-xs">0{i + 1}</span>
              <span className="text-[11px] uppercase tracking-wider truncate">{s.label}</span>
              <span className="ml-auto">
                <StatusDot variant={variant} size={6} />
              </span>
            </button>
          )
        })}
      </div>

      <SectionHeader label={step.label} meta={metaFor(step.id)} />
      <p className="mt-2 text-[11px] text-dim">{step.hint}</p>

      <div className="mt-4">
        {step.id === 'credentials' && <CredentialsStep />}
        {step.id === 'github' && <GithubAccountsStep />}
      </div>

      <div className="mt-5 flex items-center justify-between border-t border-border/60 pt-4">
        <Button
          variant="secondary"
          size="md"
          disabled={stepIdx === 0}
          onClick={() => setStepIdx((i) => Math.max(0, i - 1))}
        >
          ← Back
        </Button>
        <div className="text-[10px] uppercase tracking-widest text-dimmer">
          STEP {stepIdx + 1} / {STEPS.length}
        </div>
        {stepIdx < STEPS.length - 1 ? (
          <Button
            variant="primary"
            size="md"
            onClick={() => setStepIdx((i) => Math.min(STEPS.length - 1, i + 1))}
          >
            Next →
          </Button>
        ) : (
          <Button variant="hero" size="md" onClick={close}>
            Close
          </Button>
        )}
      </div>
    </Dialog>
  )
}

/* ────────────────────────────────────────────────────────────── steps */

function CredentialsStep() {
  const toast = useUi((s) => s.toast)
  const present = useUi((s) => s.credentials?.present)
  const [credentialsJson, setCredentialsJson] = useState('')
  const [claudeJson, setClaudeJson] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const valid =
    credentialsJson.trim().length > 0 &&
    claudeJson.trim().length > 0 &&
    isJson(credentialsJson) &&
    isJson(claudeJson)

  async function submit() {
    if (!valid) return
    setBusy(true)
    setError(null)
    try {
      await api.setCredentials(credentialsJson, claudeJson)
      toast('ok', 'claude credentials staged · restart vessels to pick up')
      await refreshNow()
      setCredentialsJson('')
      setClaudeJson('')
    } catch (e) {
      setError(isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }

  async function clear() {
    setBusy(true)
    try {
      await api.clearCredentials()
      toast('ok', 'claude credentials cleared')
      await refreshNow()
    } catch (e) {
      toast('err', isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-3">
      <div className="text-[11px] text-dim leading-relaxed">
        On a laptop where you've run <span className="text-cyan font-mono">claude auth login</span>,
        paste the contents of these two files. Neither is ever read back.
      </div>
      <Textarea
        label="~/.claude/.credentials.json"
        name="creds"
        rows={5}
        placeholder='{ "claudeAiOauth": { ... } }'
        value={credentialsJson}
        error={credentialsJson && !isJson(credentialsJson) ? 'invalid json' : undefined}
        onChange={(e) => setCredentialsJson(e.target.value)}
      />
      <Textarea
        label="~/.claude.json"
        name="claudejson"
        rows={6}
        placeholder='{ "projects": { ... }, "oauthAccount": { ... }, ... }'
        value={claudeJson}
        error={claudeJson && !isJson(claudeJson) ? 'invalid json' : undefined}
        hint="up to ~1 MB"
        onChange={(e) => setClaudeJson(e.target.value)}
      />
      {error && (
        <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">
          {error}
        </div>
      )}
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-widest text-dimmer">
          POST /v1/auth/claude-credentials
        </span>
        <div className="flex gap-2">
          {present && (
            <Button variant="danger" size="sm" onClick={clear} loading={busy}>
              Clear
            </Button>
          )}
          <Button variant="hero" size="md" onClick={submit} disabled={!valid} loading={busy}>
            Stage
          </Button>
        </div>
      </div>
    </div>
  )
}

const ALIAS_RE = /^[a-z0-9_-]{1,32}$/

function GithubAccountsStep() {
  const toast = useUi((s) => s.toast)
  const accounts = useUi((s) => s.githubAccounts) ?? []

  const [alias, setAlias] = useState('')
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const aliasErr = alias.length === 0 ? null : ALIAS_RE.test(alias) ? null : 'a-z 0-9 _ - · max 32'
  const valid = ALIAS_RE.test(alias) && token.trim().length > 10 && !/\s/.test(token.trim())

  async function submit() {
    if (!valid) return
    setBusy(true)
    setError(null)
    try {
      await api.upsertGithubAccount(alias, token.trim())
      toast('ok', `github account "${alias}" staged`)
      await refreshNow()
      setAlias('')
      setToken('')
    } catch (e) {
      setError(isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }

  async function remove(a: string) {
    if (!confirm(`Remove github account "${a}"? Vessels using it will lose GITHUB_TOKEN on next restart.`)) {
      return
    }
    try {
      await api.deleteGithubAccount(a)
      toast('ok', `github account "${a}" removed`)
      await refreshNow()
    } catch (e) {
      toast('err', isApiError(e) ? e.message : 'failed')
    }
  }

  return (
    <div className="space-y-4">
      {/* Existing accounts list */}
      <div className="space-y-1.5">
        {accounts.length === 0 ? (
          <div className="border border-border/60 bg-surface-2/30 px-3 py-3 text-[11px] text-dim text-center">
            no github accounts yet — add one below
          </div>
        ) : (
          accounts.map((a) => (
            <div
              key={a.alias}
              className="flex items-center justify-between border border-border bg-surface-2/40 px-3 py-2"
            >
              <div className="flex items-center gap-3 min-w-0">
                <StatusDot variant={a.present ? 'green' : 'red'} pulse={a.present} />
                <span className="font-mono text-sm text-text">{a.alias}</span>
                <span className="text-[10px] uppercase tracking-widest text-dimmer">
                  {a.present
                    ? a.set_at
                      ? `staged ${relativeTime(a.set_at)}`
                      : 'staged'
                    : 'token missing'}
                </span>
              </div>
              <Button variant="danger" size="sm" onClick={() => remove(a.alias)}>
                Remove
              </Button>
            </div>
          ))
        )}
      </div>

      {/* Add / replace form */}
      <div className="border border-border/60 bg-surface-2/30 p-3 space-y-3">
        <div className="text-[10px] uppercase tracking-widest text-dim">
          Add account · or replace existing alias
        </div>
        <Field
          label="Alias"
          name="alias"
          placeholder="personal"
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          value={alias}
          error={aliasErr ?? undefined}
          hint="a-z 0-9 _ - · max 32"
          onChange={(e) => setAlias(e.target.value.toLowerCase())}
        />
        <Field
          label="PAT"
          name="github_token"
          type="password"
          placeholder="github_pat_…"
          autoComplete="off"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          hint="fine-grained · contents:RW + workflow"
        />
        {error && (
          <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">
            {error}
          </div>
        )}
        <div className="flex items-center justify-between">
          <span className="text-[10px] uppercase tracking-widest text-dimmer">
            PUT /v1/auth/github-accounts/{alias || '<alias>'}
          </span>
          <Button variant="hero" size="md" onClick={submit} disabled={!valid} loading={busy}>
            Stage
          </Button>
        </div>
      </div>
    </div>
  )
}

function isJson(s: string): boolean {
  try {
    JSON.parse(s)
    return true
  } catch {
    return false
  }
}
