import type {
  ApiError,
  CreateVmReq,
  CreateVmResp,
  CredStatus,
  HealthDto,
  VmDto,
} from './types'

const BASE = ''

async function request<T = unknown>(path: string, init?: RequestInit): Promise<T> {
  let res: Response
  try {
    res = await fetch(`${BASE}${path}`, {
      ...init,
      headers: {
        ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
        Accept: 'application/json, text/plain;q=0.8, */*;q=0.5',
        ...init?.headers,
      },
      credentials: 'include',
    })
  } catch (e) {
    const msg = e instanceof Error ? e.message : 'network error'
    throw <ApiError>{ status: 0, message: msg }
  }

  if (!res.ok) {
    let body: { error?: string; detail?: string } = {}
    try {
      body = await res.json()
    } catch {
      // body might be empty or non-json
    }
    throw <ApiError>{
      status: res.status,
      code: body.error,
      detail: body.detail,
      message: body.detail ?? body.error ?? res.statusText ?? `HTTP ${res.status}`,
    }
  }

  if (res.status === 204) return undefined as T
  const ct = res.headers.get('content-type') ?? ''
  if (ct.includes('application/json')) return (await res.json()) as T
  return (await res.text()) as unknown as T
}

export const api = {
  health: () => request<HealthDto>('/v1/health'),

  listVms: () => request<VmDto[]>('/v1/repos'),
  getVm: (n: string) => request<VmDto>(`/v1/repos/${encodeURIComponent(n)}`),
  createVm: (b: CreateVmReq) =>
    request<CreateVmResp>('/v1/repos', { method: 'POST', body: JSON.stringify(b) }),
  deleteVm: (n: string, wipe: boolean) =>
    request<void>(
      `/v1/repos/${encodeURIComponent(n)}?wipe_persistent=${wipe ? 'true' : 'false'}`,
      { method: 'DELETE' },
    ),
  startVm: (n: string) =>
    request<void>(`/v1/repos/${encodeURIComponent(n)}/start`, { method: 'POST' }),
  stopVm: (n: string) =>
    request<void>(`/v1/repos/${encodeURIComponent(n)}/stop`, { method: 'POST' }),
  restartVm: (n: string) =>
    request<void>(`/v1/repos/${encodeURIComponent(n)}/restart`, { method: 'POST' }),
  getLogs: (n: string, lines = 200) =>
    request<string>(`/v1/repos/${encodeURIComponent(n)}/logs?lines=${lines}`),

  getOauthToken: () => request<CredStatus>('/v1/auth/claude-oauth-token'),
  setOauthToken: (token: string) =>
    request<void>('/v1/auth/claude-oauth-token', {
      method: 'POST',
      body: JSON.stringify({ token }),
    }),
  clearOauthToken: () =>
    request<void>('/v1/auth/claude-oauth-token', { method: 'DELETE' }),

  getCredentials: () => request<CredStatus>('/v1/auth/claude-credentials'),
  setCredentials: (credentials_json: string, claude_json: string) =>
    request<void>('/v1/auth/claude-credentials', {
      method: 'POST',
      body: JSON.stringify({ credentials_json, claude_json }),
    }),
  clearCredentials: () =>
    request<void>('/v1/auth/claude-credentials', { method: 'DELETE' }),

  getGithubToken: () => request<CredStatus>('/v1/auth/github-token'),
  setGithubToken: (token: string) =>
    request<void>('/v1/auth/github-token', {
      method: 'POST',
      body: JSON.stringify({ token }),
    }),
  clearGithubToken: () =>
    request<void>('/v1/auth/github-token', { method: 'DELETE' }),
}
