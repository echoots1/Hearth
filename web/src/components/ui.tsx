/** Small shared pieces of the Hearth look. */
import { useEffect, useId, useState, type ReactNode } from "react";
import { Link } from "react-router";
import { parseMoney, toInput } from "../lib/money";

export function Card({ children, className = "", tint }: { children: ReactNode; className?: string; tint?: "critical" | "question" }) {
  const extra =
    tint === "critical" ? " outline-attention outline-2 -outline-offset-2" : tint === "question" ? " bg-tint!" : "";
  return <div className={`card${extra} ${className}`}>{children}</div>;
}

export function CardHead({ title, right }: { title: ReactNode; right?: ReactNode }) {
  return (
    <div className="card-head">
      <span className="text-[13px] font-extrabold">{title}</span>
      {right !== undefined && <span className="text-muted text-[12px] font-bold">{right}</span>}
    </div>
  );
}

export function SectionTitle({ title, right }: { title: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between px-1 pt-1">
      <span className="text-[13px] font-extrabold">{title}</span>
      {right && <span className="text-muted text-[12px] font-bold">{right}</span>}
    </div>
  );
}

export function Progress({ bps, color = "ink", marker, thick }: { bps: number; color?: "ink" | "attention" | "income"; marker?: number; thick?: boolean }) {
  const w = Math.max(0, Math.min(100, bps / 100));
  const bg = color === "attention" ? "bg-attention" : color === "income" ? "bg-income" : "bg-ink";
  return (
    <div className={`bg-line relative overflow-hidden rounded-full ${thick ? "h-2.5" : "h-1.5"}`}>
      <div className={`h-full rounded-full ${bg}`} style={{ width: `${w}%` }} />
      {marker !== undefined && (
        <div className="bg-attention absolute top-0 h-full w-0.5" style={{ left: `${Math.min(100, marker)}%` }} aria-hidden />
      )}
    </div>
  );
}

export function Spinner({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="text-muted flex items-center justify-center gap-2 py-10 text-[13px] font-semibold" role="status">
      <span className="border-line border-t-ink inline-block h-4 w-4 animate-spin rounded-full border-2" />
      {label}
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="text-muted px-4 py-8 text-center text-[13px] font-semibold">{children}</div>;
}

export function ErrorNote({ error, onClose }: { error: string | null | undefined; onClose?: () => void }) {
  if (!error) return null;
  return (
    <div className="bg-tint text-tint-ink border-tint-line flex items-start justify-between gap-3 rounded-xl border px-3 py-2 text-[13px] font-semibold" role="alert">
      <span>{error}</span>
      {onClose && (
        <button type="button" onClick={onClose} aria-label="Dismiss" className="font-extrabold">
          ×
        </button>
      )}
    </div>
  );
}

export function Pill({ children, tone = "light", to, onClick }: { children: ReactNode; tone?: "light" | "attention" | "dark"; to?: string; onClick?: () => void }) {
  const cls =
    "inline-flex h-10 items-center gap-2 rounded-full px-3.5 text-[13px] font-bold " +
    (tone === "attention" ? "bg-attention text-white" : tone === "dark" ? "bg-ink text-ground" : "bg-card text-ink shadow-[var(--shadow-card)]");
  if (to) return <Link to={to} className={cls}>{children}</Link>;
  return (
    <button type="button" onClick={onClick} className={cls}>
      {children}
    </button>
  );
}

export function Tabs<T extends string>({ value, onChange, items }: { value: T; onChange: (v: T) => void; items: { key: T; label: string }[] }) {
  return (
    <div className="bg-line flex gap-1.5 rounded-2xl p-1" role="tablist">
      {items.map((it) => (
        <button
          key={it.key}
          type="button"
          role="tab"
          aria-selected={value === it.key}
          onClick={() => onChange(it.key)}
          className={`h-10 flex-1 rounded-xl text-[13px] font-bold ${value === it.key ? "bg-card text-ink font-extrabold shadow-[var(--shadow-card)]" : "text-muted"}`}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

/** A bottom sheet for forms. Closes on backdrop tap or Escape. */
export function Sheet({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: ReactNode }) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = "";
    };
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-end justify-center sm:items-center" role="dialog" aria-modal="true" aria-label={title}>
      <button type="button" aria-label="Close" onClick={onClose} className="bg-ink/40 absolute inset-0" />
      <div className="bg-ground relative max-h-[92vh] w-full max-w-md overflow-y-auto rounded-t-[28px] px-4 pt-3 pb-[max(1rem,env(safe-area-inset-bottom))] sm:rounded-[28px]">
        <div className="bg-line mx-auto mb-3 h-1 w-10 rounded-full" aria-hidden />
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[17px] font-extrabold">{title}</h2>
          <button type="button" onClick={onClose} className="icon-btn" aria-label="Close">
            <Icon name="x" />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-muted text-[12px] font-bold">{label}</span>
      {children}
      {hint && <span className="text-faint text-[11px] font-semibold">{hint}</span>}
    </label>
  );
}

/** Dollar input that reports cents. */
export function MoneyInput({ value, onChange, placeholder = "0.00", autoFocus, allowNegative }: { value: number | null; onChange: (cents: number | null) => void; placeholder?: string; autoFocus?: boolean; allowNegative?: boolean }) {
  const [text, setText] = useState(toInput(value));
  useEffect(() => {
    // keep in sync when the parent resets the value
    if (parseMoney(text) !== value) setText(toInput(value));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
  return (
    <div className="relative">
      <span className="text-muted pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-[15px] font-bold">$</span>
      <input
        className="field pl-7"
        inputMode="decimal"
        autoFocus={autoFocus}
        placeholder={placeholder}
        value={text}
        onChange={(e) => {
          const t = e.target.value;
          setText(t);
          const c = parseMoney(t);
          onChange(c === null ? null : allowNegative ? c : Math.abs(c));
        }}
      />
    </div>
  );
}

export function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-3">
      <label htmlFor={id} className="flex flex-col">
        <span className="text-[15px] font-bold">{label}</span>
        {hint && <span className="text-muted text-[12px] font-semibold">{hint}</span>}
      </label>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative h-7 w-12 shrink-0 rounded-full transition ${checked ? "bg-ink" : "bg-line"}`}
      >
        <span className={`bg-card absolute top-0.5 h-6 w-6 rounded-full shadow transition ${checked ? "left-[22px]" : "left-0.5"}`} />
      </button>
    </div>
  );
}

export function Segmented<T extends string>({ value, onChange, items, small, grow }: { value: T; onChange: (v: T) => void; items: { key: T; label: string }[]; small?: boolean; grow?: boolean }) {
  return (
    <div className={`bg-line ${grow ? "flex" : "inline-flex"} gap-1 rounded-xl p-0.5`} role="radiogroup">
      {items.map((it) => (
        <button
          key={it.key}
          type="button"
          role="radio"
          aria-checked={value === it.key}
          onClick={() => onChange(it.key)}
          className={`rounded-[10px] px-3 font-bold ${grow ? "flex-1" : ""} ${small ? "h-8 text-[12px]" : "h-9 text-[13px]"} ${value === it.key ? "bg-card text-ink shadow-[var(--shadow-card)]" : "text-muted"}`}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

export type IconName =
  | "home" | "list" | "calendar" | "target" | "more" | "back" | "forward" | "x" | "check" | "alert"
  | "question" | "edit" | "settings" | "plus" | "chevron" | "flame" | "undo" | "bell" | "key" | "users" | "rule" | "bank" | "plug" | "clock" | "sparkle" | "logout";

export function Icon({ name, size = 20, className = "" }: { name: IconName; size?: number; className?: string }) {
  const p: Record<IconName, ReactNode> = {
    home: <><path d="M3 12l9-8 9 8" /><path d="M5 10v10h14V10" /></>,
    list: <><path d="M4 6h16" /><path d="M4 12h16" /><path d="M4 18h10" /></>,
    calendar: <><rect x="3" y="4" width="18" height="17" rx="2" /><path d="M8 2v4" /><path d="M16 2v4" /><path d="M3 10h18" /></>,
    target: <><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="4.5" /><circle cx="12" cy="12" r="1" /></>,
    more: <><circle cx="5" cy="12" r="1.5" /><circle cx="12" cy="12" r="1.5" /><circle cx="19" cy="12" r="1.5" /></>,
    back: <path d="M15 18l-6-6 6-6" />,
    forward: <path d="M9 6l6 6-6 6" />,
    chevron: <path d="M9 6l6 6-6 6" />,
    x: <><path d="M18 6L6 18" /><path d="M6 6l12 12" /></>,
    check: <path d="M5 12l5 5L20 7" />,
    alert: <><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></>,
    question: <><circle cx="12" cy="12" r="10" /><path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></>,
    edit: <><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></>,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></>,
    plus: <><path d="M12 5v14" /><path d="M5 12h14" /></>,
    flame: <path d="M12 3c2.5 3.5 6 6 6 10.5a6 6 0 0 1-12 0c0-2.5 1.2-4.2 2.5-6 0 2.5.8 3.5 1.7 3.5 0-2.5.4-5 1.8-8z" />,
    undo: <><path d="M3 7v6h6" /><path d="M21 17a9 9 0 0 0-15-6.7L3 13" /></>,
    bell: <><path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" /><path d="M13.7 21a2 2 0 0 1-3.4 0" /></>,
    key: <><circle cx="8" cy="15" r="4" /><path d="M10.9 12.1 20 3" /><path d="M16 7l3 3" /></>,
    users: <><circle cx="9" cy="8" r="4" /><path d="M2 21a7 7 0 0 1 14 0" /><path d="M16 11a4 4 0 1 0 0-8" /><path d="M22 21a7 7 0 0 0-5-6.7" /></>,
    rule: <><path d="M4 7h16" /><path d="M4 12h10" /><path d="M4 17h7" /><path d="M17 15l2 2 4-4" /></>,
    bank: <><path d="M3 10l9-6 9 6" /><path d="M5 10v8" /><path d="M9 10v8" /><path d="M15 10v8" /><path d="M19 10v8" /><path d="M3 20h18" /></>,
    plug: <><path d="M9 2v6" /><path d="M15 2v6" /><path d="M6 8h12v4a6 6 0 0 1-12 0z" /><path d="M12 18v4" /></>,
    clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>,
    sparkle: <><path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" /><path d="M19 17l.8 2.2L22 20l-2.2.8L19 23l-.8-2.2L16 20l2.2-.8z" /></>,
    logout: <><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" /><path d="M16 17l5-5-5-5" /><path d="M21 12H9" /></>,
  };
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={className}>
      {p[name]}
    </svg>
  );
}

export function PageHeader({ title, subtitle, back, right }: { title: ReactNode; subtitle?: ReactNode; back?: string | (() => void); right?: ReactNode }) {
  return (
    <div className="flex h-11 items-center justify-between">
      {back ? (
        typeof back === "string" ? (
          <Link to={back} className="icon-btn" aria-label="Back">
            <Icon name="back" />
          </Link>
        ) : (
          <button type="button" onClick={back} className="icon-btn" aria-label="Back">
            <Icon name="back" />
          </button>
        )
      ) : (
        <div className="w-11" />
      )}
      <div className="flex flex-col items-center leading-tight">
        <span className="text-[17px] font-extrabold">{title}</span>
        {subtitle && <span className="text-muted text-[12px] font-semibold">{subtitle}</span>}
      </div>
      {right ?? <div className="w-11" />}
    </div>
  );
}

export function Wordmark() {
  return (
    <div className="flex items-center gap-2.5">
      <svg width="28" height="28" viewBox="0 0 32 32" aria-hidden="true">
        <rect width="32" height="32" rx="8" fill="#1C1917" />
        <path d="M16 6c3 4 7 7 7 12a7 7 0 0 1-14 0c0-3 1.5-5 3-7 0 3 1 4 2 4 0-3 .5-6 2-9z" fill="#F59E0B" />
      </svg>
      <div className="flex flex-col leading-tight">
        <span className="text-[20px] font-extrabold tracking-[-0.02em]">Hearth</span>
        <span className="text-muted text-[11px] font-semibold">household money, on plan.</span>
      </div>
    </div>
  );
}

export function Stat({ label, value, tone, dark }: { label: string; value: ReactNode; tone?: "attention" | "income" | "muted"; dark?: boolean }) {
  const c = tone === "attention" ? "text-attention" : tone === "income" ? "text-income" : tone === "muted" ? "text-muted" : "";
  return (
    <div className={`flex flex-col gap-0.5 rounded-xl px-2.5 py-2.5 ${dark ? "bg-dark-tile" : "bg-card-head"}`}>
      <span className={`text-[11px] font-semibold ${dark ? "text-faint" : "text-muted"}`}>{label}</span>
      <span className={`text-[15px] font-bold ${c}`}>{value}</span>
    </div>
  );
}

/** Confirm-style action row at the bottom of a sheet. */
export function SheetActions({ children }: { children: ReactNode }) {
  return <div className="mt-4 flex gap-2 [&>*]:flex-1">{children}</div>;
}
