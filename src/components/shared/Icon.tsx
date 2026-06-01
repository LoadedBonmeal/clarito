/**
 * Icon component — stroke icons (SVG, 24×24 viewBox).
 *
 * Contains all legacy real-app icons PLUS all prototype RF_ICON_PATHS icons.
 * Aliases ensure that both legacy names and prototype names resolve correctly.
 */

import type { CSSProperties, JSX } from "react";

type IconName = keyof typeof PATHS;

interface IconProps {
  name: IconName | string;
  size?: number;
  className?: string;
  style?: CSSProperties;
  stroke?: number;
}

export function Icon({ name, size = 16, className = "", style, stroke = 1.6 }: IconProps) {
  const path = (PATHS as Record<string, JSX.Element>)[name] ?? PATHS.box;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={style}
    >
      {path}
    </svg>
  );
}

const PATHS = {
  // ── Fallback ──────────────────────────────────────────────────────────────
  box: <path d="M21 8 12 3 3 8v8l9 5 9-5z M3 8l9 5 9-5 M12 21V13" />,

  // ── Navigation / UI chrome ────────────────────────────────────────────────
  search: <><circle cx="11" cy="11" r="6.5" /><path d="M20 20l-3.5-3.5" /></>,
  filter: <path d="M3 4h18l-7 8v6l-4 2v-8z" />,
  chevronDown: <path d="M6 9l6 6 6-6" />,
  chevronRight: <path d="M9 6l6 6-6 6" />,
  chevronLeft: <path d="M15 6l-6 6 6 6" />,
  /** alias for prototype chevDown */
  chevDown: <path d="M6 9l6 6 6-6" />,
  /** alias for prototype chevRight */
  chevRight: <path d="M9 6l6 6-6 6" />,
  /** alias for prototype chevLeft */
  chevLeft: <path d="M15 6l-6 6 6 6" />,
  caret: <path d="M5 9l5 5 5-5" />,
  x: <path d="M6 6l12 12M18 6L6 18" />,
  plus: <path d="M12 5v14M5 12h14" />,
  minus: <path d="M5 12h14" />,
  check: <path d="M20 6 9 17l-5-5" />,
  alert: <><path d="m10.29 3.86-8.18 14a1.5 1.5 0 0 0 1.29 2.25h16.4a1.5 1.5 0 0 0 1.29-2.25l-8.18-14a1.5 1.5 0 0 0-2.62 0z" /><path d="M12 9v4M12 17h.01" /></>,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 16v-4M12 8h.01" /></>,
  warning: <><circle cx="12" cy="12" r="9" /><path d="M12 8v5" /><path d="M12 15.5v.5" /></>,
  cancel: <><circle cx="12" cy="12" r="9" /><path d="M8 8l8 8M16 8l-8 8" /></>,
  xCircle: <><circle cx="12" cy="12" r="9" /><path d="m15 9-6 6M9 9l6 6" /></>,
  checkCircle: <><circle cx="12" cy="12" r="9" /><path d="m8.5 12 2.5 2.5 5-5" /></>,
  refresh: <><path d="M3 12a9 9 0 0 1 15-6.7L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15 6.7L3 16" /><path d="M3 21v-5h5" /></>,
  download: <><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" /></>,
  upload: <><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M17 8l-5-5-5 5" /><path d="M12 3v12" /></>,
  printer: <><rect x="6" y="3" width="12" height="6" /><rect x="6" y="14" width="12" height="7" /><path d="M3 9h18v6H3z" /><circle cx="17" cy="12" r={0.5} fill="currentColor" /></>,
  more: <><circle cx="6" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="18" cy="12" r="1" fill="currentColor" stroke="none" /></>,
  /** alias for prototype 'dots' (vertical dots) */
  dots: <><circle cx="12" cy="5" r="1.4" /><circle cx="12" cy="12" r="1.4" /><circle cx="12" cy="19" r="1.4" /></>,
  pen: <><path d="M4 20h4l10-10-4-4L4 16z" /><path d="M14 6l4 4" /></>,
  copy: <><rect x="8" y="8" width="12" height="12" /><path d="M16 8V4H4v12h4" /></>,
  external: <><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M10 6H5v13h13v-5" /></>,
  save: <><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" /><path d="M17 21v-8H7v8M7 3v5h8" /></>,
  history: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3.5 2" /></>,
  arrowUp: <><path d="M12 19V5" /><path d="M5 12l7-7 7 7" /></>,
  arrowDown: <><path d="M12 5v14" /><path d="M5 12l7 7 7-7" /></>,
  arrowRight: <><path d="M5 12h14" /><path d="M13 5l7 7-7 7" /></>,
  arrowLeft: <><path d="M19 12H5" /><path d="M11 5l-7 7 7 7" /></>,
  sortAsc: <><path d="M6 9l4-4 4 4" /><path d="M10 5v14" /></>,
  sortDsc: <><path d="M6 15l4 4 4-4" /><path d="M10 5v14" /></>,
  eye: <><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>,
  link: <><path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1.5 1.5" /><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1.5-1.5" /></>,

  // ── Files / documents ─────────────────────────────────────────────────────
  file: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /></>,
  fileOut: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /><path d="M12 17V11" /><path d="m9 14 3-3 3 3" /></>,
  fileIn: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /><path d="M12 11v6" /><path d="m9 14 3 3 3-3" /></>,
  pdf: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /><path d="M8 13h1.5a1.5 1.5 0 0 1 0 3H8zM8 13v5" /></>,
  xml: <><path d="m8 16-3-4 3-4M16 8l3 4-3 4M13.5 7l-3 10" /></>,
  declaration: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /><path d="M9 13h6M9 17h4" /></>,
  receipt: <><path d="M5 21V4a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v17l-3-2-3 2-3-2-3 2-2-2z" /><path d="M9 7h6M9 11h6" /></>,
  folder: <><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></>,
  reverse: <><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M5 21V5a2 2 0 0 1 2-2h7l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" /><path d="M9 14h6" /><path d="m12 11-3 3 3 3" /></>,

  // ── Business / modules ────────────────────────────────────────────────────
  invoice: <><rect x="5" y="3" width="14" height="18" /><path d="M8 8h8M8 12h8M8 16h5" /></>,
  invoiceIn: <><rect x="5" y="3" width="14" height="18" /><path d="M8 8h8M8 12h8" /><path d="M9 17l3 3 3-3" /></>,
  building: <><rect x="4" y="3" width="16" height="18" rx="1.5" /><path d="M9 7h2M13 7h2M9 11h2M13 11h2M9 15h2M13 15h2" /></>,
  buildings: <><rect x="3" y="8" width="8" height="13" /><rect x="11" y="3" width="10" height="18" /><path d="M14 7h1M17 7h1M14 11h1M17 11h1M14 15h1M17 15h1M6 12h1M6 16h1" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 21c0-4.4 3.6-8 8-8s8 3.6 8 8" /></>,
  users: <><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" /><circle cx="9" cy="7" r="4" /><path d="M22 21v-2a4 4 0 0 0-3-3.87" /><path d="M16 3.13a4 4 0 0 1 0 7.75" /></>,
  bank: <><path d="m3 9 9-6 9 6" /><path d="M4 9h16v2H4z" /><path d="M6 11v7M10 11v7M14 11v7M18 11v7" /><path d="M3 21h18" /></>,
  wallet: <><path d="M19 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-2" /><path d="M21 9v6h-5a3 3 0 0 1 0-6z" /></>,
  repeat: <><path d="m17 2 4 4-4 4" /><path d="M3 11V9a4 4 0 0 1 4-4h14" /><path d="m7 22-4-4 4-4" /><path d="M21 13v2a4 4 0 0 1-4 4H3" /></>,
  stock: <><path d="M3 7l9-4 9 4-9 4z" /><path d="M3 12l9 4 9-4" /><path d="M3 17l9 4 9-4" /></>,
  /** alias for 'stock' (prototype uses 'box') */
  dashboard: <><rect x="3" y="3" width="7" height="9" rx="1" /><rect x="14" y="3" width="7" height="5" rx="1" /><rect x="14" y="12" width="7" height="9" rx="1" /><rect x="3" y="16" width="7" height="5" rx="1" /></>,
  ledger: <><path d="M4 4h16v16H4z" /><path d="M9 4v16M4 9h5M4 14h5" /></>,
  chart: <><path d="M3 3v18h18" /><rect x="7" y="11" width="3" height="6" /><rect x="12" y="7" width="3" height="10" /><rect x="17" y="13" width="3" height="4" /></>,
  reports: <><rect x="4" y="4" width="16" height="16" /><path d="M8 16V9M12 16v-5M16 16v-9" /></>,

  // ── Actions ───────────────────────────────────────────────────────────────
  send: <><path d="m22 2-7 20-4-9-9-4z" /><path d="M22 2 11 13" /></>,
  edit: <><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4z" /></>,
  trash: <><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" /></>,
  storno: <><path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-1" /></>,
  archive: <><rect x="3" y="4" width="18" height="4" rx="1" /><path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4" /></>,
  draft: <><path d="M4 20h4l10-10-4-4L4 16z" /><path d="M14 6l4 4" /><path d="M3 21l1-3" /></>,

  // ── Settings / system ─────────────────────────────────────────────────────
  settings: <><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" /></>,
  ops: <><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 00-.2-1.7l2-1.5-2-3.5-2.4.9a7 7 0 00-3-1.7L13 2h-4l-.4 2.5a7 7 0 00-3 1.7l-2.4-.9-2 3.5 2 1.5A7 7 0 003 12c0 .6.1 1.1.2 1.7l-2 1.5 2 3.5 2.4-.9a7 7 0 003 1.7L9 22h4l.4-2.5a7 7 0 003-1.7l2.4.9 2-3.5-2-1.5c.1-.6.2-1.1.2-1.7z" /></>,
  shield: <><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /><path d="m9 12 2 2 4-4" /></>,
  moon: <path d="M12 3a6.4 6.4 0 0 0 9 9 9 9 0 1 1-9-9z" />,
  sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" /></>,
  globe: <><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18z" /></>,
  keyboard: <><rect x="2" y="6" width="20" height="12" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" /></>,

  // ── Status / indicators ───────────────────────────────────────────────────
  bell: <><path d="M18 8a6 6 0 1 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" /><path d="M13.7 21a2 2 0 0 1-3.4 0" /></>,
  dot: <circle cx="12" cy="12" r="4" />,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>,
  calendar: <><rect x="3" y="4" width="18" height="18" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /></>,
  star: <path d="M12 3l2.7 5.5 6.1.9-4.4 4.3 1 6-5.4-2.8-5.4 2.8 1-6-4.4-4.3 6.1-.9z" />,
  bookmark: <path d="M6 3h12v18l-6-4-6 4z" />,
  tag: <><path d="M3 12V3h9l9 9-9 9z" /><circle cx="8" cy="8" r="1.2" /></>,
  list: <><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></>,
  table: <><rect x="3" y="4" width="18" height="16" rx="1.5" /><path d="M3 10h18M3 15h18M9 4v16M15 4v16" /></>,

  // ── Sync / ANAF ───────────────────────────────────────────────────────────
  anaf: <><rect x="3" y="3" width="18" height="18" /><path d="M8 10l2 2 6-6" /><path d="M8 16h8" /></>,
  cloud: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18z" /></>,
  cloudUp: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18" /><path d="M9 14l3-3 3 3" /><path d="M12 11v8" /></>,
  cloudDn: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18" /><path d="M9 12l3 3 3-3" /><path d="M12 6v9" /></>,

  // ── Finance specific ─────────────────────────────────────────────────────
  bnr: <><circle cx="12" cy="12" r="9" /><path d="M12 7v10M9.5 9h3.5a1.5 1.5 0 0 1 0 3H9.5M9.5 12h3a1.5 1.5 0 0 1 0 3H9.5" /></>,

  // ── Misc ─────────────────────────────────────────────────────────────────
  help: <><circle cx="12" cy="12" r="9" /><path d="M9.5 9a2.5 2.5 0 0 1 4.5 1.5c0 1.5-2 2-2 3M12 17h.01" /></>,
  view: <><circle cx="12" cy="12" r="3" /><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" /></>,
  data: <><ellipse cx="12" cy="5" rx="8" ry="2.5" /><path d="M4 5v6c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5V5" /><path d="M4 11v6c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5v-6" /></>,
  database: <><ellipse cx="12" cy="6" rx="8" ry="3" /><path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6" /><path d="M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" /></>,
  scale: <><path d="M12 3v18" /><path d="M4 8l8-2 8 2" /><path d="M2 14a4 4 0 008 0L6 8z" /><path d="M14 14a4 4 0 008 0l-4-6z" /></>,
  split: <><path d="M4 5h7v14H4z" /><path d="M13 5h7v14h-7z" /></>,
  command: <><path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3" /></>,
  mail: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m3 7 9 6 9-6" /></>,

  // ── Missing icons (icon sweep Fix-Wave C) ─────────────────────────────────
  alertTriangle: <><path d="m10.29 3.86-8.18 14a1.5 1.5 0 0 0 1.29 2.25h16.4a1.5 1.5 0 0 0 1.29-2.25l-8.18-14a1.5 1.5 0 0 0-2.62 0z" /><path d="M12 9v4M12 17h.01" /></>,
  book: <><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" /></>,
  code: <><path d="m16 18 6-6-6-6" /><path d="m8 6-6 6 6 6" /></>,
  key: <><circle cx="7.5" cy="15.5" r="5.5" /><path d="m21 2-9.6 9.6" /><path d="m15.5 7.5 3 3L22 7l-3-3" /></>,
  map: <><path d="M3 7l6-3 6 3 6-3v13l-6 3-6-3-6 3z" /><path d="M9 4v13M15 7v13" /></>,
  package: <><path d="m12.89 1.45 8 4A2 2 0 0 1 22 7.24v9.53a2 2 0 0 1-1.11 1.79l-8 4a2 2 0 0 1-1.79 0l-8-4A2 2 0 0 1 2 16.77V7.24a2 2 0 0 1 1.11-1.79l8-4a2 2 0 0 1 1.78 0z" /><path d="M2.32 6.16 12 11l9.68-4.84M12 22.76V11" /></>,
  percent: <><path d="m19 5-14 14" /><circle cx="6.5" cy="6.5" r="2.5" /><circle cx="17.5" cy="17.5" r="2.5" /></>,
} as const;
