/**
 * Icon component — Win-business flat stroke icons (SVG, 24×24 viewBox).
 *
 * Portat din Claude Design. Toate iconițele folosesc currentColor pentru
 * culoare. Aceleași nume ca în chrome.jsx / icons.jsx.
 */

import type { CSSProperties, JSX } from "react";

type IconName = keyof typeof PATHS;

interface IconProps {
  name: IconName | string;
  size?: number;
  className?: string;
  style?: CSSProperties;
}

export function Icon({ name, size = 16, className = "", style }: IconProps) {
  const path = (PATHS as Record<string, JSX.Element>)[name] ?? PATHS.box;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
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
  // chrome
  box: <rect x="4" y="4" width="16" height="16" />,
  search: <><circle cx="11" cy="11" r="6.5" /><path d="M20 20l-3.5-3.5" /></>,
  filter: <path d="M4 5h16l-6 8v6l-4-2v-4z" />,
  chevronDown: <path d="M6 9l6 6 6-6" />,
  chevronRight: <path d="M9 6l6 6-6 6" />,
  chevronLeft: <path d="M15 6l-6 6 6 6" />,
  caret: <path d="M5 9l5 5 5-5" />,
  x: <path d="M6 6l12 12M18 6L6 18" />,
  plus: <path d="M12 5v14M5 12h14" />,
  minus: <path d="M5 12h14" />,
  check: <path d="M5 12l5 5 9-11" />,
  alert: <><path d="M12 9v5" /><path d="M12 17.5v.5" /><path d="M3.5 19l8.5-15 8.5 15z" /></>,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v5" /><path d="M12 8.5v.5" /></>,
  warning: <><circle cx="12" cy="12" r="9" /><path d="M12 8v5" /><path d="M12 15.5v.5" /></>,
  cancel: <><circle cx="12" cy="12" r="9" /><path d="M8 8l8 8M16 8l-8 8" /></>,
  refresh: <><path d="M4 12a8 8 0 0114-5.3" /><path d="M20 12a8 8 0 01-14 5.3" /><path d="M18 4v3h-3M6 20v-3h3" /></>,
  download: <><path d="M12 4v12" /><path d="M7 11l5 5 5-5" /><path d="M5 20h14" /></>,
  upload: <><path d="M12 20V8" /><path d="M7 13l5-5 5 5" /><path d="M5 4h14" /></>,
  printer: <><rect x="6" y="3" width="12" height="6" /><rect x="6" y="14" width="12" height="7" /><path d="M3 9h18v6H3z" /><circle cx="17" cy="12" r={0.5} fill="currentColor" /></>,
  mail: <><rect x="3" y="5" width="18" height="14" /><path d="M3 6l9 7 9-7" /></>,
  more: <><circle cx="6" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="18" cy="12" r="1" fill="currentColor" stroke="none" /></>,
  pen: <><path d="M4 20h4l10-10-4-4L4 16z" /><path d="M14 6l4 4" /></>,
  trash: <><path d="M4 7h16" /><path d="M9 7V4h6v3" /><path d="M6 7l1 13h10l1-13" /></>,
  copy: <><rect x="8" y="8" width="12" height="12" /><path d="M16 8V4H4v12h4" /></>,
  link: <><path d="M10 14a4 4 0 010-5.6l3-3a4 4 0 015.6 5.6l-1.5 1.5" /><path d="M14 10a4 4 0 010 5.6l-3 3A4 4 0 015.4 13l1.5-1.5" /></>,
  external: <><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M10 6H5v13h13v-5" /></>,
  eye: <><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>,
  save: <><path d="M5 3h11l3 3v15H5z" /><rect x="8" y="3" width="8" height="6" /><rect x="8" y="13" width="8" height="6" /></>,
  history: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3.5 2" /></>,

  // menu hints
  file: <><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /></>,
  edit: <><path d="M4 20h4l10-10-4-4L4 16z" /></>,
  ops: <><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 00-.2-1.7l2-1.5-2-3.5-2.4.9a7 7 0 00-3-1.7L13 2h-4l-.4 2.5a7 7 0 00-3 1.7l-2.4-.9-2 3.5 2 1.5A7 7 0 003 12c0 .6.1 1.1.2 1.7l-2 1.5 2 3.5 2.4-.9a7 7 0 003 1.7L9 22h4l.4-2.5a7 7 0 003-1.7l2.4.9 2-3.5-2-1.5c.1-.6.2-1.1.2-1.7z" /></>,
  data: <><ellipse cx="12" cy="5" rx="8" ry="2.5" /><path d="M4 5v6c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5V5" /><path d="M4 11v6c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5v-6" /></>,
  reports: <><rect x="4" y="4" width="16" height="16" /><path d="M8 16V9M12 16v-5M16 16v-9" /></>,
  view: <><circle cx="12" cy="12" r="3" /><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" /></>,
  help: <><circle cx="12" cy="12" r="9" /><path d="M9.5 10a2.5 2.5 0 015 0c0 1.5-2.5 2-2.5 3.5" /><circle cx="12" cy="17" r={0.6} fill="currentColor" stroke="none" /></>,

  // modules / business actions
  invoice: <><rect x="5" y="3" width="14" height="18" /><path d="M8 8h8M8 12h8M8 16h5" /></>,
  invoiceIn: <><rect x="5" y="3" width="14" height="18" /><path d="M8 8h8M8 12h8" /><path d="M9 17l3 3 3-3" /></>,
  buildings: <><rect x="3" y="8" width="8" height="13" /><rect x="11" y="3" width="10" height="18" /><path d="M14 7h1M17 7h1M14 11h1M17 11h1M14 15h1M17 15h1M6 12h1M6 16h1" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 21c0-4.4 3.6-8 8-8s8 3.6 8 8" /></>,
  users: <><circle cx="9" cy="9" r="3.5" /><path d="M2 20c0-3.9 3.1-7 7-7s7 3.1 7 7" /><circle cx="17" cy="7" r="3" /><path d="M17 13c2.8 0 5 2.2 5 5" /></>,
  bank: <><path d="M3 9l9-5 9 5" /><path d="M5 9v9M9 9v9M15 9v9M19 9v9" /><path d="M3 20h18" /></>,
  stock: <><path d="M3 7l9-4 9 4-9 4z" /><path d="M3 12l9 4 9-4" /><path d="M3 17l9 4 9-4" /></>,
  settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 11-4 0v-.09a1.65 1.65 0 00-1-1.51 1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 11-2.83-2.83l.06-.06A1.65 1.65 0 005.5 15a1.65 1.65 0 00-1.51-1H4a2 2 0 110-4h.09A1.65 1.65 0 005.5 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 112.83-2.83l.06.06A1.65 1.65 0 009 4.6V4a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 112.83 2.83l-.06.06A1.65 1.65 0 0019.4 9c.14.34.34.65.6.92" /></>,

  // sync / anaf
  anaf: <><rect x="3" y="3" width="18" height="18" /><path d="M8 10l2 2 6-6" /><path d="M8 16h8" /></>,
  cloud: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18z" /></>,
  cloudUp: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18" /><path d="M9 14l3-3 3 3" /><path d="M12 11v8" /></>,
  cloudDn: <><path d="M6 18a4 4 0 010-8 5 5 0 019.6-1.8A4 4 0 0118 18" /><path d="M9 12l3 3 3-3" /><path d="M12 6v9" /></>,

  // misc
  keyboard: <><rect x="2" y="6" width="20" height="12" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M10 14h4" /></>,
  star: <path d="M12 3l2.7 5.5 6.1.9-4.4 4.3 1 6-5.4-2.8-5.4 2.8 1-6-4.4-4.3 6.1-.9z" />,
  bookmark: <path d="M6 3h12v18l-6-4-6 4z" />,
  bell: <><path d="M18 16V11a6 6 0 10-12 0v5l-2 3h16z" /><path d="M10 21a2 2 0 004 0" /></>,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>,
  calendar: <><rect x="3" y="5" width="18" height="16" /><path d="M3 9h18" /><path d="M8 3v4M16 3v4" /></>,
  tag: <><path d="M3 12V3h9l9 9-9 9z" /><circle cx="8" cy="8" r="1.2" /></>,
  receipt: <><path d="M5 3h14v18l-3-2-2 2-2-2-2 2-2-2-3 2z" /><path d="M8 8h8M8 12h8M8 16h4" /></>,
  arrowRight: <><path d="M5 12h14" /><path d="M13 5l7 7-7 7" /></>,
  arrowLeft: <><path d="M19 12H5" /><path d="M11 5l-7 7 7 7" /></>,
  sortAsc: <><path d="M6 9l4-4 4 4" /><path d="M10 5v14" /></>,
  sortDsc: <><path d="M6 15l4 4 4-4" /><path d="M10 5v14" /></>,
  command: <><path d="M9 6a3 3 0 11-3 3h12a3 3 0 11-3 3v-6a3 3 0 113 3H6a3 3 0 11 3-3z" /></>,
  database: <><ellipse cx="12" cy="6" rx="8" ry="3" /><path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6" /><path d="M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" /></>,
  scale: <><path d="M12 3v18" /><path d="M4 8l8-2 8 2" /><path d="M2 14a4 4 0 008 0L6 8z" /><path d="M14 14a4 4 0 008 0l-4-6z" /></>,
  split: <><path d="M4 5h7v14H4z" /><path d="M13 5h7v14h-7z" /></>,
  storno: <><circle cx="12" cy="12" r="9" /><path d="M5.6 5.6l12.8 12.8" /></>,
  draft: <><path d="M4 20h4l10-10-4-4L4 16z" /><path d="M14 6l4 4" /><path d="M3 21l1-3" /></>,
} as const;
