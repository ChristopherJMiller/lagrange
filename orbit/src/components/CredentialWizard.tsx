import { useEffect, useMemo, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { Field, Textarea } from './ui/Input'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { StatusDot } from './ui/StatusDot'
import { useUi, type CredKind } from '../store/ui'
import { api } from '../api/client'
import { isApiError } from '../api/types'
import { refreshNow } from '../api/hooks'
import { relativeTime } from '../lib/time'
import { cn } from '../lib/cn'

type StepId = CredKind

const STEPS: { id: StepId; label: string; hint: string }[] = [
  {
    id: 'credentials',
    label: 'Claude Session',
    hint: 'Full-scope login. Required for Remote Control. Paste two files from your local machine.',
  },
  {
    id: 'oauth',
    label: 'Claude OAuth Token',
    hint: 'Inference-only fallback token from `claude setup-token`. Single line.',
  },
  {
    id: 'github',
    label: 'GitHub PAT',
    hint: 'Fine-grained personal access token. Used by `git push` inside each vessel.',
  },
]

export function CredentialWizard() {
  const open = useUi((s) => s.wizardOpen)
  const forced = useUi((s) => s.wizardForced)
  const close = useUi((s) => s.closeWizard)
  const creds = useUi((s) => s.creds)

  // Resolve a useful starting step: the first missing one, or step 1 if all present.
  const firstMissingIdx = useMemo(
    () => Math.max(0, STEPS.findIndex((s) => !creds[s.id]?.present)),
    [creds],
  )
  const [stepIdx, setStepIdx] = useState(firstMissingIdx)

  useEffect(() => {
    if (open) setStepIdx(firstMissingIdx)
  }, [open, firstMissingIdx])

  const step = STEPS[stepIdx]

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
          const present = creds[s.id]?.present
          const variant = present === true ? 'green' : present === false ? 'red' : 'dim'
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
              <span className="text-cyan/60 font-bold tabular-nums text-xs">
                0{i + 1}
              </span>
              <span className="text-[11px] uppercase tracking-wider truncate">{s.label}</span>
              <span className="ml-auto">
                <StatusDot variant={variant} size={6} />
              </span>
            </button>
          )
        })}
      </div>

      <SectionHeader label={step.label} meta={statusMeta(creds[step.id])} />
      <p className="mt-2 text-[11px] text-dim">{step.hint}</p>

      <div className="mt-4">
        {step.id === 'credentials' && <CredentialsStep />}
        {step.id === 'oauth' && <OauthStep />}
        {step.id === 'github' && <GithubStep />}
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

  function statusMeta(s: ReturnType<typeof useUi.getState>['creds'][CredKind]) {
    if (!s) return 'unknown'
    if (s.present) return `staged · ${relativeTime(s.set_at)}`
    return 'absent'
  }
}

/* ────────────────────────────────────────────────────────────── steps */

function CredentialsStep() {
  const toast = useUi((s) => s.toast)
  const present = useUi((s) => s.creds.credentials?.present)
  const [credentialsJson, setCredentialsJson] = useState('')
  const [claudeJson, setClaudeJson] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const valid =
    credentialsJson.trim().length > 0 && claudeJson.trim().length > 0 && isJson(credentialsJson) && isJson(claudeJson)

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
      {error && <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">{error}</div>}
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

function OauthStep() {
  const toast = useUi((s) => s.toast)
  const present = useUi((s) => s.creds.oauth?.present)
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const valid = token.trim().length > 8 && !token.includes('\n')

  async function submit() {
    if (!valid) return
    setBusy(true)
    setError(null)
    try {
      await api.setOauthToken(token.trim())
      toast('ok', 'claude oauth token staged')
      await refreshNow()
      setToken('')
    } catch (e) {
      setError(isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }
  async function clear() {
    setBusy(true)
    try {
      await api.clearOauthToken()
      toast('ok', 'claude oauth token cleared')
      await refreshNow()
    } catch (e) {
      toast('err', isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="space-y-3">
      <Field
        label="Token"
        name="oauth"
        type="password"
        placeholder="sk-ant-oat01-…"
        autoComplete="off"
        value={token}
        onChange={(e) => setToken(e.target.value)}
      />
      {error && <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">{error}</div>}
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-widest text-dimmer">
          POST /v1/auth/claude-oauth-token
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

function GithubStep() {
  const toast = useUi((s) => s.toast)
  const present = useUi((s) => s.creds.github?.present)
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const valid = token.trim().length > 10 && !/\s/.test(token)

  async function submit() {
    if (!valid) return
    setBusy(true)
    setError(null)
    try {
      await api.setGithubToken(token.trim())
      toast('ok', 'github token staged')
      await refreshNow()
      setToken('')
    } catch (e) {
      setError(isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }
  async function clear() {
    setBusy(true)
    try {
      await api.clearGithubToken()
      toast('ok', 'github token cleared')
      await refreshNow()
    } catch (e) {
      toast('err', isApiError(e) ? e.message : 'failed')
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="space-y-3">
      <Field
        label="PAT"
        name="github"
        type="password"
        placeholder="github_pat_…"
        autoComplete="off"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        hint="fine-grained, contents:RW + workflow"
      />
      {error && <div className="border border-red/40 bg-red/5 px-3 py-2 text-xs text-red font-mono">{error}</div>}
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-widest text-dimmer">
          POST /v1/auth/github-token
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

function isJson(s: string): boolean {
  try {
    JSON.parse(s)
    return true
  } catch {
    return false
  }
}
