/**
 * VM size tiers. Purely client-side: the admin API takes raw vcpu/mem_mb
 * and doesn't care about names. Tiers exist to make the common case a
 * single click and to nudge the operator away from one-off sizes that
 * are hard to compare on the capacity bar.
 *
 * Roughly modeled on EC2 t-class sizing — keep cpu:mem ratio at 1:1
 * (one GiB per vCPU) since that's what most repo builds want.
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
    memGib: 1,
    blurb: 'docs · small scripts',
  },
  {
    id: 'small',
    label: 'small',
    vcpu: 2,
    memGib: 2,
    blurb: 'most web apps',
  },
  {
    id: 'medium',
    label: 'medium',
    vcpu: 4,
    memGib: 4,
    blurb: 'typical default',
  },
  {
    id: 'large',
    label: 'large',
    vcpu: 8,
    memGib: 8,
    blurb: 'heavier builds',
  },
  {
    id: 'xlarge',
    label: 'xlarge',
    vcpu: 16,
    memGib: 16,
    blurb: 'rust + native deps',
  },
]

export function tierForSize(vcpu: number, memMb: number): TierId {
  const memGib = memMb / 1024
  const match = TIERS.find((t) => t.vcpu === vcpu && t.memGib === memGib)
  return match?.id ?? 'custom'
}
