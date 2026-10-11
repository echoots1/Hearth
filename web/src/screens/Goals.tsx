import { useEffect, useState } from "react";
import { AccountPicker } from "../components/AccountPicker";
import { CategoryPicker } from "../components/CategoryPicker";
import { Card, Empty, ErrorNote, Field, Icon, MoneyInput, PageHeader, Progress, SectionTitle, Sheet, SheetActions, Spinner, Stat } from "../components/ui";
import { tool } from "../lib/api";
import { useDebtComparison, useDebtProjection, useDebts, useGoals, useToday } from "../lib/data";
import { ago, monthLabel, monthShort, ordinal, todayIso } from "../lib/dates";
import { apr, dollars, money } from "../lib/money";
import { useMutation, useQuery } from "../lib/query";
import type { Debt, DebtProjection, Goal } from "../lib/types";

type WhatIf = {
  baseline: DebtProjection;
  with_extra: DebtProjection | null;
  months_sooner: number | null;
  interest_saved_cents: number | null;
  charge: { extra_interest_cents: number; payoff_delay_days: number; interest_per_day_cents: number; new_payoff_month: string | null } | null;
};

export function Goals() {
  const today = useToday();
  const debts = useDebts();
  const proj = useDebtProjection();
  const goals = useGoals();
  const [editDebt, setEditDebt] = useState<Debt | null>(null);
  const [addDebt, setAddDebt] = useState(false);
  const [editGoal, setEditGoal] = useState<Goal | null>(null);
  const [addGoal, setAddGoal] = useState(false);
  if (!today.data) return <Spinner />;
  const open = (debts.data ?? []).filter((d) => !d.is_closed);
  return (
    <>
      <PageHeader title="Goals" subtitle="out of debt, then building savings" />
      {open.length > 0 && proj.data ? <DebtFreeHero p={proj.data} debts={open} /> : null}

      <SectionTitle title="Debts" right={open.length ? `${dollars(open.reduce((s, d) => s + d.balance_cents, 0))} owed` : undefined} />
      {open.length === 0 && <Card><Empty>No debts on the books. Add one to see the payoff forecast.</Empty></Card>}
      {open.map((d) => (
        <DebtCard key={d.id} d={d} p={proj.data} onEdit={() => setEditDebt(d)} />
      ))}
      <button type="button" className="btn-light" onClick={() => setAddDebt(true)}><Icon name="plus" size={16} /> Add a debt</button>

      {open.length > 1 && <StrategyCard />}

      <SectionTitle title="Savings goals" />
      {!goals.data?.length && <Card><Empty>No goals yet.</Empty></Card>}
      {(goals.data ?? []).map((g) => <GoalCard key={g.id} g={g} onEdit={() => setEditGoal(g)} />)}
      <button type="button" className="btn-light" onClick={() => setAddGoal(true)}><Icon name="plus" size={16} /> Add a goal</button>

      <DebtSheet open={addDebt || !!editDebt} onClose={() => { setAddDebt(false); setEditDebt(null); }} debt={editDebt} />
      <GoalSheet open={addGoal || !!editGoal} onClose={() => { setAddGoal(false); setEditGoal(null); }} goal={editGoal} />
    </>
  );
}

function DebtFreeHero({ p, debts }: { p: DebtProjection; debts: Debt[] }) {
  const [extra, setExtra] = useState<number | null>(null);
  const first = p.debts[0]?.debt_id ?? debts[0].id;
  const wi = useQuery<WhatIf>(extra ? `debts.what_if:${first}:${extra}` : null, () => tool("debts.what_if", { debt_id: first, extra_monthly_cents: extra }));
  const apply = useMutation(() => tool("debts.set_strategy", { extra_payment_cents: p.extra_payment_cents + (extra ?? 0) }), ["debts.", "household."]);
  const perDay = debts.reduce((s, d) => s + d.interest_per_day_cents, 0);
  return (
    <div className="bg-dark text-ground flex flex-col gap-4 rounded-[var(--radius-hero)] p-5">
      <div className="flex flex-col gap-1">
        <span className="text-faint text-[12px] font-bold tracking-[0.06em] uppercase">Debt-free</span>
        <span className="text-[36px] leading-none font-extrabold tracking-[-0.03em]">{p.debt_free_month ? monthLabel(p.debt_free_month) : "—"}</span>
        <span className="text-faint text-[12px] font-semibold">
          {p.strategy} · {dollars(p.total_monthly_payment_cents)}/mo{p.extra_payment_cents > 0 && ` incl. ${dollars(p.extra_payment_cents)} extra`} · {dollars(p.total_interest_cents)} interest left · {money(perDay)}/day
        </span>
      </div>
      <div className="bg-dark-line h-px" />
      <div className="flex flex-col gap-2">
        <span className="text-faint text-[12px] font-bold tracking-[0.06em] uppercase">What if we paid more each month?</span>
        <div className="flex gap-2">
          {[5000, 10000, 20000].map((c) => (
            <button key={c} type="button" onClick={() => setExtra(extra === c ? null : c)} className={`h-9 rounded-full px-3.5 text-[13px] font-bold ${extra === c ? "bg-ground text-ink" : "border-dark-line border"}`}>+{dollars(c)}</button>
          ))}
        </div>
        {extra && wi.data && (
          <div className="grid grid-cols-3 gap-2">
            <Stat dark label="Sooner" value={wi.data.months_sooner != null ? `${wi.data.months_sooner} mo` : "—"} />
            <Stat dark label="Interest saved" value={dollars(wi.data.interest_saved_cents ?? 0)} />
            <Stat dark label="Debt-free" value={wi.data.with_extra?.debt_free_month ? monthShort(wi.data.with_extra.debt_free_month) : "—"} />
          </div>
        )}
        {extra && wi.data && (
          <button type="button" className="border-dark-line text-ground h-10 rounded-full border text-[13px] font-bold" disabled={apply.busy} onClick={async () => { if ((await apply.run()) !== undefined) setExtra(null); }}>
            Make +{dollars(extra)} the plan
          </button>
        )}
        <ErrorNote error={apply.error} />
      </div>
    </div>
  );
}

function DebtCard({ d, p, onEdit }: { d: Debt; p: DebtProjection | undefined; onEdit: () => void }) {
  const row = p?.debts.find((x) => x.debt_id === d.id);
  const order = p ? p.debts.findIndex((x) => x.debt_id === d.id) + 1 : 0;
  const [bal, setBal] = useState(false);
  const [charge, setCharge] = useState(false);
  return (
    <Card className="flex flex-col gap-3 p-4">
      <button type="button" className="flex w-full items-start justify-between gap-3 text-left" onClick={onEdit}>
        <div className="flex flex-col gap-0.5">
          <span className="text-[15px] font-bold">{d.name}{order > 0 && <span className="bg-line ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">#{order} in order</span>}</span>
          <span className="text-muted text-[12px] font-semibold">{apr(d.apr_bps)} APR · min {dollars(d.min_payment_cents)} · due {ordinal(d.due_day)} · {money(d.interest_per_day_cents)}/day interest</span>
        </div>
        <span className="text-[17px] font-extrabold">{dollars(d.balance_cents)}</span>
      </button>
      <div className="grid grid-cols-3 gap-2">
        <Stat label="Paying" value={`${dollars(row?.monthly_payment_cents ?? d.planned_payment_cents ?? d.min_payment_cents)}/mo`} />
        <Stat label="Paid off" value={row?.payoff_month ? monthShort(row.payoff_month) : "—"} />
        <Stat label="Interest to go" value={dollars(row?.total_interest_cents ?? 0)} />
      </div>
      <div className="flex gap-2">
        <button type="button" className="chip-light" onClick={() => setBal(true)}>Set balance</button>
        <button type="button" className="chip-light" onClick={() => setCharge(true)}>What if a charge…</button>
      </div>
      <span className="text-faint text-[11px] font-semibold">balance as of {ago(d.balance_as_of)}</span>
      <SetBalanceSheet open={bal} onClose={() => setBal(false)} d={d} />
      <ChargeSheet open={charge} onClose={() => setCharge(false)} d={d} />
    </Card>
  );
}

function SetBalanceSheet({ open, onClose, d }: { open: boolean; onClose: () => void; d: Debt }) {
  const [v, setV] = useState<number | null>(d.balance_cents);
  useEffect(() => setV(d.balance_cents), [d.balance_cents, open]);
  const m = useMutation(() => tool("debts.set_balance", { debt_id: d.id, balance_cents: v }), ["debts."]);
  return (
    <Sheet open={open} onClose={onClose} title={`${d.name} balance`}>
      <Field label="Current balance"><MoneyInput value={v} onChange={setV} autoFocus /></Field>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || v == null} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>Save</button>
      </SheetActions>
    </Sheet>
  );
}

function ChargeSheet({ open, onClose, d }: { open: boolean; onClose: () => void; d: Debt }) {
  const [amt, setAmt] = useState<number | null>(null);
  const wi = useQuery<WhatIf>(open && amt ? `debts.what_if:charge:${d.id}:${amt}` : null, () => tool("debts.what_if", { debt_id: d.id, charge_cents: amt }));
  const c = wi.data?.charge;
  return (
    <Sheet open={open} onClose={onClose} title={`A charge on ${d.name}`}>
      <Field label="If we put this on the card" hint="Hearth shows what it costs over the whole payoff."><MoneyInput value={amt} onChange={setAmt} autoFocus /></Field>
      {c && (
        <div className="mt-3 grid grid-cols-3 gap-2">
          <Stat label="Extra interest" value={dollars(c.extra_interest_cents)} tone="attention" />
          <Stat label="Delays payoff" value={`${Math.round(c.payoff_delay_days / 30)} mo`} />
          <Stat label="Costs per day" value={money(c.interest_per_day_cents)} />
        </div>
      )}
      <SheetActions><button type="button" className="btn-dark" onClick={onClose}>Close</button></SheetActions>
    </Sheet>
  );
}

function StrategyCard() {
  const cmp = useDebtComparison();
  const set = useMutation((kind: string) => tool("debts.set_strategy", { kind }), ["debts.", "household."]);
  if (!cmp.data) return null;
  const items: { key: "avalanche" | "snowball" | "custom"; label: string; hint: string }[] = [
    { key: "avalanche", label: "Avalanche", hint: "highest APR first" },
    { key: "snowball", label: "Snowball", hint: "smallest balance first" },
    { key: "custom", label: "Custom", hint: "your order" },
  ];
  return (
    <>
      <SectionTitle title="Strategy" right="switching is a deliberate tap" />
      <Card className="divider overflow-hidden">
        {items.map((it) => {
          const p = cmp.data![it.key];
          const current = cmp.data!.current === it.key;
          return (
            <div key={it.key} className={`row ${current ? "bg-card-head" : ""}`}>
              <div className="flex flex-col gap-0.5">
                <span className="text-[15px] font-bold">{it.label}{current && <span className="bg-ink text-ground ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">current</span>}</span>
                <span className="text-muted text-[12px] font-semibold">{it.hint} · debt-free {p.debt_free_month ? monthShort(p.debt_free_month) : "—"} · {dollars(p.total_interest_cents)} interest</span>
              </div>
              {!current && <button type="button" className="chip-light" disabled={set.busy} onClick={() => set.run(it.key)}>Use</button>}
            </div>
          );
        })}
        {set.error && <div className="px-4 py-2"><ErrorNote error={set.error} /></div>}
      </Card>
    </>
  );
}

function GoalCard({ g, onEdit }: { g: Goal; onEdit: () => void }) {
  const [saved, setSaved] = useState(false);
  const [v, setV] = useState<number | null>(g.saved_cents);
  const m = useMutation(() => tool("goals.set_saved", { goal_id: g.id, saved_cents: v }), ["goals."]);
  const p = g.projection;
  const done = g.status === "reached";
  return (
    <Card className="flex flex-col gap-3 p-4">
      <button type="button" className="flex w-full items-start justify-between gap-3 text-left" onClick={onEdit}>
        <div className="flex flex-col gap-0.5">
          <span className="text-[15px] font-bold">{g.name}{g.status !== "active" && <span className="bg-line ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">{g.status}</span>}</span>
          <span className="text-muted text-[12px] font-semibold">
            {dollars(g.saved_cents)} of {dollars(g.target_cents)}{g.target_date && ` · by ${monthShort(g.target_date.slice(0, 7))}`}{p && p.monthly_cents > 0 && ` · ${dollars(p.monthly_cents)}/mo`}
          </span>
        </div>
        <span className="text-[17px] font-extrabold">{p ? `${Math.round(p.progress_bps / 100)}%` : ""}</span>
      </button>
      <Progress bps={p?.progress_bps ?? Math.round((g.saved_cents / g.target_cents) * 10000)} color={done ? "income" : "ink"} />
      {p && !done && (
        <div className="grid grid-cols-3 gap-2">
          <Stat label="Reach at this pace" value={p.reach_month ? monthShort(p.reach_month) : "—"} tone={p.on_track === false ? "attention" : undefined} />
          <Stat label="Needed per month" value={p.needed_monthly_cents != null ? dollars(p.needed_monthly_cents) : "—"} />
          <Stat label="To go" value={dollars(p.remaining_cents)} />
        </div>
      )}
      {saved ? (
        <div className="flex gap-2">
          <div className="flex-1"><MoneyInput value={v} onChange={setV} autoFocus /></div>
          <button type="button" className="btn-dark" disabled={m.busy || v == null} onClick={async () => { if ((await m.run()) !== undefined) setSaved(false); }}>Save</button>
          <button type="button" className="btn-light" onClick={() => setSaved(false)}>Back</button>
        </div>
      ) : (
        <div className="flex items-center justify-between">
          <button type="button" className="chip-light" onClick={() => setSaved(true)}>Update saved</button>
          <span className="text-faint text-[11px] font-semibold">{g.saved_as_of ? `updated ${ago(g.saved_as_of)}` : ""}</span>
        </div>
      )}
      <ErrorNote error={m.error} />
    </Card>
  );
}

function DebtSheet({ open, onClose, debt }: { open: boolean; onClose: () => void; debt: Debt | null }) {
  const blank = { name: "", kind: "credit_card" as Debt["kind"], balance: null as number | null, apr: "", min: null as number | null, planned: null as number | null, due_day: 1, account_id: null as string | null, note: "" };
  const [f, setF] = useState(blank);
  useEffect(() => {
    setF(debt ? { name: debt.name, kind: debt.kind, balance: debt.balance_cents, apr: (debt.apr_bps / 100).toString(), min: debt.min_payment_cents, planned: debt.planned_payment_cents, due_day: debt.due_day, account_id: debt.account_id, note: debt.note } : blank);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [debt, open]);
  const m = useMutation(
    () =>
      tool("debts.upsert", {
        debt_id: debt?.id ?? null, name: f.name, kind: f.kind, balance_cents: f.balance, apr_bps: Math.round(Number(f.apr) * 100), min_payment_cents: f.min, planned_payment_cents: f.planned, due_day: f.due_day, account_id: f.account_id, note: f.note || null,
      }),
    ["debts.", "household."],
  );
  return (
    <Sheet open={open} onClose={onClose} title={debt ? debt.name : "Add a debt"}>
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus={!debt} /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Kind">
            <select className="field" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value as Debt["kind"] })}>
              <option value="credit_card">Credit card</option><option value="loan">Loan</option><option value="mortgage">Mortgage</option><option value="other">Other</option>
            </select>
          </Field>
          <Field label="APR %"><input className="field" inputMode="decimal" value={f.apr} onChange={(e) => setF({ ...f, apr: e.target.value })} placeholder="24.99" /></Field>
        </div>
        <Field label="Balance"><MoneyInput value={f.balance} onChange={(c) => setF({ ...f, balance: c })} /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Minimum payment"><MoneyInput value={f.min} onChange={(c) => setF({ ...f, min: c })} /></Field>
          <Field label="Planned payment" hint="What you actually pay."><MoneyInput value={f.planned} onChange={(c) => setF({ ...f, planned: c })} /></Field>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Due day"><input className="field" type="number" min={1} max={31} value={f.due_day} onChange={(e) => setF({ ...f, due_day: Number(e.target.value) })} /></Field>
          <Field label="Account"><AccountPicker value={f.account_id} onChange={(id) => setF({ ...f, account_id: id })} none="Not linked" /></Field>
        </div>
        <Field label="Note"><input className="field" value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} /></Field>
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim() || f.balance == null || f.min == null || !f.apr} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>{debt ? "Save" : "Add debt"}</button>
      </SheetActions>
    </Sheet>
  );
}

function GoalSheet({ open, onClose, goal }: { open: boolean; onClose: () => void; goal: Goal | null }) {
  const blank = { name: "", target: null as number | null, target_date: "", account_id: null as string | null, category_id: null as string | null, saved: null as number | null, note: "", status: "active" as Goal["status"] };
  const [f, setF] = useState(blank);
  useEffect(() => {
    setF(goal ? { name: goal.name, target: goal.target_cents, target_date: goal.target_date ?? "", account_id: goal.account_id, category_id: goal.category_id, saved: goal.saved_cents, note: goal.note, status: goal.status } : blank);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goal, open]);
  const m = useMutation(
    () =>
      goal
        ? tool("goals.update", { goal_id: goal.id, name: f.name, target_cents: f.target, target_date: f.target_date || null, category_id: f.category_id, note: f.note || null, status: f.status })
        : tool("goals.create", { name: f.name, target_cents: f.target, target_date: f.target_date || null, account_id: f.account_id, category_id: f.category_id, saved_cents: f.saved, note: f.note || null }),
    ["goals."],
  );
  return (
    <Sheet open={open} onClose={onClose} title={goal ? goal.name : "Add a goal"}>
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus={!goal} placeholder="Emergency fund" /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Target"><MoneyInput value={f.target} onChange={(c) => setF({ ...f, target: c })} /></Field>
          <Field label="By (optional)"><input className="field" type="date" value={f.target_date} min={todayIso()} onChange={(e) => setF({ ...f, target_date: e.target.value })} /></Field>
        </div>
        {!goal && <Field label="Saved so far"><MoneyInput value={f.saved} onChange={(c) => setF({ ...f, saved: c })} /></Field>}
        <Field label="Funded by category" hint="Its monthly plan is the contribution the projection uses."><CategoryPicker value={f.category_id} onChange={(id) => setF({ ...f, category_id: id })} none="Not set" /></Field>
        {!goal && <Field label="Held in account"><AccountPicker value={f.account_id} onChange={(id) => setF({ ...f, account_id: id })} none="Not set" /></Field>}
        {goal && (
          <Field label="Status">
            <select className="field" value={f.status} onChange={(e) => setF({ ...f, status: e.target.value as Goal["status"] })}>
              <option value="active">Active</option><option value="reached">Reached</option><option value="paused">Paused</option><option value="dropped">Dropped</option>
            </select>
          </Field>
        )}
        <Field label="Note"><input className="field" value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} /></Field>
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim() || !f.target} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>{goal ? "Save" : "Add goal"}</button>
      </SheetActions>
    </Sheet>
  );
}
