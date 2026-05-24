import { useEffect, useRef } from 'react'
import { api } from './client'
import { isApiError } from './types'
import { useUi } from '../store/ui'

const POLL_MS = 5000

/**
 * Single polling loop that fans out to all the read endpoints.
 * Writes results into the zustand store. Survives transient failures
 * (just sets vmsError; next tick clears it on success).
 */
export function usePolling() {
  const ranRef = useRef(false)

  useEffect(() => {
    if (ranRef.current) return
    ranRef.current = true

    let cancelled = false
    let timer: number | undefined

    const tick = async () => {
      const ui = useUi.getState()
      try {
        const [vms, health, oauth, credentials, github] = await Promise.all([
          api.listVms(),
          api.health().catch(() => null),
          api.getOauthToken().catch(() => null),
          api.getCredentials().catch(() => null),
          api.getGithubToken().catch(() => null),
        ])
        if (cancelled) return
        ui.setVms(vms)
        ui.setHealth(health)
        ui.setCred('oauth', oauth)
        ui.setCred('credentials', credentials)
        ui.setCred('github', github)
        ui.setVmsError(null)
        ui.setLastFetched(Date.now())
        ui.markInitialLoaded()
      } catch (e) {
        if (cancelled) return
        const msg = isApiError(e) ? e.message : 'fetch failed'
        ui.setVmsError(msg)
        ui.markInitialLoaded()
      } finally {
        if (!cancelled) {
          timer = window.setTimeout(tick, POLL_MS)
        }
      }
    }

    tick()
    return () => {
      cancelled = true
      if (timer) window.clearTimeout(timer)
    }
  }, [])
}

/** Force an immediate refresh (after a mutation). */
export async function refreshNow() {
  const ui = useUi.getState()
  try {
    const [vms, health, oauth, credentials, github] = await Promise.all([
      api.listVms(),
      api.health().catch(() => null),
      api.getOauthToken().catch(() => null),
      api.getCredentials().catch(() => null),
      api.getGithubToken().catch(() => null),
    ])
    ui.setVms(vms)
    ui.setHealth(health)
    ui.setCred('oauth', oauth)
    ui.setCred('credentials', credentials)
    ui.setCred('github', github)
    ui.setVmsError(null)
    ui.setLastFetched(Date.now())
  } catch (e) {
    const msg = isApiError(e) ? e.message : 'fetch failed'
    ui.setVmsError(msg)
  }
}
