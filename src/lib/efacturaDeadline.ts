/**
 * e-Factura 2026 transmission deadline.
 *
 * From 1 Jan 2026 a B2B/B2C invoice must be sent to RO e-Factura within **5 working
 * days** counted from the working day immediately AFTER the issue date (EU Reg.
 * 1182/71). Saturdays, Sundays and Romanian public holidays are excluded. This is an
 * informational aid only — ANAF enforces the real deadline.
 */

/** Orthodox (Gregorian) Easter via the Meeus Julian algorithm + 13-day offset (1900–2099). */
function orthodoxEaster(year: number): Date {
  const a = year % 4;
  const b = year % 7;
  const c = year % 19;
  const d = (19 * c + 15) % 30;
  const e = (2 * a + 4 * b - d + 34) % 7;
  const month = Math.floor((d + e + 114) / 31); // 3 = March, 4 = April (Julian)
  const day = ((d + e + 114) % 31) + 1;
  const date = new Date(Date.UTC(year, month - 1, day));
  date.setUTCDate(date.getUTCDate() + 13); // Julian → Gregorian
  return date;
}

function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setUTCDate(r.getUTCDate() + n);
  return r;
}

function iso(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/** Romanian public-holiday dates (YYYY-MM-DD) for a year (Codul Muncii art. 139). */
export function roPublicHolidays(year: number): Set<string> {
  const easter = orthodoxEaster(year);
  const fixed = [
    `${year}-01-01`,
    `${year}-01-02`, // Anul Nou
    `${year}-01-06`,
    `${year}-01-07`, // Bobotează, Sf. Ion
    `${year}-01-24`, // Unirea Principatelor
    `${year}-05-01`, // Ziua Muncii
    `${year}-06-01`, // Ziua Copilului
    `${year}-08-15`, // Adormirea Maicii Domnului
    `${year}-11-30`, // Sf. Andrei
    `${year}-12-01`, // Ziua Națională
    `${year}-12-25`,
    `${year}-12-26`, // Crăciun
  ];
  const movable = [
    iso(addDays(easter, -2)), // Vinerea Mare
    iso(easter), // Paște (duminică)
    iso(addDays(easter, 1)), // A doua zi de Paște
    iso(addDays(easter, 49)), // Rusalii (duminică)
    iso(addDays(easter, 50)), // A doua zi de Rusalii
  ];
  return new Set([...fixed, ...movable]);
}

function isWorkingDay(d: Date, holidays: Set<string>): boolean {
  const dow = d.getUTCDay(); // 0 = Sun, 6 = Sat
  if (dow === 0 || dow === 6) return false;
  return !holidays.has(iso(d));
}

/**
 * The e-Factura deadline: the 5th working day starting from the day AFTER `issueDate`
 * (YYYY-MM-DD). Returns null for an invalid date.
 */
export function efacturaDeadline(issueDate: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(issueDate.trim());
  if (!m) return null;
  const start = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3]));
  if (Number.isNaN(start.getTime())) return null;
  // A deadline can cross a year boundary, so include both years' holidays.
  const holidays = new Set<string>([
    ...roPublicHolidays(start.getUTCFullYear()),
    ...roPublicHolidays(start.getUTCFullYear() + 1),
  ]);
  let d = start;
  let counted = 0;
  while (counted < 5) {
    d = addDays(d, 1);
    if (isWorkingDay(d, holidays)) counted += 1;
  }
  return d;
}

/** Whole calendar days from today until `deadline` (negative = overdue). */
export function deadlineDaysLeft(deadline: Date, today: Date = new Date()): number {
  const t0 = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  const t1 = Date.UTC(
    deadline.getUTCFullYear(),
    deadline.getUTCMonth(),
    deadline.getUTCDate(),
  );
  return Math.round((t1 - t0) / 86_400_000);
}

/** Format a deadline Date as dd.mm.yyyy (RO). */
export function formatDeadline(d: Date): string {
  const dd = String(d.getUTCDate()).padStart(2, "0");
  const mm = String(d.getUTCMonth() + 1).padStart(2, "0");
  return `${dd}.${mm}.${d.getUTCFullYear()}`;
}
