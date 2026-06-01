import logoUrl from "@/assets/clarito-logo.png";

/**
 * Clarito brand mark (app icon). Renders the logo PNG at the requested size.
 * The asset is a squircle with transparent corners, so it sits cleanly on any
 * surface without an extra background container.
 */
export function BrandMark({
  size = 28,
  className,
}: {
  size?: number;
  className?: string;
}) {
  return (
    <img
      src={logoUrl}
      alt="Clarito"
      width={size}
      height={size}
      draggable={false}
      className={className}
      style={{ display: "block", flexShrink: 0, userSelect: "none" }}
    />
  );
}
