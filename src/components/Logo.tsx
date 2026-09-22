/**
 * The wave sweeping away what a program left behind.
 *
 * Inline rather than an `<img>`: it inherits `currentColor`, so the mark is
 * the same blue as the rest of the interface without a second copy of the
 * palette living in an SVG file. The same drawing is `public/logo.svg` for the
 * window icon and `site/assets/img/logo.svg` for the website.
 */
export function Logo({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 200 200"
      className={className}
      role="img"
      aria-label="Wave Uninstaller"
      fill="none"
      stroke="currentColor"
      strokeLinejoin="round"
      strokeLinecap="round"
    >
      {/* the crest */}
      <path d="M18 108c20-30 40-30 60 0s40 30 60 0 30-22 44-8" strokeWidth="14" />
      {/* the trough below it, lighter, so it reads as water rather than a squiggle */}
      <path d="M18 146c20-30 40-30 60 0s40 30 60 0 30-22 44-8" strokeWidth="8" opacity="0.45" />
      {/* what is being swept away */}
      <path d="M62 60h76" strokeWidth="10" />
      <path d="M74 60V44a8 8 0 0 1 8-8h36a8 8 0 0 1 8 8v16" strokeWidth="10" />
    </svg>
  );
}
