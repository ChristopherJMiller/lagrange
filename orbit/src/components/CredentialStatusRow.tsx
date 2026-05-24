import { useUi } from '../store/ui'
import { relativeTime } from '../lib/time'
import { StatusDot } from './ui/StatusDot'
import { Button } from './ui/Button'
import { SectionHeader } from './ui/Bracket'
import { cn } from '../lib/cn'

type Item = { kind: 'oauth' | 'credentials' | 'github'; label: string; sub: string }

const ITEMS: Item[] = [
  {
    kind: 'credentials',
    label: 'Claude Session',
    sub: 'full-scope login — required for Remote Control',
  },
  {
    kind: 'oauth',
    label: 'Claude OAuth Token',
    sub: 'inference-only fallback',
  },
  {
    kind: 'github',
    label: 'GitHub PAT',
    sub: 'fine-grained, used for git push',
  },
]

export function CredentialStatusRow() {
  const creds = useUi((s) => s.creds)
  const openWizard = useUi((s) => s.openWizard)
  const initialLoad = useUi((s) => s.initialLoad)

  const missingCount = ITEMS.filter((it) => creds[it.kind] && !creds[it.kind]!.present).length
  const unknownCount = ITEMS.filter((it) => !creds[it.kind]).length
  const allKnown = unknownCount === 0

  return (
    <section className="mt-6">
      <SectionHeader
        label="Operator Credentials"
        meta={
          initialLoad
            ? 'reading...'
            : missingCount === 0 && allKnown
              ? 'all staged'
              : `${missingCount} unstaged`
        }
      />
      <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-3 sm:gap-3">
        {ITEMS.map((it) => {
          const s = creds[it.kind]
          const present = s?.present
          const variant = present === true ? 'green' : present === false ? 'red' : 'dim'
          return (
            <button
              key={it.kind}
              onClick={() => openWizard(false)}
              className={cn(
                'group relative flex items-start gap-3 border bg-surface/60 px-3.5 py-3 text-left',
                'transition-colors duration-150',
                present === false
                  ? 'border-red/30 hover:border-red/60'
                  : 'border-border hover:border-border-bright',
              )}
            >
              <span className="mt-1.5 shrink-0">
                <StatusDot variant={variant} pulse={present === true} />
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="text-[11px] uppercase tracking-wider text-text">
                    {it.label}
                  </span>
                  <span
                    className={cn(
                      'text-[10px] uppercase tracking-widest tabular-nums',
                      present === true
                        ? 'text-green'
                        : present === false
                          ? 'text-red'
                          : 'text-dimmer',
                    )}
                  >
                    {present === true ? 'staged' : present === false ? 'absent' : '—'}
                  </span>
                </div>
                <div className="mt-0.5 text-[10px] text-dim truncate">{it.sub}</div>
                <div className="mt-1.5 text-[10px] tracking-wider text-dimmer">
                  {s?.set_at ? `set ${relativeTime(s.set_at)}` : 'not set'}
                </div>
              </div>
            </button>
          )
        })}
      </div>

      {/* CTA when something missing */}
      {!initialLoad && missingCount > 0 && (
        <div className="mt-3 flex items-center justify-between border border-amber/30 bg-amber/[0.05] px-4 py-2.5">
          <div className="flex items-baseline gap-3">
            <span className="text-amber text-xs tracking-widest">! BRIEF</span>
            <span className="text-[11px] text-amber/90 tracking-wider">
              {missingCount === ITEMS.length
                ? 'No credentials staged. New vessels will register but will fail to authenticate.'
                : `${missingCount} credential${missingCount === 1 ? '' : 's'} missing. Run the setup brief.`}
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
