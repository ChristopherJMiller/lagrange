/**
 * VM size tiers. Purely client-side: the admin API takes raw vcpu/mem_mb
 * and doesn't care about names. Tiers exist to make the common case a
 * single click and to nudge the operator away from one-off sizes that
 * are hard to compare on the capacity bar.
 *
 * **Memory-heavy, asymmetric ratios.** Claude Code itself is I/O bound
 * (the loop is mostly API round-trips to anthropic.com), idles at
 * ~400 MB resident, peaks at ~650 MB in a steady session, but is
 * documented to balloon to several GB during code generation and
 * tens of GB in pathological codebase-analysis scenarios. The build
 * tools (cargo, rust-analyzer, go) are the things that actually
 * parallelize — and those want memory too. Net: pin tiers to a
 * mem:vcpu ratio of 2× → 5×, not 1:1.
 *
 * `custom` lets the operator override with arbitrary values.
 */
export type TierId = 'micro' | 'small' | 'medium' | 'large' | 'xlarge' | 'custom'

export type Tier = {
  id: TierId
  label: string
  vcpu: number
  memGib: number
  blurb: string
}

export const TIERS: Tier[] = [
  {
    id: 'micro',
    label: 'micro',
    vcpu: 1,
    memGib: 2,
    blurb: 'docs · small scripts',
  },
  {
    id: 'small',
    label: 'small',
    vcpu: 2,
    memGib: 4,
    blurb: 'most web apps',
  },
  {
    id: 'medium',
    label: 'medium',
    vcpu: 2,
    memGib: 8,
    blurb: 'typical default',
  },
  {
    id: 'large',
    label: 'large',
    vcpu: 4,
    memGib: 16,
    blurb: 'rust · analyzers',
  },
  {
    id: 'xlarge',
    label: 'xlarge',
    vcpu: 6,
    memGib: 32,
    blurb: 'heavy / leak-prone',
  },
]

export function tierForSize(vcpu: number, memMb: number): TierId {
  const memGib = memMb / 1024
  const match = TIERS.find((t) => t.vcpu === vcpu && t.memGib === memGib)
  return match?.id ?? 'custom'
}
