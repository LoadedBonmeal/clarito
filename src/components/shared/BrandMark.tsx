/**
 * Clarito brand mark — monochrome (design clarito-shell.css `.mark`):
 * a white "C" arc + center dot on a near-black rounded square. Pure inline SVG
 * so it inherits the theme tokens (no raster asset, crisp at any size).
 */
export function BrandMark({
  size = 26,
  className,
}: {
  size?: number;
  className?: string;
}) {
  const glyph = Math.round(size * 0.7);
  return (
    <span
      className={className}
      aria-label="Clarito"
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.27),
        background: "var(--rf-accent)",
        display: "grid",
        placeItems: "center",
        flexShrink: 0,
        userSelect: "none",
      }}
    >
      <svg width={glyph} height={glyph} viewBox="0 0 32 32" fill="none" style={{ display: "block" }}>
        <path
          d="M23 9.4A9 9 0 1 0 23 22.6"
          stroke="var(--rf-text-on-accent)"
          strokeWidth="2.7"
          strokeLinecap="round"
        />
        <circle cx="16" cy="16" r="2.9" fill="var(--rf-text-on-accent)" />
      </svg>
    </span>
  );
}
