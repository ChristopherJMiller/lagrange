/**
 * Layered background:
 *  - Deep base color
 *  - Faint dotted grid (instrument paper)
 *  - Soft radial blooms (cyan & amber, off-center)
 *  - CRT vignette
 *  - Subtle grain
 *
 * All purely decorative; pointer-events-none. Fixed position so it
 * persists across scroll on tall mobile viewports.
 */
export function Background() {
  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 z-0 overflow-hidden">
      {/* base */}
      <div className="absolute inset-0 bg-bg" />

      {/* dotted grid */}
      <div className="absolute inset-0 grid-dots opacity-60" />

      {/* radial blooms */}
      <div
        className="absolute -top-40 -left-40 h-[60vh] w-[60vh] rounded-full"
        style={{
          background:
            'radial-gradient(circle, rgba(86, 211, 255, 0.06) 0%, transparent 60%)',
        }}
      />
      <div
        className="absolute -bottom-32 -right-40 h-[55vh] w-[55vh] rounded-full"
        style={{
          background:
            'radial-gradient(circle, rgba(255, 180, 84, 0.05) 0%, transparent 60%)',
        }}
      />

      {/* orbital ring — subtle decorative SVG, off-center */}
      <svg
        className="absolute -right-20 top-1/3 h-[70vh] w-[70vh] opacity-[0.07]"
        viewBox="0 0 400 400"
        fill="none"
      >
        <ellipse
          cx="200"
          cy="200"
          rx="180"
          ry="60"
          stroke="#56d3ff"
          strokeWidth="0.5"
          strokeDasharray="2 4"
        />
        <ellipse
          cx="200"
          cy="200"
          rx="60"
          ry="180"
          stroke="#ffb454"
          strokeWidth="0.5"
          strokeDasharray="2 4"
          transform="rotate(28 200 200)"
        />
        <circle cx="200" cy="200" r="3" fill="#ffb454" />
      </svg>

      {/* vignette */}
      <div className="absolute inset-0 vignette" />

      {/* grain */}
      <div className="absolute inset-0 grain opacity-[0.07] mix-blend-overlay" />
    </div>
  )
}
