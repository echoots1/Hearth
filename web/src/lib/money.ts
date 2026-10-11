/** Money is integer cents everywhere in the API. These are the only formatters. */

const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" });
const usdWhole = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  maximumFractionDigits: 0,
});
const plain = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** `$1,234.56`, with a leading `−` (true minus) for negatives. */
export function money(cents: number | null | undefined): string {
  if (cents == null) return "—";
  const s = usd.format(Math.abs(cents) / 100);
  return cents < 0 ? `−${s}` : s;
}

/** `$1,235`, rounded to whole dollars; `−` for negatives. */
export function dollars(cents: number | null | undefined): string {
  if (cents == null) return "—";
  const s = usdWhole.format(Math.abs(cents) / 100);
  return cents < 0 ? `−${s}` : s;
}

/** `1,235` (no sign, no currency) for dense list columns. */
export function whole(cents: number): string {
  return plain.format(Math.abs(cents) / 100);
}

/** Signed with an explicit `+` for income: `+$3,200.00` / `−$48.11`. */
export function signed(cents: number): string {
  const s = usd.format(Math.abs(cents) / 100);
  return cents < 0 ? `−${s}` : `+${s}`;
}

/** Parse what a person typed ("1,234.5", "$80", "-12") into cents, or null. */
export function parseMoney(text: string): number | null {
  const t = text.replace(/[$,\s]/g, "").replace("−", "-");
  if (!t) return null;
  const n = Number(t);
  if (!Number.isFinite(n)) return null;
  return Math.round(n * 100);
}

/** Cents → the decimal string an input shows ("80.00"). */
export function toInput(cents: number | null | undefined): string {
  if (cents == null) return "";
  return (cents / 100).toFixed(2);
}

export function pct(bps: number): number {
  return Math.round(bps / 100);
}

/** APR in basis points → "24.99%". */
export function apr(bps: number): string {
  return `${(bps / 100).toFixed(2).replace(/\.?0+$/, "")}%`;
}
