type Props = { className?: string }

/** The vsplit mark: three stacked horizontal bars of varying widths (a clip
 *  timeline) crossed by a faint dashed vertical line — the "cut" a splitter
 *  would make. Reads as "split a timeline" at a glance.
 *
 *  Reused at any size: the SVG uses a viewBox, so callers control the rendered
 *  size via className (e.g. `size-7`). */
export function Logo({ className }: Props) {
  return (
    <svg
      viewBox="0 0 32 32"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      role="img"
      aria-label="vsplit logo"
    >
      <defs>
        <linearGradient id="vsplit-logo-bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#60a5fa" />
          <stop offset="1" stopColor="#1e40af" />
        </linearGradient>
      </defs>
      <rect width="32" height="32" rx="7" fill="url(#vsplit-logo-bg)" />
      <g fill="#ffffff">
        {/* Three rows of varying widths — a stylised clip timeline. */}
        <rect x="7"  y="8"  width="18" height="4" rx="1.5" />
        <rect x="7"  y="14" width="11" height="4" rx="1.5" />
        <rect x="7"  y="20" width="15" height="4" rx="1.5" />
      </g>
      {/* Dashed vertical line crossing the bars — the "cut" mark. */}
      <line
        x1="14.5" y1="6" x2="14.5" y2="26"
        stroke="#ffffff" strokeOpacity="0.45"
        strokeWidth="0.75" strokeDasharray="1.5 1.5"
      />
    </svg>
  )
}
