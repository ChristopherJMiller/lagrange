import { useUi } from '../store/ui'

/**
 * The "deploy new vessel" affordance — sits at the end of the fleet grid.
 * Designed to feel like an empty launch pad: dashed border + hatched
 * background + reticle in the middle.
 */
export function DeployTile() {
  const openDeploy = useUi((s) => s.openDeploy)
  return (
    <button
      onClick={openDeploy}
      className="group relative flex min-h-[280px] flex-col items-center justify-center gap-3 border border-dashed border-border hover:border-cyan hover:bg-cyan/[0.03] transition-colors"
    >
      <div className="hatch absolute inset-0 opacity-40 group-hover:opacity-100 transition-opacity" />
      <svg width="56" height="56" viewBox="0 0 56 56" className="relative text-dim group-hover:text-cyan transition-colors">
        <circle cx="28" cy="28" r="22" stroke="currentColor" strokeWidth="0.6" strokeDasharray="2 3" fill="none" />
        <circle cx="28" cy="28" r="12" stroke="currentColor" strokeWidth="0.8" fill="none" />
        <line x1="28" y1="6" x2="28" y2="14" stroke="currentColor" strokeWidth="1" />
        <line x1="28" y1="42" x2="28" y2="50" stroke="currentColor" strokeWidth="1" />
        <line x1="6" y1="28" x2="14" y2="28" stroke="currentColor" strokeWidth="1" />
        <line x1="42" y1="28" x2="50" y2="28" stroke="currentColor" strokeWidth="1" />
        <circle cx="28" cy="28" r="3" fill="currentColor" />
      </svg>
      <div className="relative text-center">
        <div className="text-xs uppercase tracking-widest text-dim group-hover:text-text transition-colors">
          Deploy New Vessel
        </div>
        <div className="mt-1 text-[10px] tracking-wider text-dimmer">
          name · repo · branch
        </div>
      </div>
    </button>
  )
}
