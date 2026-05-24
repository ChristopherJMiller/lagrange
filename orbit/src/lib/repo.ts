/**
 * Best-effort owner/repo extraction from a git URL. Accepts the three
 * shapes the operator might paste:
 *
 *   git@github.com:owner/repo.git
 *   https://github.com/owner/repo(.git)
 *   ssh://git@github.com/owner/repo(.git)
 *
 * Returns null when the URL doesn't look like github (we only call the
 * branches endpoint for github URLs; other forges aren't supported yet).
 */
export function parseGithubOwnerRepo(url: string): { owner: string; repo: string } | null {
  if (!url) return null
  const cleaned = url.trim().replace(/\.git$/, '')

  // git@github.com:owner/repo
  const ssh = cleaned.match(/^git@github\.com:([^/]+)\/([^/]+?)$/)
  if (ssh) return { owner: ssh[1], repo: ssh[2] }

  // https://github.com/owner/repo  OR  ssh://git@github.com/owner/repo
  const url2 = cleaned.match(/^(?:https?|ssh):\/\/(?:[^@]+@)?github\.com\/([^/]+)\/([^/]+?)$/)
  if (url2) return { owner: url2[1], repo: url2[2] }

  return null
}
