import { useUi } from '../store/ui'

export function Footer() {
  const vmsError = useUi((s) => s.vmsError)
  const version = useUi((s) => s.health?.version)

  return (
    <footer className="relative z-10 border-t border-border bg-bg/70 backdrop-blur-sm">
      <div className="mx-auto flex w-full max-w-[1600px] flex-wrap items-center justify-between gap-y-1 gap-x-6 px-6 lg:px-10 py-3 text-[10px] uppercase tracking-widest text-dimmer">
        <div className="flex items-baseline gap-3">
          <span>ORBIT 0.1</span>
          <span className="text-cyan/50">//</span>
          <span>LAGRANGE Satellite Console</span>
          {version && version.rev !== 'unknown' && version.rev !== 'dev' && (
            <>
              <span className="text-cyan/30">·</span>
              <a
                href={`https://github.com/ChristopherJMiller/lagrange/commit/${version.rev.replace(/-dirty$/, '')}`}
                target="_blank"
                rel="noopener noreferrer"
                className="font-mono normal-case tracking-normal text-dim hover:text-cyan"
                title={`Admin service built from ${version.rev}${version.dirty ? ' (uncommitted changes)' : ''}`}
              >
                {version.short || version.rev}
                {version.dirty && <span className="text-amber"> · dirty</span>}
              </a>
            </>
          )}
          {version && (version.rev === 'unknown' || version.rev === 'dev') && (
            <>
              <span className="text-cyan/30">·</span>
              <span className="font-mono normal-case tracking-normal text-amber">
                {version.rev}
              </span>
            </>
          )}
        </div>
        <div className="flex items-baseline gap-4">
          <span>Polling 5s</span>
          {vmsError && <span className="text-red">! {vmsError}</span>}
          <span className="hidden sm:inline">
            Auth via Authentik · Same-domain cookie
          </span>
        </div>
      </div>
    </footer>
  )
}
