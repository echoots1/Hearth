import { useEffect, useState } from "react";
import { AccountPicker, useAccountName } from "../components/AccountPicker";
import { CategoryPicker, useCategoryName } from "../components/CategoryPicker";
import { Card, CardHead, Empty, ErrorNote, Field, Icon, MoneyInput, PageHeader, SectionTitle, Segmented, Sheet, SheetActions, Spinner, Stat, Toggle } from "../components/ui";
import { tool } from "../lib/api";
import { useBills, usePlanned, useSinking, useTimeline, useToday } from "../lib/data";
import { ago, dueWord, longDate, monthShort, ordinal, shortDate, todayIso } from "../lib/dates";
import { dollars, money, signed } from "../lib/money";
import { useMutation } from "../lib/query";
import type { Bill, PlannedItem, TimelineEntry, Today } from "../lib/types";

export function Plan() {
  const today = useToday();
  const bills = useBills({ include_canceled: true });
  const planned = usePlanned({ limit: 100 });
  const [addBill, setAddBill] = useState(false);
  const [editBill, setEditBill] = useState<Bill | null>(null);
  const [addPlanned, setAddPlanned] = useState(false);
  const [editPlanned, setEditPlanned] = useState<PlannedItem | null>(null);
  if (!today.data) return <Spinner />;
  const t = today.data;
  const all = bills.data ?? [];
  const active = all.filter((b) => b.status === "active");
  const monthly = all.filter((b) => b.cadence_months === 1);
  const yearly = all.filter((b) => b.cadence_months === 12);
  const everyN = all.filter((b) => b.cadence_months !== 1 && b.cadence_months !== 12);

  return (
    <>
      <PageHeader title="Plan" subtitle={`next paycheck ${shortDate(t.next_payday)} · ${dueWord(t.next_payday, t.today)}`} />

      <TimelineCard
        today={t}
        onBill={(id) => { const b = all.find((x) => x.id === id); if (b) setEditBill(b); }}
        onPlanned={(id) => { const p = (planned.data ?? []).find((x) => x.id === id); if (p) setEditPlanned(p); }}
      />

      <SectionTitle title="Recurring bills" right={`${dollars(active.reduce((s, b) => s + (b.monthly_equivalent_cents ?? Math.round(b.amount_cents / b.cadence_months)), 0))}/mo`} />
      <BillGroup title="Monthly" bills={monthly} today={t} onTap={setEditBill} />
      <BillGroup title="Every few months" bills={everyN} today={t} onTap={setEditBill} />
      <BillGroup title="Yearly" bills={yearly} today={t} onTap={setEditBill} />
      <button type="button" className="btn-light" onClick={() => setAddBill(true)}><Icon name="plus" size={16} /> Add a bill</button>

      <SinkingCard />

      <SectionTitle title="One-offs" right="planned ahead of time" />
      <Card className="divider overflow-hidden">
        {!planned.data?.length && <Empty>Nothing planned. "Can we buy X?" lands here as a YES.</Empty>}
        {(planned.data ?? []).map((p) => <PlannedRow key={p.id} p={p} onTap={() => setEditPlanned(p)} />)}
      </Card>
      <button type="button" className="btn-light" onClick={() => setAddPlanned(true)}><Icon name="plus" size={16} /> Plan a one-off</button>

      <BillSheet open={addBill || !!editBill} onClose={() => { setAddBill(false); setEditBill(null); }} bill={editBill} today={t} />
      <PlannedSheet open={addPlanned || !!editPlanned} onClose={() => { setAddPlanned(false); setEditPlanned(null); }} item={editPlanned} />
    </>
  );
}

const HORIZONS = [
  { key: "28", label: "4 weeks" }, { key: "56", label: "8 weeks" }, { key: "91", label: "13 weeks" },
] as const;
type Horizon = (typeof HORIZONS)[number]["key"];

/** Expected cash from today: paychecks, bills, one-offs and the everyday drain, with the balance after each. */
function TimelineCard({ today, onBill, onPlanned }: { today: Today; onBill: (id: string) => void; onPlanned: (id: string) => void }) {
  const [horizon, setHorizon] = useState<Horizon>("56");
  const q = useTimeline(Number(horizon));
  const v = q.data;
  const t = v?.timeline;
  const floor = v?.cash_floor_cents ?? 0;
  return (
    <>
      <SectionTitle title="Cash ahead" right={<Segmented small value={horizon} onChange={setHorizon} items={HORIZONS.map((h) => ({ key: h.key, label: h.label }))} />} />
      {q.error && <ErrorNote error={q.error.message} />}
      {!t && !q.error && <Spinner />}
      {v && t && (
        <Card className="divider overflow-hidden">
          <div className="flex flex-col gap-3 p-4">
            <div className="grid grid-cols-3 gap-2">
              <Stat label="Now" value={money(t.start_cents)} />
              <Stat label={`Low · ${shortDate(t.low_date)}`} value={money(t.low_cents)} tone={t.low_cents < floor ? "attention" : undefined} />
              <Stat label={`On ${shortDate(t.until)}`} value={money(t.end_cents)} tone={t.end_cents < floor ? "attention" : t.end_cents > t.start_cents ? "income" : undefined} />
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <span className={`rounded-[14px] px-3 py-1.5 text-[13px] font-bold ${v.headroom_cents >= 0 ? "bg-floor text-ink" : "bg-attention text-white"}`}>
                {v.headroom_cents >= 0 ? `${dollars(v.headroom_cents)} above the floor at the low` : `${dollars(-v.headroom_cents)} below the floor at the low`}
              </span>
              <span className="text-muted text-[12px] font-semibold">
                {t.paydays.length} paycheck{t.paydays.length === 1 ? "" : "s"} · +{dollars(t.income_cents)} · bills −{dollars(t.bills_cents)} · everyday −{dollars(t.everyday_cents)}{t.planned_cents > 0 && ` · one-offs −${dollars(t.planned_cents)}`}
              </span>
            </div>
            {t.overdue_count > 0 && (
              <p className="text-attention text-[12px] font-semibold">
                {t.overdue_count} item{t.overdue_count === 1 ? " is" : "s are"} past due and not posted yet. {t.overdue_count === 1 ? "It stays" : "They stay"} on the timeline, counted today, until the charge comes out.
              </p>
            )}
            {t.unscheduled_income_monthly_cents > 0 && (
              <p className="text-muted text-[12px] font-semibold">
                Not counting {dollars(t.unscheduled_income_monthly_cents)}/mo of income with no payday. Set those categories to plan per paycheck to put them on the timeline.
              </p>
            )}
            {t.paydays.length === 0 && t.income_cents === 0 && t.unscheduled_income_monthly_cents === 0 && (
              <p className="text-muted text-[12px] font-semibold">No income on the timeline: set your paycheck category to plan per paycheck (Budget → category → edit) and check the pay schedule in Settings.</p>
            )}
          </div>
          {t.entries.length === 0 && <Empty>Nothing expected before {shortDate(t.until)}.</Empty>}
          {t.entries.map((e, i) => <TimelineRow key={i} e={e} today={today} floor={floor} onTap={e.bill_id ? () => onBill(e.bill_id!) : e.planned_item_id ? () => onPlanned(e.planned_item_id!) : undefined} />)}
        </Card>
      )}
    </>
  );
}

function TimelineRow({ e, today, floor, onTap }: { e: TimelineEntry; today: Today; floor: number; onTap?: () => void }) {
  const catName = useCategoryName();
  const income = e.amount_cents > 0;
  const everyday = e.kind === "everyday";
  const when = everyday
    ? `${e.days} day${e.days === 1 ? "" : "s"} through ${shortDate(e.date)}`
    : e.overdue && e.due_date
      ? `due ${shortDate(e.due_date)} · ${dueWord(e.due_date, today.today)} · not posted yet`
      : `${shortDate(e.date)} · ${dueWord(e.date, today.today)}`;
  const what = e.kind === "paycheck" ? "paycheck" : e.kind === "bill" ? "bill" : e.kind === "planned" ? "one-off" : null;
  const body = (
    <>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className={`truncate text-[15px] font-bold ${everyday ? "text-muted" : ""}`}>{e.label}</span>
        <span className={`truncate text-[12px] font-semibold ${e.overdue ? "text-attention" : "text-muted"}`}>
          {when}{what && ` · ${what}`}{e.category_id && e.kind !== "paycheck" && ` · ${catName(e.category_id)}`}
        </span>
      </div>
      <div className="flex shrink-0 flex-col items-end">
        <span className={`text-[15px] font-extrabold ${income ? "text-income" : everyday ? "text-muted" : ""}`}>{signed(e.amount_cents)}</span>
        <span className={`text-[11px] font-semibold ${e.balance_after_cents < floor ? "text-attention" : "text-muted"}`}>{money(e.balance_after_cents)} after</span>
      </div>
    </>
  );
  return onTap ? (
    <button type="button" onClick={onTap} className="row w-full text-left">{body}</button>
  ) : (
    <div className="row">{body}</div>
  );
}

function BillGroup({ title, bills, today, onTap }: { title: string; bills: Bill[]; today: Today; onTap: (b: Bill) => void }) {
  if (bills.length === 0) return null;
  const sorted = [...bills].sort((a, b) => (a.status === b.status ? a.next_due.localeCompare(b.next_due) : a.status === "active" ? -1 : 1));
  return (
    <Card className="divider overflow-hidden">
      <CardHead title={title} right={`${bills.filter((b) => b.status === "active").length} active`} />
      {sorted.map((b) => <BillRow key={b.id} b={b} today={today} onTap={() => onTap(b)} />)}
    </Card>
  );
}

function BillRow({ b, today, onTap }: { b: Bill; today: Today; onTap: () => void }) {
  const catName = useCategoryName();
  const accountName = useAccountName();
  const overdue = b.status === "active" && b.next_due < today.today;
  const inLowPoint = b.status === "active" && b.next_due < today.next_payday;
  const when = b.cadence_months === 1 ? `due ${ordinal(Number(b.next_due.slice(8)))}` : `next ${shortDate(b.next_due)}`;
  return (
    <button type="button" onClick={onTap} className="row w-full text-left">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="flex items-center gap-2 truncate text-[15px] font-bold">
          {b.name}
          {b.status !== "active" && <span className="bg-line rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">{b.status}</span>}
        </span>
        <span className={`truncate text-[12px] font-semibold ${overdue || inLowPoint ? "text-attention" : "text-muted"}`}>
          {when}{overdue && " · overdue, not posted"}{inLowPoint && " · in low point"} · {catName(b.category_id)}{b.account_id && ` · ${accountName(b.account_id)}`}
        </span>
      </div>
      <div className="flex shrink-0 flex-col items-end">
        <span className="text-[15px] font-extrabold">{money(b.amount_cents)}</span>
        {b.cadence_months !== 1 && <span className="text-muted text-[11px] font-semibold">{dollars(b.monthly_equivalent_cents ?? Math.round(b.amount_cents / b.cadence_months))}/mo</span>}
      </div>
    </button>
  );
}

function PlannedRow({ p, onTap }: { p: PlannedItem; onTap: () => void }) {
  const catName = useCategoryName();
  return (
    <button type="button" onClick={onTap} className="row w-full text-left">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="flex items-center gap-2 truncate text-[15px] font-bold">
          {p.item}
          <span className={`rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase ${p.status === "planned" ? "bg-floor" : "bg-line"}`}>{p.status}</span>
        </span>
        <span className="text-muted truncate text-[12px] font-semibold">
          {shortDate(p.date)} · {catName(p.category_id)}{p.decided_by && ` · ${p.decided_by}`}{p.matched_transaction_id && " · posted"}
        </span>
      </div>
      <span className="shrink-0 text-[15px] font-extrabold">{money(p.amount_cents)}</span>
    </button>
  );
}

function SinkingCard() {
  const s = useSinking();
  const [open, setOpen] = useState(false);
  const [balance, setBalance] = useState<number | null>(null);
  const m = useMutation(() => tool("sinking.set_balance", { balance_cents: balance }), ["sinking."]);
  if (!s.data) return null;
  const v = s.data.view;
  const short = v.shortfall_cents > 0;
  return (
    <>
      <SectionTitle title="Sinking fund" right="for the non-monthly bills" />
      <Card className="flex flex-col gap-3 p-4">
        <div className="flex items-end justify-between">
          <div className="flex flex-col gap-1">
            <span className="label">Set aside</span>
            <span className={`text-[30px] leading-none font-extrabold tracking-[-0.03em] ${short ? "text-attention" : ""}`}>{dollars(v.funded_balance_cents)}</span>
            <span className="text-muted text-[12px] font-semibold">
              should have {dollars(v.should_have_saved_cents)}{short ? ` · ${dollars(v.shortfall_cents)} short` : v.surplus_cents > 0 ? ` · ${dollars(v.surplus_cents)} ahead` : ""}
              {s.data.balance_as_of && ` · updated ${ago(s.data.balance_as_of)}`}
            </span>
          </div>
          <button type="button" className="btn-light h-9" onClick={() => { setBalance(v.funded_balance_cents); setOpen(true); }}>Update</button>
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Stat label="Monthly set-aside" value={dollars(v.monthly_set_aside_cents)} />
          <Stat label="Per year" value={dollars(v.annualized_cents)} />
        </div>
        {v.items.length > 0 && (
          <div className="divider border-line-soft border-t pt-1">
            {v.items.map((it) => (
              <div key={it.id} className="flex items-center justify-between py-2">
                <div className="flex flex-col">
                  <span className="text-[13px] font-bold">{it.name}</span>
                  <span className={`text-[11px] font-semibold ${it.overdue ? "text-attention" : "text-muted"}`}>
                    {money(it.amount_cents)} every {it.cycle_months === 12 ? "year" : `${it.cycle_months} months`} · due {monthShort(it.due_month)}
                  </span>
                </div>
                <span className="text-muted text-[12px] font-bold">{dollars(it.should_have_saved_cents)} by now</span>
              </div>
            ))}
          </div>
        )}
      </Card>
      <Sheet open={open} onClose={() => setOpen(false)} title="Sinking fund balance">
        <Field label="What's actually set aside right now"><MoneyInput value={balance} onChange={setBalance} autoFocus /></Field>
        <ErrorNote error={m.error} />
        <SheetActions>
          <button type="button" className="btn-light" onClick={() => setOpen(false)}>Cancel</button>
          <button type="button" className="btn-dark" disabled={m.busy || balance == null} onClick={async () => { if ((await m.run()) !== undefined) setOpen(false); }}>Save</button>
        </SheetActions>
      </Sheet>
    </>
  );
}

const CADENCES = [
  { v: 1, label: "Monthly" }, { v: 2, label: "Every 2 months" }, { v: 3, label: "Quarterly" }, { v: 6, label: "Twice a year" }, { v: 12, label: "Yearly" },
];

function BillSheet({ open, onClose, bill, today }: { open: boolean; onClose: () => void; bill: Bill | null; today: Today }) {
  const blank = { name: "", amount: null as number | null, cadence: 1, next_due: todayIso(), category_id: null as string | null, account_id: null as string | null, match_pattern: "", note: "", raise_plan: true };
  const [f, setF] = useState(blank);
  useEffect(() => {
    setF(bill ? { name: bill.name, amount: bill.amount_cents, cadence: bill.cadence_months, next_due: bill.next_due, category_id: bill.category_id, account_id: bill.account_id, match_pattern: bill.match_pattern ?? "", note: bill.note, raise_plan: false } : blank);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bill, open]);
  const inv = ["bills.", "budget.", "balance.", "sinking."];
  const save = useMutation(
    () =>
      bill
        ? tool("bills.update", { bill_id: bill.id, name: f.name, amount_cents: f.amount, cadence_months: f.cadence, next_due: f.next_due, category_id: f.category_id, account_id: f.account_id, match_pattern: f.match_pattern || null, note: f.note || null })
        : tool("bills.add", { name: f.name, amount_cents: f.amount, cadence_months: f.cadence, next_due: f.next_due, category_id: f.category_id, account_id: f.account_id, match_pattern: f.match_pattern || null, note: f.note || null, raise_plan: f.raise_plan }),
    inv,
  );
  const status = useMutation((verb: "pause" | "resume" | "cancel" | "delete") => tool(`bills.${verb}`, { bill_id: bill!.id }), inv);
  const act = async (fn: () => Promise<unknown>) => { if ((await fn()) !== undefined) onClose(); };
  return (
    <Sheet open={open} onClose={onClose} title={bill ? bill.name : "Add a bill"}>
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus={!bill} placeholder="Demo Video" /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Amount"><MoneyInput value={f.amount} onChange={(c) => setF({ ...f, amount: c })} /></Field>
          <Field label="How often">
            <select className="field" value={f.cadence} onChange={(e) => setF({ ...f, cadence: Number(e.target.value) })}>
              {CADENCES.map((c) => <option key={c.v} value={c.v}>{c.label}</option>)}
            </select>
          </Field>
        </div>
        <Field label="Next due"><input className="field" type="date" value={f.next_due} onChange={(e) => setF({ ...f, next_due: e.target.value })} /></Field>
        <Field label="Category" hint={f.cadence !== 1 ? "Non-monthly bills post to their category and are tracked by the sinking fund." : undefined}><CategoryPicker value={f.category_id} onChange={(id) => setF({ ...f, category_id: id })} /></Field>
        <Field label="Paid from"><AccountPicker value={f.account_id} onChange={(id) => setF({ ...f, account_id: id })} none="Not set" /></Field>
        <Field label="Matches descriptions containing" hint="How Hearth recognizes the charge when it posts."><input className="field" value={f.match_pattern} onChange={(e) => setF({ ...f, match_pattern: e.target.value })} placeholder={f.name.toLowerCase()} /></Field>
        <Field label="Note"><input className="field" value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} /></Field>
        {!bill && <Toggle label="Raise the category's plan if this doesn't fit" checked={f.raise_plan} onChange={(v) => setF({ ...f, raise_plan: v })} />}
        {bill && (
          <div className="text-muted text-[12px] font-semibold">
            {bill.last_matched_at ? `Last matched ${longDate(bill.last_matched_at)}` : "Not matched to a charge yet"} · status {bill.status} · next payday {shortDate(today.next_payday)}
          </div>
        )}
      </div>
      <ErrorNote error={save.error || status.error} />
      {bill && (
        <div className="mt-3 flex flex-wrap gap-2">
          {bill.status === "active" && <button type="button" className="chip-light" onClick={() => act(() => status.run("pause"))}>Pause</button>}
          {bill.status !== "active" && <button type="button" className="chip-light" onClick={() => act(() => status.run("resume"))}>Resume</button>}
          {bill.status !== "canceled" && <button type="button" className="chip-light" onClick={() => act(() => status.run("cancel"))}>Cancel the service</button>}
          <button type="button" className="chip-light text-attention" onClick={() => { if (confirm("Delete this bill outright? Canceling keeps history.")) act(() => status.run("delete")); }}>Delete</button>
        </div>
      )}
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Close</button>
        <button type="button" className="btn-dark" disabled={save.busy || !f.name.trim() || f.amount == null || !f.next_due} onClick={() => act(() => save.run())}>{bill ? "Save" : "Add bill"}</button>
      </SheetActions>
    </Sheet>
  );
}

function PlannedSheet({ open, onClose, item }: { open: boolean; onClose: () => void; item: PlannedItem | null }) {
  const blank = { item: "", amount: null as number | null, date: todayIso(), category_id: null as string | null, decided_by: "", note: "" };
  const [f, setF] = useState(blank);
  useEffect(() => {
    setF(item ? { item: item.item, amount: item.amount_cents, date: item.date, category_id: item.category_id, decided_by: item.decided_by, note: item.note } : blank);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item, open]);
  const inv = ["planned.", "balance."];
  const save = useMutation(
    () =>
      item
        ? tool("planned.update", { planned_item_id: item.id, item: f.item, amount_cents: f.amount, date: f.date, category_id: f.category_id, note: f.note || null })
        : tool("planned.create", { item: f.item, amount_cents: f.amount, date: f.date, category_id: f.category_id, decided_by: f.decided_by || null, note: f.note || null }),
    inv,
  );
  const setStatus = useMutation((status: "planned" | "done" | "dropped") => tool("planned.update", { planned_item_id: item!.id, status }), inv);
  const act = async (fn: () => Promise<unknown>) => { if ((await fn()) !== undefined) onClose(); };
  return (
    <Sheet open={open} onClose={onClose} title={item ? item.item : "Plan a one-off"}>
      <div className="flex flex-col gap-3">
        <Field label="What"><input className="field" value={f.item} onChange={(e) => setF({ ...f, item: e.target.value })} autoFocus={!item} placeholder="New tires" /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="About how much"><MoneyInput value={f.amount} onChange={(c) => setF({ ...f, amount: c })} /></Field>
          <Field label="Around when"><input className="field" type="date" value={f.date} onChange={(e) => setF({ ...f, date: e.target.value })} /></Field>
        </div>
        <Field label="Category"><CategoryPicker value={f.category_id} onChange={(id) => setF({ ...f, category_id: id })} none="Not set" /></Field>
        {!item && <Field label="Decided by"><input className="field" value={f.decided_by} onChange={(e) => setF({ ...f, decided_by: e.target.value })} placeholder="who said yes" /></Field>}
        <Field label="Note"><input className="field" value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} /></Field>
      </div>
      <ErrorNote error={save.error || setStatus.error} />
      {item && (
        <div className="mt-3 flex flex-wrap gap-2">
          {item.status !== "done" && <button type="button" className="chip-light" onClick={() => act(() => setStatus.run("done"))}>Mark done</button>}
          {item.status !== "dropped" && <button type="button" className="chip-light" onClick={() => act(() => setStatus.run("dropped"))}>Drop</button>}
          {item.status !== "planned" && <button type="button" className="chip-light" onClick={() => act(() => setStatus.run("planned"))}>Back to planned</button>}
        </div>
      )}
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Close</button>
        <button type="button" className="btn-dark" disabled={save.busy || !f.item.trim() || f.amount == null} onClick={() => act(() => save.run())}>{item ? "Save" : "Plan it"}</button>
      </SheetActions>
    </Sheet>
  );
}
