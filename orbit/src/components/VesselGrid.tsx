import { useUi } from '../store/ui'
import { VesselCard } from './VesselCard'
import { DeployTile } from './DeployTile'
import { SectionHeader } from './ui/Bracket'

export function VesselGrid() {
  const vms = useUi((s) => s.vms)
  const initialLoad = useUi((s) => s.initialLoad)
  const vmsError = useUi((s) => s.vmsError)

  return (
    <section className="mt-8 mb-12">
      <SectionHeader
        label="Fleet Manifest"
        meta={vms ? `${vms.length} VESSEL${vms.length === 1 ? '' : 'S'}` : '—'}
      />

      {initialLoad && !vms ? (
        <SkeletonGrid />
      ) : vmsError && !vms ? (
        <div className="mt-4 border border-red/40 bg-red/5 px-4 py-6 text-sm text-red">
          <div className="text-[10px] uppercase tracking-widest text-red/80">LINK FAULT</div>
          <div className="mt-1 font-mono text-red">{vmsError}</div>
          <div className="mt-2 text-[11px] text-dim">
            Retrying every 5s. If this persists, check the admin service / ingress auth path.
          </div>
        </div>
      ) : vms && vms.length === 0 ? (
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          <div className="col-span-full">
            <div className="border border-border/60 bg-surface/40 px-6 py-10 text-center">
              <div className="text-amber text-[10px] uppercase tracking-widest">[ NO VESSELS ]</div>
              <div className="mt-2 text-sm text-text">
                Fleet is empty. Deploy the first vessel.
              </div>
              <div className="mt-1 text-[11px] text-dim">
                One vessel = one repo-VM. Name it, point it at a git URL, hit launch.
              </div>
            </div>
          </div>
          <DeployTile />
        </div>
      ) : (
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {vms!.map((vm, i) => (
            <div
              key={vm.name}
              style={{ animationDelay: `${Math.min(i * 30, 240)}ms` }}
              className="animate-slide-up"
            >
              <VesselCard vm={vm} />
            </div>
          ))}
          <div style={{ animationDelay: `${Math.min((vms?.length || 0) * 30, 240)}ms` }} className="animate-slide-up">
            <DeployTile />
          </div>
        </div>
      )}
    </section>
  )
}

function SkeletonGrid() {
  return (
    <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      {Array.from({ length: 4 }).map((_, i) => (
        <div
          key={i}
          className="min-h-[280px] border border-border/40 bg-surface/40 animate-pulse-soft"
          style={{ animationDelay: `${i * 80}ms` }}
        >
          <div className="border-b border-border/40 px-4 py-3">
            <div className="h-3 w-20 bg-border" />
          </div>
          <div className="space-y-2 p-4">
            <div className="h-2 w-3/4 bg-border/50" />
            <div className="h-2 w-1/2 bg-border/50" />
            <div className="h-2 w-2/3 bg-border/50" />
          </div>
        </div>
      ))}
    </div>
  )
}
