/** Dates are ISO `YYYY-MM-DD` strings in the household timezone; months are `YYYY-MM`. */

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];
const MONTHS_SHORT = MONTHS.map((m) => m.slice(0, 3));

export function parseDate(iso: string): Date {
  const [y, m, d] = iso.slice(0, 10).split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** "Oct 12" */
export function shortDate(iso: string): string {
  const d = parseDate(iso);
  return `${MONTHS_SHORT[d.getMonth()]} ${d.getDate()}`;
}

/** "Oct 12, 2026" */
export function longDate(iso: string): string {
  const d = parseDate(iso);
  return `${MONTHS_SHORT[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`;
}

/** "October 2026" from "2026-10" */
export function monthLabel(month: string): string {
  const [y, m] = month.split("-").map(Number);
  return `${MONTHS[m - 1]} ${y}`;
}

/** "Oct 2026" */
export function monthShort(month: string): string {
  const [y, m] = month.split("-").map(Number);
  return `${MONTHS_SHORT[m - 1]} ${y}`;
}

export function addMonths(month: string, n: number): string {
  const [y, m] = month.split("-").map(Number);
  const d = new Date(y, m - 1 + n, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

export function monthOf(iso: string): string {
  return iso.slice(0, 7);
}

export function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function addDays(iso: string, n: number): string {
  const d = parseDate(iso);
  d.setDate(d.getDate() + n);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** "1st", "12th" */
export function ordinal(n: number): string {
  const s = ["th", "st", "nd", "rd"];
  const v = n % 100;
  return `${n}${s[(v - 20) % 10] ?? s[v] ?? s[0]}`;
}

/** Relative age of a timestamp: "4h ago", "2 days ago", "just now". */
export function ago(ts: string | null | undefined, now = Date.now()): string {
  if (!ts) return "never";
  const ms = now - new Date(ts).getTime();
  const min = Math.round(ms / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min}m ago`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d === 1) return "yesterday";
  if (d < 30) return `${d} days ago`;
  const mo = Math.round(d / 30);
  return mo === 1 ? "a month ago" : `${mo} months ago`;
}

/** Days from today to an ISO date (negative = past). */
export function daysUntil(iso: string, today = todayIso()): number {
  return Math.round((parseDate(iso).getTime() - parseDate(today).getTime()) / 86_400_000);
}

export function dueWord(iso: string, today = todayIso()): string {
  const n = daysUntil(iso, today);
  if (n === 0) return "today";
  if (n === 1) return "tomorrow";
  if (n < 0) return `${-n} day${n === -1 ? "" : "s"} overdue`;
  return `in ${n} days`;
}
