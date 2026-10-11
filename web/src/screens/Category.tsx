import { useEffect, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { CategoryPicker } from "../components/CategoryPicker";
import { useAccountName } from "../components/AccountPicker";
import { Card, CardHead, ErrorNote, Field, Icon, MoneyInput, PageHeader, Progress, Sheet, SheetActions, Spinner, Stat, Toggle } from "../components/ui";
import { tool } from "../lib/api";
import { useCategoryDetail, useGroups, useToday } from "../lib/data";
import { addMonths, monthLabel, monthShort, ordinal, shortDate } from "../lib/dates";
import { dollars, money } from "../lib/money";
import { useMutation } from "../lib/query";
import { useSession } from "../lib/session";
import type { CategoryDetail, Today } from "../lib/types";
import { describe } from "./Budget";

export function CategoryScreen() {
  const { id } = useParams();
  const [params] = useSearchParams();
  const today = useToday();
  const month = params.get("month") ?? today.data?.month;
  const detail = useCategoryDetail(id, month);
  const nav = useNavigate();
  const [sheet, setSheet] = useState<null | "plan" | "move" | "ask" | "edit">(null);
  const groups = useGroups();

  if (!today.data || !detail.data) return detail.error ? <ErrorNote error={detail.error.message} /> : <Spinner />;
  const t = today.data;
  const d = detail.data;
  const v = d.view;
  const isCurrent = month === t.month;
  const des = describe(v, t, isCurrent);
  const groupName = groups.data?.groups.find((g) => g.id === d.category.group_id)?.name ?? "";
  const monthPct = isCurrent ? Math.round((t.day_of_month / t.days_in_month) * 100) : 100;
  const planPace = Math.round(v.plan_cents / t.days_in_month);
  const income = v.sign === "income";

  return (
    <>
      <PageHeader
        title={v.name}
        subtitle={`${groupName} · ${monthLabel(month!)}`}
        back={() => nav(-1)}
        right={
          <button type="button" className="icon-btn" aria-label="Edit category" onClick={() => setSheet("edit")}>
            <Icon name="edit" size={18} />
          </button>
        }
      />

      <Card className="flex flex-col gap-3.5 p-5">
        <div className="flex items-end justify-between">
          <div className="flex flex-col gap-1">
            <span className="label">{income ? "Still expected" : "Left this month"}</span>
            <span className={`text-[40px] leading-none font-extrabold tracking-[-0.03em] ${des.attention ? "text-attention" : income ? "text-income" : ""}`}>
              {dollars(income ? Math.max(0, v.remaining_cents) : v.remaining_cents)}
            </span>
          </div>
          <div className="text-muted flex flex-col items-end gap-0.5 text-[13px] font-semibold">
            <span>{income ? "received" : "spent"} {dollars(des.spent)}</span>
            <span>plan {dollars(v.plan_cents)}</span>
          </div>
        </div>
        <Progress bps={v.used_bps} color={income ? "income" : des.attention ? "attention" : "ink"} marker={isCurrent ? monthPct : undefined} thick />
        <div className="text-muted flex justify-between text-[12px] font-semibold">
          <span>{Math.round(v.used_bps / 100)}% used</span>
          {isCurrent && <span className={des.attention ? "text-attention" : ""}>{monthPct}% of the month gone</span>}
        </div>
        {!income && (
          <div className="grid grid-cols-3 gap-2">
            <Stat label="Actual pace" value={isCurrent ? `${dollars(des.paceCentsPerDay)}/day` : "—"} tone={des.pacingOver ? "attention" : undefined} />
            <Stat label="Plan pace" value={`${dollars(planPace)}/day`} />
            <Stat label="Projected" value={dollars(des.projected)} tone={des.pacingOver ? "attention" : undefined} />
          </div>
        )}
        {v.bills_total_cents > 0 && (
          <p className={`text-[12px] font-semibold ${v.bills_over_plan_cents > 0 ? "text-attention" : "text-muted"}`}>
            Bills in this category add up to {dollars(v.bills_total_cents)}/mo
            {v.bills_over_plan_cents > 0 ? `, ${dollars(v.bills_over_plan_cents)} more than the plan.` : "."}
          </p>
        )}
      </Card>

      <div className="grid grid-cols-3 gap-2">
        <button type="button" className="btn-dark h-12 rounded-2xl" onClick={() => setSheet("plan")}>Set plan</button>
        <button type="button" className="btn-light h-12 rounded-2xl" onClick={() => setSheet("move")}>Move money</button>
        <button type="button" className="btn-light h-12 rounded-2xl" onClick={() => setSheet("ask")}>Ask Claude</button>
      </div>

      <PlanPhases d={d} today={t} />
      <NoteCard d={d} />

      {d.bills.length > 0 && (
        <Card className="divider overflow-hidden">
          <CardHead title="Bills here" right={`${dollars(v.bills_total_cents)}/mo`} />
          {d.bills.map((b) => (
            <div key={b.id} className="row">
              <div className="flex flex-col gap-0.5">
                <span className="text-[15px] font-bold">{b.name}</span>
                <span className="text-muted text-[12px] font-semibold">
                  {b.cadence_months === 1 ? `due ${ordinal(Number(b.next_due.slice(8)))}` : `every ${b.cadence_months === 12 ? "year" : `${b.cadence_months} months`} · next ${shortDate(b.next_due)}`}
                  {b.status !== "active" && ` · ${b.status}`}
                </span>
              </div>
              <span className="text-[15px] font-extrabold">{money(b.amount_cents)}</span>
            </div>
          ))}
        </Card>
      )}

      <TransactionsCard d={d} month={month!} />

      <SetPlanSheet open={sheet === "plan"} onClose={() => setSheet(null)} d={d} today={t} month={month!} />
      <MoveMoneySheet open={sheet === "move"} onClose={() => setSheet(null)} d={d} month={month!} />
      <AskClaudeSheet open={sheet === "ask"} onClose={() => setSheet(null)} d={d} today={t} isCurrent={isCurrent} />
      <EditCategorySheet open={sheet === "edit"} onClose={() => setSheet(null)} d={d} />
    </>
  );
}

function PlanPhases({ d, today }: { d: CategoryDetail; today: Today }) {
  const plans = [...d.plans].sort((a, b) => a.starts_month.localeCompare(b.starts_month));
  const current = [...plans].reverse().find((p) => p.starts_month <= today.month);
  const future = plans.filter((p) => p.starts_month > today.month);
  const del = useMutation((plan_id: string) => tool("budget.delete_plan", { plan_id }), ["budget."]);
  const owner = useSession().me?.role === "owner";
  if (plans.length === 0) return null;
  return (
    <Card className="divider overflow-hidden">
      <CardHead title="Plan" />
      {current && (
        <div className="row">
          <div className="flex flex-col gap-0.5">
            <span className="text-[15px] font-bold">{current.label || "Now"}</span>
            <span className="text-muted text-[12px] font-semibold">since {monthShort(current.starts_month)}</span>
          </div>
          <span className="text-[15px] font-extrabold">{dollars(current.amount_cents)} / mo</span>
        </div>
      )}
      {future.map((p) => (
        <div key={p.id} className="row">
          <div className="flex flex-col gap-0.5">
            <span className="text-[15px] font-bold">{p.label || "Next"}</span>
            <span className="text-muted text-[12px] font-semibold">from {monthShort(p.starts_month)}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-muted text-[15px] font-extrabold">{dollars(p.amount_cents)} / mo</span>
            {owner && <button type="button" className="text-muted text-[12px] font-bold underline" onClick={() => del.run(p.id)} disabled={del.busy}>remove</button>}
          </div>
        </div>
      ))}
      {del.error && <div className="px-4 pb-3"><ErrorNote error={del.error} /></div>}
    </Card>
  );
}

function NoteCard({ d }: { d: CategoryDetail }) {
  const [note, setNote] = useState(d.category.note);
  useEffect(() => setNote(d.category.note), [d.category.note]);
  const m = useMutation(() => tool("budget.set_category_note", { category_id: d.category.id, note }), ["budget."]);
  const dirty = note !== d.category.note;
  return (
    <Card className="flex flex-col gap-2 px-4 py-3.5">
      <div className="flex items-center justify-between">
        <span className="text-[13px] font-extrabold">Standing note</span>
        <span className="text-muted text-[11px] font-bold">read by Claude and the alert rules</span>
      </div>
      <textarea
        aria-label="Standing note"
        className="field min-h-20 resize-none leading-snug"
        rows={3}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder="How this category works, and anything Claude should know. [no-alerts] [no-large-charge] [no-pace] are honored by the rules."
      />
      {dirty && (
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-light h-9" onClick={() => setNote(d.category.note)}>Discard</button>
          <button type="button" className="btn-dark h-9" disabled={m.busy} onClick={() => m.run()}>Save note</button>
        </div>
      )}
      <ErrorNote error={m.error} />
    </Card>
  );
}

function TransactionsCard({ d, month }: { d: CategoryDetail; month: string }) {
  const accountName = useAccountName();
  const txs = d.transactions;
  return (
    <Card className="divider overflow-hidden">
      <CardHead title={monthShort(month)} right={`${txs.length} transaction${txs.length === 1 ? "" : "s"}`} />
      {txs.length === 0 && <div className="text-muted px-4 py-6 text-center text-[13px] font-semibold">Nothing here yet.</div>}
      {txs.map((x) => (
        <div key={x.id} className="row">
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="truncate text-[15px] font-bold">{x.description}</span>
            <span className="text-muted truncate text-[12px] font-semibold">
              {shortDate(x.date)}
              {x.pending && " · pending"}
              {` · ${x.source === "manual" ? "manual" : x.source === "csv" ? "import" : accountName(x.account_id) || x.source}`}
              {x.rule_id && " · by rule"}
              {x.recurring_bill_id && " · bill"}
              {x.note && ` · ${x.note}`}
            </span>
          </div>
          <span className={`shrink-0 text-[15px] font-extrabold ${x.amount_cents > 0 ? "text-income" : ""}`}>{money(x.amount_cents)}</span>
        </div>
      ))}
    </Card>
  );
}

function SetPlanSheet({ open, onClose, d, today, month }: { open: boolean; onClose: () => void; d: CategoryDetail; today: Today; month: string }) {
  const [amount, setAmount] = useState<number | null>(d.view.plan_cents);
  const [starts, setStarts] = useState(month);
  const [label, setLabel] = useState("");
  useEffect(() => { setAmount(d.view.plan_cents); setStarts(month); }, [d.view.plan_cents, month, open]);
  const m = useMutation(() => tool("budget.set_plan", { category_id: d.category.id, amount_cents: amount, starts_month: starts, label: label || null }), ["budget."]);
  const options = [month, addMonths(month, 1), addMonths(month, 2), addMonths(month, 3)].filter((x, i, a) => a.indexOf(x) === i && x >= today.month);
  return (
    <Sheet open={open} onClose={onClose} title={`Plan for ${d.view.name}`}>
      <div className="flex flex-col gap-3">
        <Field label="Amount per month"><MoneyInput value={amount} onChange={setAmount} autoFocus /></Field>
        <Field label="Starting" hint="A plan runs from its start month until the next phase.">
          <select className="field" value={starts} onChange={(e) => setStarts(e.target.value)}>
            {options.map((o) => <option key={o} value={o}>{monthLabel(o)}{o === today.month ? " (now)" : ""}</option>)}
          </select>
        </Field>
        {starts !== today.month && <Field label="Label (optional)"><input className="field" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="After the move" /></Field>}
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || amount == null} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>Save plan</button>
      </SheetActions>
    </Sheet>
  );
}

function MoveMoneySheet({ open, onClose, d, month }: { open: boolean; onClose: () => void; d: CategoryDetail; month: string }) {
  const [to, setTo] = useState<string | null>(null);
  const [amount, setAmount] = useState<number | null>(null);
  const [direction, setDirection] = useState<"out" | "in">("out");
  const m = useMutation(async () => {
    if (!to || !amount) return;
    const other = await tool<CategoryDetail>("budget.category", { id: to, month });
    const sign = direction === "out" ? -1 : 1;
    await tool("budget.set_plan", { category_id: d.category.id, amount_cents: Math.max(0, d.view.plan_cents + sign * amount), starts_month: month });
    await tool("budget.set_plan", { category_id: to, amount_cents: Math.max(0, other.view.plan_cents - sign * amount), starts_month: month });
  }, ["budget."]);
  return (
    <Sheet open={open} onClose={onClose} title="Move money">
      <p className="text-muted mb-3 text-[13px] font-semibold">Shifts this month's plan between two categories. Totals stay the same.</p>
      <div className="flex flex-col gap-3">
        <Field label="Direction">
          <select className="field" value={direction} onChange={(e) => setDirection(e.target.value as "out" | "in")}>
            <option value="out">From {d.view.name} to…</option>
            <option value="in">Into {d.view.name} from…</option>
          </select>
        </Field>
        <Field label="Other category"><CategoryPicker value={to} onChange={setTo} /></Field>
        <Field label="Amount"><MoneyInput value={amount} onChange={setAmount} /></Field>
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !to || !amount} onClick={async () => { if ((await m.run()) !== undefined) { setAmount(null); onClose(); } }}>Move</button>
      </SheetActions>
    </Sheet>
  );
}

function AskClaudeSheet({ open, onClose, d, today, isCurrent }: { open: boolean; onClose: () => void; d: CategoryDetail; today: Today; isCurrent: boolean }) {
  const v = d.view;
  const line = `${v.name}: ${dollars(v.actual_cents)} of ${dollars(v.plan_cents)} ${v.sign === "income" ? "received" : "spent"}${isCurrent ? `, day ${today.day_of_month} of ${today.days_in_month}` : ""}${v.note ? `. Note: ${v.note}` : ""}`;
  const [copied, setCopied] = useState(false);
  const api = today.assistant.mode === "api";
  return (
    <Sheet open={open} onClose={onClose} title="Ask Claude">
      {api ? (
        <p className="text-muted text-[13px] font-semibold">Open the chat with this category's numbers already in the message.</p>
      ) : (
        <p className="text-muted text-[13px] font-semibold">Hearth is in Pull mode: open the Claude app and ask there. Claude reads this category through the Hearth connector. Paste this line to give it the context in one go:</p>
      )}
      <div className="bg-card border-line mt-3 rounded-xl border px-3 py-2 text-[13px] font-semibold">{line}</div>
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Close</button>
        {api ? (
          <Link to={`/chat?q=${encodeURIComponent(line + " — ")}`} className="btn-dark">Open chat</Link>
        ) : (
          <button type="button" className="btn-dark" onClick={async () => { try { await navigator.clipboard.writeText(line); setCopied(true); } catch { setCopied(false); } }}>{copied ? "Copied" : "Copy"}</button>
        )}
      </SheetActions>
    </Sheet>
  );
}

function EditCategorySheet({ open, onClose, d }: { open: boolean; onClose: () => void; d: CategoryDetail }) {
  const groups = useGroups();
  const [f, setF] = useState({ name: d.category.name, group_id: d.category.group_id, is_everyday: d.category.is_everyday, is_archived: d.category.is_archived });
  useEffect(() => setF({ name: d.category.name, group_id: d.category.group_id, is_everyday: d.category.is_everyday, is_archived: d.category.is_archived }), [d.category, open]);
  const nav = useNavigate();
  const m = useMutation(() => tool("budget.update_category", { category_id: d.category.id, ...f }), ["budget."]);
  return (
    <Sheet open={open} onClose={onClose} title="Edit category">
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
        <Field label="Group">
          <select className="field" value={f.group_id} onChange={(e) => setF({ ...f, group_id: e.target.value })}>
            {(groups.data?.groups ?? []).map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
          </select>
        </Field>
        <Toggle label="Everyday spending" hint="Counts toward the low point at its daily pace." checked={f.is_everyday} onChange={(v) => setF({ ...f, is_everyday: v })} />
        <Toggle label="Archived" hint="Hidden from the budget; history stays." checked={f.is_archived} onChange={(v) => setF({ ...f, is_archived: v })} />
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim()} onClick={async () => { if ((await m.run()) !== undefined) { onClose(); if (f.is_archived) nav("/"); } }}>Save</button>
      </SheetActions>
    </Sheet>
  );
}
