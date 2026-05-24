export type VmDto = {
  name: string
  repo_url: string
  branch: string
  vm_ip: string
  vcpu: number
  mem_mb: number
  status: string
  runtime_active: boolean
  claude_session_name: string | null
  claude_session_url: string | null
}

export type CredStatus = {
  present: boolean
  set_at: string | null
}

export type HealthDto = {
  status: string
  vms_running: number
  vms_total: number
}

export type CreateVmReq = {
  name: string
  repo_url: string
  branch?: string
  vcpu?: number
  mem_mb?: number
}

export type CreateVmResp = {
  name: string
  status: string
  vm_ip: string
  claude_session_hint: string
}

export type ApiError = {
  status: number
  code?: string
  detail?: string
  message: string
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === 'object' && e !== null && 'status' in e && 'message' in e
}
