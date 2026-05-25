import { useUi } from '../store/ui'
import { relativeTime } from '../lib/time'
import { StatusDot } from './ui/StatusDot'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { cn } from '../lib/cn'

/**
 * Two cards: claude session (singleton) + github accounts (collection).
 * Click either to open the credential wizard at the relevant step.
 */
export function CredentialStatusRow() {
  const credentials = useUi((s) => s.credentials)
  const accounts = useUi((s) => s.githubAccounts)
  const openWizard = useUi((s) => s.openWizard)
  const initialLoad = useUi((s) => s.initialLoad)

  const credPresent = credentials?.present === true
  const credKnown = credentials !== null
  const credExpired = credentials?.expired === true
  const accountsKnown = accounts !== null
  const presentAccounts = (accounts ?? []).filter((a) => a.present)

  // What's "missing": claude session unstaged, OR github has 0 accounts.
  // Expired creds count too — they're staged but unusable for new deploys.
  const missing: string[] = []
  if (credKnown && !credPresent) missing.push('claude session')
  if (credPresent && credExpired) missing.push('claude session (expired)')
  if (accountsKnown && presentAccounts.length === 0) missing.push('github account')

  return (
    <section className="mt-6">
      <SectionHeader
        label="Operator Credentials"
        meta={
          initialLoad
            ? 'reading…'
            : missing.length === 0
              ? 'all staged'
              : `${missing.length} unstaged`
        }
      />
      <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 sm:gap-3">
        {/* Claude session card */}
        <button
          onClick={() => openWizard(false)}
          className={cn(
            'group relative flex items-start gap-3 border bg-surface/60 px-3.5 py-3 text-left',
            'transition-colors duration-150',
            (credKnown && !credPresent) || credExpired
              ? 'border-red/30 hover:border-red/60'
              : 'border-border hover:border-border-bright',
          )}
        >
          <span className="mt-1.5 shrink-0">
            <StatusDot
              variant={credExpired ? 'red' : credPresent ? 'green' : credKnown ? 'red' : 'dim'}
              pulse={credPresent && !credExpired}
            />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[11px] uppercase tracking-wider text-text">
                Claude Session
              </span>
              <span
                className={cn(
                  'text-[10px] uppercase tracking-widest tabular-nums',
                  credExpired
                    ? 'text-red'
                    : credPresent
                      ? 'text-green'
                      : credKnown
                        ? 'text-red'
                        : 'text-dimmer',
                )}
              >
                {credExpired
                  ? 'EXPIRED'
                  : credPresent
                    ? 'staged'
                    : credKnown
                      ? 'absent'
                      : '—'}
              </span>
            </div>
            <div className="mt-0.5 text-[10px] text-dim truncate">
              full-scope login — required for Remote Control
            </div>
            <ExpiryLine
              setAt={credentials?.set_at ?? null}
              expiresAt={credentials?.expires_at ?? null}
              expired={credExpired}
            />
          </div>
        </button>

        {/* GitHub accounts card */}
        <button
          onClick={() => openWizard(false)}
          className={cn(
            'group relative flex items-start gap-3 border bg-surface/60 px-3.5 py-3 text-left',
            'transition-colors duration-150',
            accountsKnown && presentAccounts.length === 0
              ? 'border-red/30 hover:border-red/60'
              : 'border-border hover:border-border-bright',
          )}
        >
          <span className="mt-1.5 shrink-0">
            <StatusDot
              variant={
                presentAccounts.length > 0
                  ? 'green'
                  : accountsKnown
                    ? 'red'
                    : 'dim'
              }
              pulse={presentAccounts.length > 0}
            />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[11px] uppercase tracking-wider text-text">
                GitHub Accounts
              </span>
              <span
                className={cn(
                  'text-[10px] uppercase tracking-widest tabular-nums',
                  presentAccounts.length > 0
                    ? 'text-green'
                    : accountsKnown
                      ? 'text-red'
                      : 'text-dimmer',
                )}
              >
                {accountsKnown
                  ? `${presentAccounts.length} staged`
                  : '—'}
              </span>
            </div>
            <div className="mt-0.5 text-[10px] text-dim truncate">
              one or more fine-grained PATs · pick one per vessel
            </div>
            <div className="mt-1.5 text-[10px] tracking-wider text-dimmer truncate font-mono">
              {presentAccounts.length > 0
                ? presentAccounts.map((a) => a.alias).join(' · ')
                : 'none yet'}
            </div>
          </div>
        </button>
      </div>

      {/* CTA when something missing or expired */}
      {!initialLoad && (missing.length > 0 || credExpired) && (
        <div
          className={cn(
            'mt-3 flex items-center justify-between border px-4 py-2.5',
            credExpired
              ? 'border-red/40 bg-red/[0.05]'
              : 'border-amber/30 bg-amber/[0.05]',
          )}
        >
          <div className="flex items-baseline gap-3">
            <span
              className={cn(
                'text-xs tracking-widest',
                credExpired ? 'text-red' : 'text-amber',
              )}
            >
              {credExpired ? '! EXPIRED' : '! BRIEF'}
            </span>
            <span
              className={cn(
                'text-[11px] tracking-wider',
                credExpired ? 'text-red/90' : 'text-amber/90',
              )}
            >
              {credExpired
                ? 'Claude session expired. New deploys are blocked. Restage from your laptop: scripts/restage-claude-credentials.sh'
                : missing.length === 2
                  ? 'No credentials staged. New vessels will register but will fail to authenticate.'
                  : `Missing: ${missing.join(', ')}. Run the setup brief.`}
            </span>
          </div>
          <Button variant="hero" size="sm" onClick={() => openWizard(true)}>
            Open Brief →
          </Button>
        </div>
      )}
    </section>
  )
}

/**
 * Compact "set X ago · expires in Y" line under the claude session
 * card. Colors the expiry side amber when <1h, red when expired.
 */
function ExpiryLine({
  setAt,
  expiresAt,
  expired,
}: {
  setAt: string | null
  expiresAt: string | null
  expired: boolean
}) {
  if (!setAt && !expiresAt) {
    return <div className="mt-1.5 text-[10px] tracking-wider text-dimmer">not set</div>
  }
  const setLine = setAt ? `set ${relativeTime(setAt)}` : null
  let expiryLine: { text: string; color: string } | null = null
  if (expiresAt) {
    const ms = Date.parse(expiresAt) - Date.now()
    if (expired) {
      expiryLine = { text: `expired ${relativeTime(expiresAt)}`, color: 'text-red' }
    } else if (ms < 60 * 60 * 1000) {
      expiryLine = { text: `expires ${relativeTime(expiresAt)}`, color: 'text-amber' }
    } else {
      expiryLine = { text: `expires ${relativeTime(expiresAt)}`, color: 'text-dimmer' }
    }
  }
  return (
    <div className="mt-1.5 flex flex-wrap items-baseline gap-x-2 text-[10px] tracking-wider">
      {setLine && <span className="text-dimmer">{setLine}</span>}
      {expiryLine && <span className={cn(expiryLine.color)}>· {expiryLine.text}</span>}
    </div>
  )
}
