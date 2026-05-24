import { create } from 'zustand'
import type { CredStatus, HealthDto, VmDto } from '../api/types'

export type CredKind = 'credentials' | 'github'

export type ToastKind = 'ok' | 'err' | 'info'
export type ToastItem = { id: string; kind: ToastKind; msg: string; ts: number }

type UiState = {
  // server state
  vms: VmDto[] | null
  vmsError: string | null
  initialLoad: boolean
  lastFetched: number | null

  health: HealthDto | null
  creds: Record<CredKind, CredStatus | null>

  // optimistic in-flight per-VM actions
  busy: Record<string, 'start' | 'stop' | 'restart' | 'destroy' | undefined>

  // dialogs
  deployOpen: boolean
  destroyTarget: VmDto | null
  wizardOpen: boolean
  wizardForced: boolean // open because no creds present
  logsTarget: VmDto | null

  // toasts
  toasts: ToastItem[]

  setVms: (vms: VmDto[]) => void
  setVmsError: (err: string | null) => void
  setHealth: (h: HealthDto | null) => void
  setCred: (kind: CredKind, v: CredStatus | null) => void
  markInitialLoaded: () => void
  setLastFetched: (ts: number) => void
  setBusy: (name: string, kind: UiState['busy'][string]) => void

  openDeploy: () => void
  closeDeploy: () => void
  openDestroy: (vm: VmDto) => void
  closeDestroy: () => void
  openWizard: (forced?: boolean) => void
  closeWizard: () => void
  openLogs: (vm: VmDto) => void
  closeLogs: () => void

  toast: (kind: ToastKind, msg: string) => void
  dismissToast: (id: string) => void
}

export const useUi = create<UiState>((set) => ({
  vms: null,
  vmsError: null,
  initialLoad: true,
  lastFetched: null,

  health: null,
  creds: { credentials: null, github: null },

  busy: {},

  deployOpen: false,
  destroyTarget: null,
  wizardOpen: false,
  wizardForced: false,
  logsTarget: null,

  toasts: [],

  setVms: (vms) => set({ vms }),
  setVmsError: (err) => set({ vmsError: err }),
  setHealth: (h) => set({ health: h }),
  setCred: (kind, v) =>
    set((s) => ({ creds: { ...s.creds, [kind]: v } })),
  markInitialLoaded: () => set({ initialLoad: false }),
  setLastFetched: (ts) => set({ lastFetched: ts }),
  setBusy: (name, kind) =>
    set((s) => {
      const next = { ...s.busy }
      if (kind == null) delete next[name]
      else next[name] = kind
      return { busy: next }
    }),

  openDeploy: () => set({ deployOpen: true }),
  closeDeploy: () => set({ deployOpen: false }),
  openDestroy: (vm) => set({ destroyTarget: vm }),
  closeDestroy: () => set({ destroyTarget: null }),
  openWizard: (forced = false) => set({ wizardOpen: true, wizardForced: forced }),
  closeWizard: () => set({ wizardOpen: false, wizardForced: false }),
  openLogs: (vm) => set({ logsTarget: vm }),
  closeLogs: () => set({ logsTarget: null }),

  toast: (kind, msg) =>
    set((s) => ({
      toasts: [
        ...s.toasts,
        { id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`, kind, msg, ts: Date.now() },
      ],
    })),
  dismissToast: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}))
