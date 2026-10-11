import { useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { CategoryPicker, useCategoryName } from "../components/CategoryPicker";
import { useAccountName } from "../components/AccountPicker";
import { Card, Empty, ErrorNote, Field, Icon, MoneyInput, PageHeader, SectionTitle, Sheet, SheetActions, Spinner, Tabs } from "../components/ui";
import { api, tool } from "../lib/api";
import { useActions, useAlerts, useGroups, useQuestions, useReviewQueue, useRules, useToday, useTransactions } from "../lib/data";
import { ago, addDays, longDate, shortDate, todayIso } from "../lib/dates";
import { dollars, money, signed } from "../lib/money";
import { invalidate, useMutation } from "../lib/query";
import type { Alert, AssistantAction, Question, ReviewItem, Rule, Transaction } from "../lib/types";

type Tab = "inbox" | "all" | "done";

export function Activity() {
  const [params, setParams] = useSearchParams();
  const tab = (params.get("tab") as Tab) || "inbox";
  const alerts = useAlerts({ live: true });
  const questions = useQuestions({ status: "open" });
  const review = useReviewQueue();
  const nAlerts = alerts.data?.length ?? 0;
  const nQ = questions.data?.length ?? 0;
  const nR = review.data?.length ?? 0;
  return (
    <>
      <PageHeader
        title={tab === "inbox" ? "Needs a look" : tab === "all" ? "All activity" : "Done"}
        subtitle={tab === "inbox" ? `${nAlerts} alert${nAlerts === 1 ? "" : "s"} · ${nQ} question${nQ === 1 ? "" : "s"} · ${nR} to categorize` : undefined}
      />
      <Tabs value={tab} onChange={(t) => setParams(t === "inbox" ? {} : { tab: t })} items={[{ key: "inbox", label: "Inbox" }, { key: "all", label: "All activity" }, { key: "done", label: "Done" }]} />
      {tab === "inbox" && <Inbox alerts={alerts.data} questions={questions.data} review={review.data} loading={alerts.loading || questions.loading || review.loading} />}
      {tab === "all" && <AllActivity />}
      {tab === "done" && <Done />}
    </>
  );
}

function Inbox({ alerts, questions, review, loading }: { alerts?: Alert[]; questions?: Question[]; review?: ReviewItem[]; loading: boolean }) {
  const actions = useActions({ undoable_only: true, limit: 10 });
  const undoable = (actions.data ?? []).filter((a) => a.undo && !a.undone_at);
  if (loading && !alerts) return <Spinner />;
  const empty = !alerts?.length && !questions?.length && !review?.length && !undoable.length;
  return (
    <>
      {empty && <Card><Empty>Nothing needs a look. Enjoy the quiet.</Empty></Card>}
      {alerts?.map((a) => <AlertCard key={a.id} a={a} />)}
      <div id="questions" />
      {questions?.map((q) => <QuestionCard key={q.id} q={q} />)}
      {undoable.length > 0 && (
        <>
          <SectionTitle title="Claude did…" right="undo available" />
          {undoable.map((a) => <ActionCard key={a.id} a={a} />)}
        </>
      )}
      <div id="review" />
      {!!review?.length && <ReviewSection items={review} />}
    </>
  );
}

const TYPE_LABEL: Record<string, string> = {
  unplanned_large_charge: "large charge",
  canceled_bill_charged: "canceled service charged",
  bill_amount_changed: "bill amount changed",
  bill_not_seen: "bill not seen",
  category_blown: "over plan",
  category_pace: "pace",
  floor_breached: "below the floor",
  debt_redraw: "debt redraw",
  card_declined: "card declined",
  planned_item_missing: "planned item missing",
  balance_stale: "stale balance",
  sync_failed: "sync failed",
  review_queue_backlog: "review backlog",
  milestone: "milestone",
};

function AlertCard({ a }: { a: Alert }) {
  const [resolving, setResolving] = useState(false);
  const [resolution, setResolution] = useState("");
  const ack = useMutation(() => tool("alerts.ack", { alert_id: a.id }), ["alerts."]);
  const snooze = useMutation(() => tool("alerts.snooze", { alert_id: a.id, until: `${addDays(todayIso(), 1)}T09:00:00` }), ["alerts."]);
  const resolve = useMutation(() => tool("alerts.resolve", { alert_id: a.id, resolution }), ["alerts."]);
  const critical = a.severity === "critical";
  const acked = a.status === "ack";
  return (
    <Card tint={critical ? "critical" : undefined} className="flex flex-col gap-3 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <span className={`label ${critical ? "text-attention" : ""}`}>Alert · {TYPE_LABEL[a.type] ?? a.type.replace(/_/g, " ")}{acked ? " · acknowledged" : ""}{a.snoozed_until ? " · snoozed" : ""}</span>
          <span className="text-[15px] leading-snug font-bold">{a.title}</span>
          {a.detail && <span className="text-muted text-[13px] leading-snug font-medium">{a.detail}</span>}
        </div>
        {a.amount_cents != null && <span className="shrink-0 text-[15px] font-extrabold">{money(a.amount_cents)}</span>}
      </div>
      {resolving ? (
        <div className="flex flex-col gap-2">
          <input className="field" autoFocus placeholder="What happened?" value={resolution} onChange={(e) => setResolution(e.target.value)} />
          <div className="flex gap-2">
            <button type="button" className="btn-dark" disabled={!resolution.trim() || resolve.busy} onClick={() => resolve.run()}>Mark done</button>
            <button type="button" className="btn-light" onClick={() => setResolving(false)}>Back</button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          {!acked && <button type="button" className="btn-dark" disabled={ack.busy} onClick={() => ack.run()}>Acknowledge</button>}
          <button type="button" className="btn-light" onClick={() => setResolving(true)}>Resolve…</button>
          {!a.snoozed_until && <button type="button" className="btn-light" disabled={snooze.busy} onClick={() => snooze.run()}>Snooze</button>}
          {a.category_id && <Link to={`/category/${a.category_id}`} className="btn-light">Open category</Link>}
        </div>
      )}
      <ErrorNote error={ack.error || snooze.error || resolve.error} />
    </Card>
  );
}

function QuestionCard({ q }: { q: Question }) {
  const [typing, setTyping] = useState(false);
  const [text, setText] = useState("");
  const [editing, setEditing] = useState(false);
  const answer = useMutation((a: string) => tool("questions.answer", { question_id: q.id, answer: a }), ["questions.", "alerts.", "transactions."]);
  const dismiss = useMutation(() => tool("questions.dismiss", { question_id: q.id }), ["questions."]);
  const approve = useMutation((args?: Record<string, unknown>) => api.post(`/api/proposals/${q.id}/approve`, args ? { args } : {}), [""]);
  const decline = useMutation(() => api.post(`/api/proposals/${q.id}/decline`, {}), ["questions."]);
  const proposal = q.kind === "proposal" && q.proposal;
  return (
    <Card tint="question" className="flex flex-col gap-3 p-4">
      <div className="flex flex-col gap-1">
        <span className="label text-tint-ink">{proposal ? "Proposal from Claude" : "Question from Claude"}{q.area ? ` · ${q.area.replace(/_/g, " & ")}` : ""} · {ago(q.created_at)}</span>
        <span className="text-[15px] leading-snug font-bold">{q.question}</span>
        {proposal && <span className="bg-card border-tint-line mt-1 rounded-xl border px-3 py-2 text-[13px] font-bold">{proposal.summary}</span>}
      </div>
      {proposal ? (
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn-dark" disabled={approve.busy} onClick={() => approve.run()}>Approve</button>
          <button type="button" className="btn-tint" onClick={() => setEditing(true)}>Edit…</button>
          <button type="button" className="btn-tint" disabled={decline.busy} onClick={() => decline.run()}>Decline</button>
        </div>
      ) : typing ? (
        <div className="flex flex-col gap-2">
          <input className="field" autoFocus placeholder="Your answer" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && text.trim()) answer.run(text.trim()); }} />
          <div className="flex gap-2">
            <button type="button" className="btn-dark" disabled={!text.trim() || answer.busy} onClick={() => answer.run(text.trim())}>Send</button>
            <button type="button" className="btn-tint" onClick={() => setTyping(false)}>Back</button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          {(q.options ?? []).map((o, i) => (
            <button key={o.key} type="button" className={i === 0 ? "btn-dark" : "btn-tint"} disabled={answer.busy} onClick={() => answer.run(o.label)}>{o.label}</button>
          ))}
          <button type="button" className="btn-tint" onClick={() => setTyping(true)}>Type an answer…</button>
          <button type="button" className="text-muted px-2 text-[12px] font-bold underline" disabled={dismiss.busy} onClick={() => dismiss.run()}>Dismiss</button>
        </div>
      )}
      <ErrorNote error={answer.error || dismiss.error || approve.error || decline.error} />
      {proposal && <EditProposalSheet open={editing} onClose={() => setEditing(false)} q={q} onApprove={(args) => approve.run(args)} />}
    </Card>
  );
}

function EditProposalSheet({ open, onClose, q, onApprove }: { open: boolean; onClose: () => void; q: Question; onApprove: (args: Record<string, unknown>) => Promise<unknown> }) {
  const [args, setArgs] = useState<Record<string, unknown>>(q.proposal?.args ?? {});
  const catName = useCategoryName();
  const entries = Object.entries(args).filter(([k]) => !k.endsWith("_id") || k === "category_id");
  return (
    <Sheet open={open} onClose={onClose} title="Edit before approving">
      <p className="text-muted mb-3 text-[13px] font-semibold">{q.proposal?.tool}</p>
      <div className="flex flex-col gap-3">
        {entries.map(([k, v]) => (
          <Field key={k} label={k.replace(/_cents$/, "").replace(/_/g, " ")}>
            {k.endsWith("_cents") ? (
              <MoneyInput value={typeof v === "number" ? v : null} onChange={(c) => setArgs({ ...args, [k]: c })} allowNegative />
            ) : k === "category_id" ? (
              <CategoryPicker value={typeof v === "string" ? v : null} onChange={(id) => setArgs({ ...args, [k]: id })} />
            ) : typeof v === "boolean" ? (
              <select className="field" value={String(v)} onChange={(e) => setArgs({ ...args, [k]: e.target.value === "true" })}><option value="true">yes</option><option value="false">no</option></select>
            ) : (
              <input className="field" value={v == null ? "" : String(v)} onChange={(e) => setArgs({ ...args, [k]: typeof v === "number" ? Number(e.target.value) : e.target.value })} />
            )}
          </Field>
        ))}
        {entries.some(([k]) => k === "category_id") && <span className="text-faint text-[11px] font-semibold">Category: {catName(String(args.category_id ?? ""))}</span>}
      </div>
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" onClick={async () => { await onApprove(args); onClose(); }}>Approve with edits</button>
      </SheetActions>
    </Sheet>
  );
}

function ActionCard({ a }: { a: AssistantAction }) {
  const undo = useMutation(() => api.post(`/api/actions/${a.id}/undo`, {}), [""]);
  const canUndo = !!a.undo && !a.undone_at;
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <span className="label">
            <Icon name="sparkle" size={12} className="mr-1 inline" />
            {a.tool === "assistant.summary" ? "Claude's pass" : "Claude did"} · {ago(a.created_at)}
            {a.undone_at && " · undone"}
          </span>
          <span className="text-[15px] leading-snug font-bold">{a.summary}</span>
          {a.tool !== "assistant.summary" && <span className="text-muted text-[12px] font-semibold">{a.tool}{a.area ? ` · ${a.area.replace(/_/g, " & ")}` : ""}</span>}
        </div>
        {canUndo && (
          <button type="button" className="btn-light h-9 shrink-0" disabled={undo.busy} onClick={() => undo.run()}>
            <Icon name="undo" size={14} /> Undo
          </button>
        )}
      </div>
      <ErrorNote error={undo.error} />
    </Card>
  );
}

/** Rules that would match a description, client-side, for suggestion chips. */
function matchRules(rules: Rule[], description: string): Rule[] {
  const d = description.toLowerCase();
  return rules.filter((r) => {
    if (!r.is_active || r.action !== "categorize" || !r.category_id) return false;
    if (r.match_type === "contains") return d.includes(r.pattern.toLowerCase());
    try {
      return new RegExp(r.pattern, "i").test(description);
    } catch {
      return false;
    }
  });
}

function ReviewSection({ items }: { items: ReviewItem[] }) {
  const rules = useRules();
  const groups = useGroups();
  const today = useToday();
  const [showAll, setShowAll] = useState(false);
  const [askOpen, setAskOpen] = useState(false);
  const list = showAll ? items : items.slice(0, 5);
  const oldest = items.reduce((m, x) => (x.date < m ? x.date : m), items[0]?.date ?? todayIso());
  const everyday = (groups.data?.categories ?? []).filter((c) => !c.is_archived && c.is_everyday);
  const income = (groups.data?.categories ?? []).filter((c) => !c.is_archived && c.sign === "income");
  const pull = today.data?.assistant.mode !== "api";
  return (
    <>
      <SectionTitle title="To categorize" right={`no rule matched · oldest ${shortDate(oldest)}`} />
      <Card className="divider overflow-hidden">
        {list.map((x) => {
          const hits = matchRules(rules.data ?? [], x.description);
          const sugg: { id: string; name: string }[] = [];
          for (const r of hits) if (r.category_id && !sugg.some((s) => s.id === r.category_id)) sugg.push({ id: r.category_id, name: (groups.data?.categories ?? []).find((c) => c.id === r.category_id)?.name ?? r.name });
          for (const c of x.amount_cents > 0 ? income : everyday) if (sugg.length < 3 && !sugg.some((s) => s.id === c.id)) sugg.push({ id: c.id, name: c.name });
          return <ReviewRow key={x.id} x={x} suggestions={sugg} />;
        })}
        <div className="row">
          <span className="text-muted text-[13px] font-bold">{items.length > list.length ? `${items.length - list.length} more` : `${items.length} total`}</span>
          <div className="flex gap-2">
            {items.length > list.length && <button type="button" className="chip-light" onClick={() => setShowAll(true)}>Show all</button>}
            <button type="button" className="chip bg-line text-ink" onClick={() => setAskOpen(true)}>Let Claude take a pass</button>
          </div>
        </div>
      </Card>
      <Sheet open={askOpen} onClose={() => setAskOpen(false)} title="Let Claude take a pass">
        {pull ? (
          <p className="text-muted text-[13px] font-semibold">Hearth is in Pull mode: Claude works the queue when its recurring task runs in the Claude app. To do it now, open the Claude app and say "take a Hearth pass". The categorizing area is {today.data?.assistant.categorizing_notes ?? today.data?.assistant.autonomy} mode.</p>
        ) : (
          <p className="text-muted text-[13px] font-semibold">The API-mode pass runs on its schedule; running it on demand from here lands with the chat panel in a later step.</p>
        )}
        <SheetActions><button type="button" className="btn-dark" onClick={() => setAskOpen(false)}>OK</button></SheetActions>
      </Sheet>
    </>
  );
}

function ReviewRow({ x, suggestions }: { x: ReviewItem; suggestions: { id: string; name: string }[] }) {
  const accountName = useAccountName();
  const [other, setOther] = useState(false);
  const [rule, setRule] = useState(false);
  const [cat, setCat] = useState<string | null>(null);
  const categorize = useMutation((category_id: string) => tool("transactions.categorize", { transaction_id: x.id, category_id }), ["transactions.", "budget.", "alerts."]);
  return (
    <div className="flex flex-col gap-2.5 px-4 py-3.5">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[15px] font-bold">{x.description}</span>
          <span className="text-muted text-[12px] font-semibold">
            {shortDate(x.date)}{x.pending && " · pending"} · {x.account_name ?? accountName(x.account_id) ?? x.source}
          </span>
        </div>
        <span className={`shrink-0 text-[15px] font-extrabold ${x.amount_cents > 0 ? "text-income" : ""}`}>{signed(x.amount_cents)}</span>
      </div>
      {other ? (
        <div className="flex gap-2">
          <CategoryPicker value={cat} onChange={setCat} autoFocus />
          <button type="button" className="btn-dark shrink-0" disabled={!cat || categorize.busy} onClick={() => cat && categorize.run(cat)}>Set</button>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          {suggestions.map((s, i) => (
            <button key={s.id} type="button" className={i === 0 ? "chip-dark" : "chip-light"} disabled={categorize.busy} onClick={() => categorize.run(s.id)}>{s.name}</button>
          ))}
          <button type="button" className="chip-light" onClick={() => setOther(true)}>Other…</button>
          <button type="button" className="chip-dashed" onClick={() => setRule(true)}>+ make a rule</button>
        </div>
      )}
      <ErrorNote error={categorize.error} />
      <RuleSheet open={rule} onClose={() => setRule(false)} seed={x} />
    </div>
  );
}

export function RuleSheet({ open, onClose, seed, rule }: { open: boolean; onClose: () => void; seed?: Transaction; rule?: Rule }) {
  const [f, setF] = useState(() => ({
    pattern: rule?.pattern ?? (seed ? seed.description.toLowerCase().replace(/[#*]\s*\d+.*$/, "").replace(/\s+\d{3,}.*$/, "").trim() : ""),
    category_id: rule?.category_id ?? null,
    regex: rule?.match_type === "regex",
    name: rule?.name ?? "",
    set_note: rule?.set_note ?? "",
    ignore: rule?.action === "ignore",
  }));
  const m = useMutation(
    () =>
      rule
        ? tool("rules.update", { rule_id: rule.id, pattern: f.pattern, category_id: f.category_id, regex: f.regex, name: f.name || null, set_note: f.set_note || null })
        : tool("rules.create", { pattern: f.pattern, category_id: f.ignore ? null : f.category_id, regex: f.regex, name: f.name || null, set_note: f.set_note || null, ignore: f.ignore, apply_now: true }),
    ["rules.", "transactions.", "budget."],
  );
  const del = useMutation(() => tool("rules.delete", { rule_id: rule!.id }), ["rules."]);
  return (
    <Sheet open={open} onClose={onClose} title={rule ? "Edit rule" : "Make a rule"}>
      <div className="flex flex-col gap-3">
        <Field label={f.regex ? "Regular expression" : "Description contains"} hint="Matched against the normalized description, case-insensitive.">
          <input className="field" value={f.pattern} onChange={(e) => setF({ ...f, pattern: e.target.value })} autoFocus />
        </Field>
        <div className="flex gap-2">
          <button type="button" className={f.regex ? "chip-light" : "chip-dark"} onClick={() => setF({ ...f, regex: false })}>contains</button>
          <button type="button" className={f.regex ? "chip-dark" : "chip-light"} onClick={() => setF({ ...f, regex: true })}>regex</button>
          {!rule && <button type="button" className={f.ignore ? "chip-dark" : "chip-light"} onClick={() => setF({ ...f, ignore: !f.ignore })}>ignore matches</button>}
        </div>
        {!f.ignore && <Field label="Category"><CategoryPicker value={f.category_id} onChange={(id) => setF({ ...f, category_id: id })} /></Field>}
        <Field label="Name (optional)"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
        <Field label="Set a note (optional)"><input className="field" value={f.set_note} onChange={(e) => setF({ ...f, set_note: e.target.value })} /></Field>
      </div>
      <ErrorNote error={m.error || del.error} />
      <SheetActions>
        {rule && <button type="button" className="btn-light text-attention" disabled={del.busy} onClick={async () => { if ((await del.run()) !== undefined) onClose(); }}>Delete</button>}
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.pattern.trim() || (!f.ignore && !f.category_id)} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>{rule ? "Save" : "Create and apply"}</button>
      </SheetActions>
    </Sheet>
  );
}

function AllActivity() {
  const actions = useActions({ limit: 30 });
  const [cursor, setCursor] = useState<string | null>(null);
  const page = useTransactions({ limit: 30, cursor });
  const catName = useCategoryName();
  const accountName = useAccountName();
  const [pages, setPages] = useState<Transaction[]>([]);
  const items = useMemo(() => {
    const all = [...pages, ...(page.data?.items ?? [])];
    const seen = new Set<string>();
    return all.filter((t) => (seen.has(t.id) ? false : (seen.add(t.id), true)));
  }, [pages, page.data]);
  return (
    <>
      <SectionTitle title="Claude" right="passes and actions" />
      {actions.data?.length ? actions.data.map((a) => <ActionCard key={a.id} a={a} />) : <Card><Empty>Claude hasn't done anything yet.</Empty></Card>}
      <SectionTitle title="Transactions" right="newest first" />
      <Card className="divider overflow-hidden">
        {items.map((x) => (
          <div key={x.id} className="row">
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate text-[15px] font-bold">{x.description}</span>
              <span className="text-muted truncate text-[12px] font-semibold">
                {shortDate(x.date)}{x.pending && " · pending"} · {catName(x.category_id)} · {accountName(x.account_id)}{x.note && ` · ${x.note}`}
              </span>
            </div>
            <span className={`shrink-0 text-[15px] font-extrabold ${x.amount_cents > 0 ? "text-income" : ""}`}>{money(x.amount_cents)}</span>
          </div>
        ))}
        {page.loading && <Spinner />}
        {page.data?.next_cursor && (
          <button type="button" className="text-muted w-full py-3 text-[13px] font-bold" onClick={() => { setPages(items); setCursor(page.data!.next_cursor); }}>Load more</button>
        )}
      </Card>
    </>
  );
}

function Done() {
  const alerts = useAlerts({ status: "done", limit: 30 });
  const qs = useQuestions({ limit: 50 });
  const done = (qs.data ?? []).filter((q) => q.status !== "open");
  const reopen = useMutation((id: string) => tool("alerts.reopen", { alert_id: id }), ["alerts."]);
  if (alerts.loading && !alerts.data) return <Spinner />;
  return (
    <>
      <SectionTitle title="Resolved alerts" />
      <Card className="divider overflow-hidden">
        {!alerts.data?.length && <Empty>No resolved alerts yet.</Empty>}
        {alerts.data?.map((a) => (
          <div key={a.id} className="row">
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate text-[15px] font-bold">{a.title}</span>
              <span className="text-muted truncate text-[12px] font-semibold">{a.resolution ? `“${a.resolution}”` : "resolved"} · {longDate(a.updated_at)}</span>
            </div>
            <button type="button" className="chip-light shrink-0" onClick={() => reopen.run(a.id)}>Reopen</button>
          </div>
        ))}
      </Card>
      <SectionTitle title="Answered questions" />
      <Card className="divider overflow-hidden">
        {!done.length && <Empty>No answered questions yet.</Empty>}
        {done.map((q) => (
          <div key={q.id} className="flex flex-col gap-0.5 px-4 py-3">
            <span className="text-[15px] font-bold">{q.question}</span>
            <span className="text-muted text-[12px] font-semibold">
              {q.kind === "proposal" ? q.status : q.answer ? `“${q.answer}”` : q.status}{q.answered_by ? ` · ${q.answered_by}` : ""} · {ago(q.answered_at ?? q.created_at)}
            </span>
          </div>
        ))}
      </Card>
      <button type="button" className="text-muted text-[12px] font-bold underline" onClick={() => invalidate("alerts.", "questions.")}>Refresh</button>
      <span className="hidden">{dollars(0)}</span>
    </>
  );
}
