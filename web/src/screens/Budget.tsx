import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { Card, CardHead, ErrorNote, Field, Icon, MoneyInput, Pill, Progress, Sheet, SheetActions, Spinner, Stat, Toggle, Wordmark } from "../components/ui";
import { tool } from "../lib/api";
import { useBalance, useGroups, useInboxCounts, useMonth, useToday } from "../lib/data";
import { addMonths, ago, monthLabel, monthShort, shortDate } from "../lib/dates";
import { dollars, money, whole } from "../lib/money";
import { useMutation } from "../lib/query";
import type { BalanceOverview, CategoryView, GroupView, Today } from "../lib/types";

export function Budget() {
  const today = useToday();
  const [params, setParams] = useSearchParams();
  const month = params.get("month") ?? today.data?.month;
  const view = useMonth(month);
  const balance = useBalance();
  const counts = useInboxCounts();
  const [addOpen, setAddOpen] = useState(false);

  if (!today.data) return <Spinner />;
  const t = today.data;
  const isCurrent = month === t.month;
  const setMonth = (m: string) => setParams(m === t.month ? {} : { month: m });

  return (
    <>
      <div className="flex h-11 items-center justify-between">
        <Wordmark />
        <div className="flex gap-2">
          {t.assistant.mode === "api" && (
            <Link to="/chat" className="icon-btn" aria-label="Chat with Claude">
              <Icon name="sparkle" size={18} />
            </Link>
          )}
          <Link to="/more/settings" className="icon-btn" aria-label="Settings">
            <Icon name="settings" />
          </Link>
        </div>
      </div>

      <div className="flex h-11 items-center justify-between">
        <button type="button" className="flex h-11 w-11 items-center justify-center rounded-full" aria-label="Previous month" onClick={() => setMonth(addMonths(month!, -1))}>
          <Icon name="back" />
        </button>
        <div className="flex flex-col items-center leading-tight">
          <span className="text-[17px] font-extrabold">{monthLabel(month!)}</span>
          <span className="text-muted text-[12px] font-semibold">
            {isCurrent ? `day ${t.day_of_month} of ${t.days_in_month} · ${t.days_left} days left` : " "}
          </span>
        </div>
        <button type="button" className="flex h-11 w-11 items-center justify-center rounded-full" aria-label="Next month" onClick={() => setMonth(addMonths(month!, 1))}>
          <Icon name="forward" />
        </button>
      </div>

      {isCurrent && <CashCard today={t} balance={balance.data} />}

      {isCurrent && (
        <div className="flex flex-wrap gap-2">
          <Pill to="/activity?tab=inbox" tone={counts.critical ? "attention" : "light"}>
            <Icon name="alert" size={16} />
            {counts.alerts} alert{counts.alerts === 1 ? "" : "s"}
          </Pill>
          <Pill to="/activity?tab=inbox#questions">
            <Icon name="question" size={16} />
            {counts.questions} question{counts.questions === 1 ? "" : "s"} from Claude
          </Pill>
          <Pill to="/activity?tab=inbox#review">
            <Icon name="check" size={16} />
            {counts.review} to review
          </Pill>
        </div>
      )}

      {view.data ? (
        <>
          <Card className="grid grid-cols-3 gap-2 px-4 py-3">
            <Stat label="Planned in" value={dollars(view.data.totals.planned_income_cents)} tone="income" />
            <Stat label="Planned out" value={dollars(view.data.totals.planned_spend_cents)} />
            <Stat
              label="Left to assign"
              value={view.data.totals.left_to_assign_cents === 0 ? "$0 ✓" : dollars(view.data.totals.left_to_assign_cents)}
              tone={view.data.totals.left_to_assign_cents < 0 ? "attention" : undefined}
            />
          </Card>

          <div className="label flex justify-between px-1">
            <span>Category</span>
            <span className="flex gap-6">
              <span className="w-[72px] text-right">Spent / plan</span>
              <span className="w-16 text-right">Left</span>
            </span>
          </div>

          {view.data.groups.map((g) => (
            <GroupCard key={g.id} group={g} today={t} month={month!} isCurrent={isCurrent} />
          ))}

          {view.data.uncategorized_cents !== 0 && (
            <Link to="/activity?tab=inbox#review" className="card text-muted flex items-center justify-between px-4 py-3 text-[13px] font-bold">
              <span>Uncategorized this month</span>
              <span className="text-ink">{money(view.data.uncategorized_cents)}</span>
            </Link>
          )}

          <button type="button" className="btn-light" onClick={() => setAddOpen(true)}>
            <Icon name="plus" size={16} /> Add a category
          </button>
          <AddCategorySheet open={addOpen} onClose={() => setAddOpen(false)} />
        </>
      ) : view.error ? (
        <ErrorNote error={view.error.message} />
      ) : (
        <Spinner />
      )}
    </>
  );
}

function CashCard({ today, balance }: { today: Today; balance: BalanceOverview | undefined }) {
  const [open, setOpen] = useState(false);
  const lp = balance?.low_point ?? null;
  const reported = balance?.accounts.filter((a) => a.include_in_cash) ?? [];
  const oldest = reported.reduce<string | null>((acc, a) => {
    if (!a.reported_balance_as_of) return acc;
    return !acc || a.reported_balance_as_of < acc ? a.reported_balance_as_of : acc;
  }, null);
  const bank = reported.filter((a) => a.bank_balance_cents != null);
  const bankSum = bank.reduce((s, a) => s + (a.bank_balance_cents ?? 0), 0);
  const bankAge = bank.reduce<string | null>((acc, a) => (!acc || (a.bank_balance_as_of ?? "") > acc ? (a.bank_balance_as_of ?? acc) : acc), null);
  const headroom = balance ? (lp ? lp.low_point_cents : balance.household_estimate_cents) - balance.cash_floor_cents : 0;
  return (
    <div className="bg-dark text-ground flex flex-col gap-4 rounded-[var(--radius-hero)] p-5">
      <div className="flex items-start justify-between">
        <div className="flex flex-col gap-1">
          <span className="text-faint text-[12px] font-bold tracking-[0.06em] uppercase">Cash now</span>
          <span className="text-[36px] leading-none font-extrabold tracking-[-0.03em]">{balance ? money(balance.household_estimate_cents) : "…"}</span>
          <span className="text-faint text-[12px] font-semibold">
            {reported.length === 0
              ? "no cash accounts yet"
              : oldest
                ? `reported ${ago(oldest)}`
                : "no balance reported yet"}
            {bank.length > 0 && ` · bank says ${dollars(bankSum)} (${ago(bankAge)})`}
          </span>
        </div>
        <button type="button" onClick={() => setOpen(true)} className="border-dark-line text-ground h-9 rounded-full border px-3.5 text-[13px] font-bold">
          Update
        </button>
      </div>
      {lp && (
        <>
          <div className="bg-dark-line h-px" />
          <div className="flex flex-col gap-2.5">
            <div className="flex items-baseline justify-between">
              <span className="text-faint text-[12px] font-bold tracking-[0.06em] uppercase">Low point before {shortDate(lp.next_payday)} paycheck</span>
              <span className="text-faint text-[12px] font-semibold">{lp.days_until_payday} day{lp.days_until_payday === 1 ? "" : "s"}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-[30px] leading-none font-extrabold tracking-[-0.03em]">{money(lp.low_point_cents)}</span>
              <span className={`rounded-[14px] px-3 py-1.5 text-[13px] font-bold ${headroom >= 0 ? "bg-floor text-ink" : "bg-attention text-white"}`}>
                {headroom >= 0 ? `${dollars(headroom)} above floor` : `${dollars(-headroom)} below floor`}
              </span>
            </div>
            <div className="grid grid-cols-3 gap-2">
              <Stat dark label="Bills due" value={`−${dollars(lp.bills_due_cents)}`} />
              <Stat dark label="Everyday pace" value={`−${dollars(lp.everyday_expected_cents)}`} />
              <Stat dark label="Planned" value={`−${dollars(lp.planned_items_cents)}`} />
            </div>
          </div>
        </>
      )}
      <UpdateBalancesSheet open={open} onClose={() => setOpen(false)} balance={balance} today={today} />
    </div>
  );
}

function UpdateBalancesSheet({ open, onClose, balance }: { open: boolean; onClose: () => void; balance: BalanceOverview | undefined; today: Today }) {
  const [values, setValues] = useState<Record<string, number | null>>({});
  const m = useMutation(async () => {
    for (const [account_id, balance_cents] of Object.entries(values)) {
      if (balance_cents != null) await tool("balance.set_reported", { account_id, balance_cents });
    }
  }, ["balance.", "accounts."]);
  const accounts = balance?.accounts ?? [];
  return (
    <Sheet open={open} onClose={onClose} title="Update balances">
      <p className="text-muted mb-3 text-[13px] font-semibold">Type what the bank shows right now. Hearth runs its estimate forward from here.</p>
      <div className="flex flex-col gap-3">
        {accounts.map((a) => (
          <Field key={a.account_id} label={`${a.name}${a.reported_balance_as_of ? ` · reported ${ago(a.reported_balance_as_of)}` : ""}`} hint={`estimate now ${money(a.estimate_cents)}`}>
            <MoneyInput value={values[a.account_id] ?? a.reported_balance_cents ?? null} onChange={(c) => setValues({ ...values, [a.account_id]: c })} allowNegative />
          </Field>
        ))}
        {accounts.length === 0 && <p className="text-muted text-[13px] font-semibold">Add an account under More → Accounts first.</p>}
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || Object.keys(values).length === 0} onClick={async () => { if ((await m.run()) !== undefined) { setValues({}); onClose(); } }}>
          Save
        </button>
      </SheetActions>
    </Sheet>
  );
}

function GroupCard({ group, today, month, isCurrent }: { group: GroupView; today: Today; month: string; isCurrent: boolean }) {
  const income = group.categories.length > 0 && group.categories.every((c) => c.sign === "income");
  const right = income ? (
    <span className="text-income">{dollars(Math.max(0, group.remaining_cents))} still expected</span>
  ) : (
    <span>{dollars(group.remaining_cents)} left of {dollars(group.plan_cents)}</span>
  );
  return (
    <Card className="divider overflow-hidden">
      <CardHead title={group.name} right={right} />
      {group.categories.map((c) => (
        <CategoryRow key={c.id} c={c} today={today} month={month} isCurrent={isCurrent} />
      ))}
    </Card>
  );
}

/** The subtitle, the colors, and the numbers for one row. */
export function describe(c: CategoryView, today: Today, isCurrent: boolean) {
  const income = c.sign === "income";
  const day = today.day_of_month;
  const spent = Math.abs(c.actual_cents);
  const paceCentsPerDay = isCurrent && day > 0 ? Math.round(spent / day) : 0;
  const projected = isCurrent ? paceCentsPerDay * today.days_in_month : spent;
  const over = !income && c.remaining_cents < 0;
  // Pace only means something for everyday spending; bills post in lumps.
  const pacingOver = !income && c.is_everyday && isCurrent && day >= 5 && c.plan_cents > 0 && projected > c.plan_cents * 1.1;
  const parts: string[] = [];
  let attention = false;
  if (income) {
    if (isCurrent) parts.push(`next ${shortDate(today.next_payday)}`);
  } else {
    if (c.bills_total_cents > 0) parts.push(`bills ${dollars(c.bills_total_cents)}/mo`);
    if (c.bills_over_plan_cents > 0) {
      parts.push(`plan short by ${dollars(c.bills_over_plan_cents)}`);
      attention = true;
    }
    if (c.is_everyday && isCurrent && paceCentsPerDay > 0) parts.push(`${dollars(paceCentsPerDay)}/day pace`);
    if (pacingOver) {
      parts.push(`on pace for ${dollars(projected)} of ${dollars(c.plan_cents)}`);
      attention = true;
    }
    if (c.next_phase) parts.push(`${dollars(c.next_phase.plan_cents)} from ${monthShort(c.next_phase.starts_month)}${c.next_phase.label ? ` · ${c.next_phase.label}` : ""}`);
    if (parts.length === 0 && c.note) parts.push(c.note.length > 56 ? `${c.note.slice(0, 56)}…` : c.note);
    if (parts.length === 0 && c.is_everyday) parts.push("everyday");
  }
  return { income, over, pacingOver, attention: !income && (attention || over), subtitle: parts.join(" · "), projected, paceCentsPerDay, spent };
}

function CategoryRow({ c, today, month, isCurrent }: { c: CategoryView; today: Today; month: string; isCurrent: boolean }) {
  const d = describe(c, today, isCurrent);
  const leftColor = d.income ? "text-income" : d.attention ? "text-attention" : "";
  const barColor = d.income ? "income" : d.attention ? "attention" : "ink";
  return (
    <Link to={`/category/${c.id}${isCurrent ? "" : `?month=${month}`}`} className="flex min-h-11 flex-col gap-2 px-4 py-3.5">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[15px] font-bold">{c.name}</span>
          {d.subtitle && <span className={`truncate text-[12px] font-semibold ${d.attention ? "text-attention" : "text-muted"}`}>{d.subtitle}</span>}
        </div>
        <div className="flex shrink-0 items-baseline gap-6">
          <span className="text-muted w-[72px] text-right text-[13px] font-semibold">
            {whole(c.actual_cents)} / {whole(c.plan_cents)}
          </span>
          <span className={`w-16 text-right text-[15px] font-extrabold ${leftColor}`}>
            {d.income ? whole(Math.max(0, c.remaining_cents)) : c.remaining_cents < 0 ? `−${whole(c.remaining_cents)}` : whole(c.remaining_cents)}
          </span>
        </div>
      </div>
      <Progress bps={c.used_bps} color={barColor} />
    </Link>
  );
}

function AddCategorySheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const groups = useGroups();
  const [f, setF] = useState({ name: "", group_id: "", group_name: "", plan: null as number | null, everyday: false, income: false });
  const m = useMutation(
    () =>
      tool("budget.create_category", {
        name: f.name,
        group_id: f.group_id || null,
        group_name: f.group_id ? null : f.group_name || null,
        plan_cents: f.plan,
        is_everyday: f.everyday,
        income: f.income,
      }),
    ["budget."],
  );
  return (
    <Sheet open={open} onClose={onClose} title="Add a category">
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus /></Field>
        <Field label="Group">
          <select className="field" value={f.group_id} onChange={(e) => setF({ ...f, group_id: e.target.value })}>
            <option value="">New group…</option>
            {(groups.data?.groups ?? []).map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
          </select>
        </Field>
        {!f.group_id && <Field label="New group name"><input className="field" value={f.group_name} onChange={(e) => setF({ ...f, group_name: e.target.value })} placeholder="Everyday" /></Field>}
        <Field label="Plan this month"><MoneyInput value={f.plan} onChange={(c) => setF({ ...f, plan: c })} /></Field>
        <Toggle label="Everyday spending" hint="Counts toward the low point at its daily pace." checked={f.everyday} onChange={(v) => setF({ ...f, everyday: v })} />
        <Toggle label="Income" checked={f.income} onChange={(v) => setF({ ...f, income: v })} />
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim() || (!f.group_id && !f.group_name.trim())} onClick={async () => { if ((await m.run()) !== undefined) { setF({ name: "", group_id: "", group_name: "", plan: null, everyday: false, income: false }); onClose(); } }}>
          Add
        </button>
      </SheetActions>
    </Sheet>
  );
}
