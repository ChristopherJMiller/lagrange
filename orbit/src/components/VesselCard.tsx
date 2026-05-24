import { useMemo } from 'react'
import type { VmDto } from '../api/types'
import { api } from '../api/client'
import { refreshNow } from '../api/hooks'
import { isApiError } from '../api/types'
import { useUi } from '../store/ui'
import { Button } from './ui/Button'
import { StatusDot } from './ui/StatusDot'
import { DataRow } from './ui/Bracket'
import { formatMem } from '../lib/time'
import { cn } from '../lib/cn'

type StatusLook = {
  dot: 'green' | 'amber' | 'red' | 'cyan' | 'dim'
  label: string
  textClass: string
  borderClass: string
  liveScan: boolean
}

function lookup(vm: VmDto): StatusLook {
  const s = vm.status.toLowerCase()
  if (vm.runtime_active && (s === 'running' || s === 'active')) {
    return {
      dot: 'green',
      label: 'IN ORBIT',
      textClass: 'text-green',
      borderClass: 'border-border',
      liveScan: true,
    }
  }
  if (s === 'provisioning' || s === 'starting') {
    return {
      dot: 'amber',
      label: 'ASCENT',
      textClass: 'text-amber',
      borderClass: 'border-amber/40',
      liveScan: false,
    }
  }
  if (s === 'stopped' || s === 'inactive') {
    return {
      dot: 'dim',
      label: 'DORMANT',
      textClass: 'text-dim',
      borderClass: 'border-border',
      liveScan: false,
    }
  }
  if (s === 'failed' || s === 'error') {
    return {
      dot: 'red',
      label: 'FAULT',
      textClass: 'text-red',
      borderClass: 'border-red/40',
      liveScan: false,
    }
  }
  return {
    dot: 'cyan',
    label: s.toUpperCase() || 'UNKNOWN',
    textClass: 'text-cyan',
    borderClass: 'border-border',
    liveScan: false,
  }
}

function shortRepo(url: string): string {
  // git@github.com:foo/bar.git → foo/bar
  // https://github.com/foo/bar(.git) → foo/bar
  const cleaned = url.replace(/\.git$/, '')
  const ssh = cleaned.match(/[:/]([^:/]+\/[^:/]+)$/)
  return ssh ? ssh[1] : cleaned.replace(/^https?:\/\/[^/]+\//, '')
}

export function VesselCard({ vm }: { vm: VmDto }) {
  const openDestroy = useUi((s) => s.openDestroy)
  const openLogs = useUi((s) => s.openLogs)
  const setBusy = useUi((s) => s.setBusy)
  const busy = useUi((s) => s.busy[vm.name])
  const toast = useUi((s) => s.toast)

  const look = useMemo(() => lookup(vm), [vm])
  const hasSession = !!vm.claude_session_name && vm.runtime_active
  const hasDeepLink = !!vm.claude_session_url

  async function action(kind: 'start' | 'stop' | 'restart', label: string) {
    setBusy(vm.name, kind)
    try {
      if (kind === 'start') await api.startVm(vm.name)
      else if (kind === 'stop') await api.stopVm(vm.name)
      else await api.restartVm(vm.name)
      toast('ok', `${vm.name}: ${label}`)
      await refreshNow()
    } catch (e) {
      toast('err', isApiError(e) ? `${vm.name}: ${e.message}` : `${vm.name}: failed`)
    } finally {
      setBusy(vm.name, undefined)
    }
  }

  function openDrive() {
    // Deep link wins: the admin service publishes it as soon as the
    // guest's claude-session-publisher scrapes the typescript.
    if (vm.claude_session_url) {
      window.open(vm.claude_session_url, '_blank', 'noopener,noreferrer')
      return
    }
    // Fallback while the URL is still pending: copy the session name so
    // the operator can fuzzy-find it in the claude.ai/code sidebar.
    if (vm.claude_session_name) {
      try {
        navigator.clipboard.writeText(vm.claude_session_name)
        toast('info', `session "${vm.claude_session_name}" copied — find it in claude.ai/code`)
      } catch {
        toast('info', `find session "${vm.claude_session_name}" in claude.ai/code`)
      }
    }
    window.open('https://claude.ai/code', '_blank', 'noopener,noreferrer')
  }

  return (
    <article
      className={cn(
        'group relative flex flex-col border bg-surface/85',
        'transition-colors duration-150',
        look.borderClass,
        'hover:border-border-hot',
      )}
    >
      {/* corner ticks */}
      <span className={cn('text-border-hot corner-ticks pointer-events-none absolute inset-0')} />

      {/* live scan */}
      {look.liveScan && (
        <div className="pointer-events-none absolute inset-0 overflow-hidden">
          <div
            className="absolute -left-2 -right-2 h-24 opacity-60"
            style={{
              background:
                'linear-gradient(to bottom, transparent 0%, rgba(127, 241, 159, 0.06) 50%, transparent 100%)',
              animation: 'scan 7s linear infinite',
            }}
          />
        </div>
      )}

      {/* header */}
      <div className="flex items-start justify-between gap-3 border-b border-border/70 bg-surface-2/80 px-4 py-2.5">
        <div className="min-w-0">
          <div className="flex items-baseline gap-2">
            <span className="text-cyan/50 text-xs">[</span>
            <h3 className="font-mono font-bold text-base text-text tracking-wider truncate">
              {vm.name}
            </h3>
            <span className="text-cyan/50 text-xs">]</span>
          </div>
          <div className="mt-0.5 text-[10px] uppercase tracking-widest text-dimmer">
            VESSEL · {vm.vm_ip}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <StatusDot variant={look.dot} pulse={look.liveScan} />
          <span className={cn('text-[10px] uppercase tracking-widest tabular-nums', look.textClass)}>
            {look.label}
          </span>
        </div>
      </div>

      {/* body — telemetry readout */}
      <div className="flex-1 space-y-1.5 px-4 py-3">
        <DataRow label="REPO" value={shortRepo(vm.repo_url)} />
        <DataRow label="BRANCH" value={vm.branch} accent />
        <DataRow
          label="THRUST"
          value={
            <span className="text-text">
              {vm.vcpu} <span className="text-dimmer">vCPU</span>
              <span className="text-dimmer mx-1.5">·</span>
              {formatMem(vm.mem_mb)}
            </span>
          }
        />
        <DataRow
          label="SESSION"
          value={
            vm.claude_session_name ? (
              <span className="text-cyan">{vm.claude_session_name}</span>
            ) : (
              <span className="text-dimmer">— pending —</span>
            )
          }
        />
      </div>

      {/* divider */}
      <div className="mx-4 divider-dotted h-px" />

      {/* actions */}
      <div className="space-y-2 px-4 py-3">
        <Button
          variant="hero"
          size="md"
          className="w-full justify-between"
          disabled={!hasSession && !vm.runtime_active && !hasDeepLink}
          onClick={openDrive}
          title={
            hasDeepLink
              ? `Open session at ${vm.claude_session_url}`
              : hasSession
                ? `Open claude.ai/code (copies "${vm.claude_session_name}" to clipboard)`
                : 'Open claude.ai/code'
          }
        >
          <span className="flex items-center gap-2">
            <span className="text-amber/60">{hasDeepLink ? '◉' : '▸'}</span>
            <span>
              {hasDeepLink
                ? 'Drive Agent'
                : hasSession
                  ? 'Drive Agent'
                  : 'Open claude.ai'}
            </span>
          </span>
          <span className="text-amber/70 transition-transform group-hover:translate-x-0.5">⤴</span>
        </Button>

        <div className="flex flex-wrap items-center gap-1.5">
          {vm.runtime_active ? (
            <>
              <Button
                size="sm"
                variant="secondary"
                loading={busy === 'restart'}
                onClick={() => action('restart', 'restart issued')}
              >
                Restart
              </Button>
              <Button
                size="sm"
                variant="secondary"
                loading={busy === 'stop'}
                onClick={() => action('stop', 'stop issued')}
              >
                Stop
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="secondary"
              loading={busy === 'start'}
              onClick={() => action('start', 'start issued')}
            >
              Start
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={() => openLogs(vm)}>
            Logs
          </Button>
          <div className="flex-1" />
          <Button size="sm" variant="danger" onClick={() => openDestroy(vm)}>
            Destroy
          </Button>
        </div>
      </div>
    </article>
  )
}
