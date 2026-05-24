export type PermissionMode = 'auto' | 'dangerously-skip'

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
  permission_mode: PermissionMode
}

export type CapacityDto = {
  host: { cpus: number; mem_mb: number }
  allocated: { vms: number; vcpu: number; mem_mb: number }
}

export type GithubRepoDto = {
  full_name: string
  name: string
  default_branch: string
  private: boolean
  ssh_url: string
  clone_url: string
  description: string | null
  pushed_at: string | null
}

export type AgentClaudeMdDto = {
  present: boolean
  set_at: string | null
  bytes: number
  content: string
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
  permission_mode?: PermissionMode
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
