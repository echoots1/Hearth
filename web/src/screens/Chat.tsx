import { useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { Card, Empty, ErrorNote, Icon, PageHeader, Sheet, Spinner } from "../components/ui";
import { api } from "../lib/api";
import { useToday } from "../lib/data";
import { ago } from "../lib/dates";
import { money } from "../lib/money";
import { invalidate, useQuery } from "../lib/query";

type Thread = { id: string; title: string; updated_at: string };
type Block = { type: string; text?: string; name?: string; input?: unknown };
type Msg = { id: string; role: "user" | "assistant"; content: Block[] | string; text: string; cost_cents: number; created_at: string };

function toolsOf(m: Msg): string[] {
  if (!Array.isArray(m.content)) return [];
  return m.content.filter((b) => b.type === "tool_use" && b.name).map((b) => b.name!.replace("__", "."));
}

export function Chat() {
  const today = useToday();
  const [params, setParams] = useSearchParams();
  const threadId = params.get("t");
  const threads = useQuery<Thread[]>("chat.threads", () => api.get("/api/chat/threads"));
  const thread = useQuery<{ thread: Thread; messages: Msg[] }>(threadId ? `chat.thread:${threadId}` : null, () => api.get(`/api/chat/threads/${threadId}`));
  const [text, setText] = useState(params.get("q") ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [list, setList] = useState(false);
  const bottom = useRef<HTMLDivElement>(null);
  const apiMode = today.data?.assistant.mode === "api";

  useEffect(() => {
    // Open the newest thread, or start one, when none is selected.
    if (threadId || !threads.data) return;
    if (threads.data.length) setParams({ t: threads.data[0].id, ...(params.get("q") ? { q: params.get("q")! } : {}) }, { replace: true });
    else void newThread();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threads.data, threadId]);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [thread.data?.messages.length, busy]);

  const newThread = async () => {
    const t = await api.post<Thread>("/api/chat/threads", {});
    invalidate("chat.threads");
    setParams({ t: t.id });
    setList(false);
  };

  const send = async () => {
    if (!threadId || !text.trim()) return;
    setBusy(true);
    setError(null);
    const msg = text.trim();
    setText("");
    try {
      await api.post(`/api/chat/threads/${threadId}/messages`, { text: msg });
      invalidate("chat.", "budget.", "transactions.", "alerts.", "questions.", "bills.", "planned.", "debts.", "goals.", "balance.", "assistant.", "settings.");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setText(msg);
    } finally {
      setBusy(false);
    }
  };

  const msgs = (thread.data?.messages ?? []).filter((m) => m.text.trim() || toolsOf(m).length);
  const cost = (thread.data?.messages ?? []).reduce((s, m) => s + m.cost_cents, 0);

  return (
    <>
      <PageHeader
        title="Claude"
        subtitle={thread.data?.thread.title ? thread.data.thread.title : "a new thread"}
        back="/"
        right={
          <button type="button" className="icon-btn" aria-label="Threads" onClick={() => setList(true)}>
            <Icon name="list" size={18} />
          </button>
        }
      />
      {today.data && !apiMode && (
        <Card className="flex flex-col gap-2 p-4">
          <span className="text-[15px] font-bold">Hearth is in Pull mode</span>
          <p className="text-muted text-[13px] font-semibold">Chat here needs API mode with your own Anthropic key. In Pull mode, talk to Claude in the Claude app; it reaches Hearth through the connector.</p>
          <Link to="/more/assistant" className="btn-dark">Assistant settings</Link>
        </Card>
      )}
      <div className="flex flex-col gap-3">
        {thread.loading && !thread.data && <Spinner />}
        {thread.data && msgs.length === 0 && <Card><Empty>Ask about the budget, or tell Claude about a new bill. It reads and writes through the same tools as the app.</Empty></Card>}
        {msgs.map((m) => (
          <div key={m.id} className={`flex flex-col gap-1 ${m.role === "user" ? "items-end" : "items-start"}`}>
            {toolsOf(m).length > 0 && (
              <div className="flex flex-wrap gap-1 px-1">
                {toolsOf(m).map((t, i) => <span key={i} className="bg-line text-muted rounded-full px-2 py-0.5 text-[10px] font-bold">{t}</span>)}
              </div>
            )}
            {m.text.trim() && (
              <div className={`max-w-[88%] rounded-[18px] px-3.5 py-2.5 text-[14px] leading-snug font-medium whitespace-pre-wrap ${m.role === "user" ? "bg-ink text-ground" : "bg-card shadow-[var(--shadow-card)]"}`}>{m.text}</div>
            )}
            <span className="text-faint px-1 text-[10px] font-semibold">{ago(m.created_at)}{m.cost_cents > 0 && ` · ${money(m.cost_cents)}`}</span>
          </div>
        ))}
        {busy && <div className="text-muted px-1 text-[13px] font-semibold">Claude is working…</div>}
        <ErrorNote error={error} onClose={() => setError(null)} />
        <div ref={bottom} />
      </div>
      <div className="bg-ground sticky bottom-24 flex gap-2 pt-2">
        <textarea
          className="field min-h-11 flex-1 resize-none"
          rows={1}
          placeholder={apiMode ? "Ask or tell Claude…" : "Switch to API mode to chat here"}
          value={text}
          disabled={!apiMode || busy}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }}
        />
        <button type="button" className="btn-dark h-11 shrink-0" disabled={!apiMode || busy || !text.trim()} onClick={send}>Send</button>
      </div>
      {cost > 0 && <span className="text-faint text-center text-[11px] font-semibold">this thread has cost {money(cost)}</span>}
      <Sheet open={list} onClose={() => setList(false)} title="Threads">
        <button type="button" className="btn-dark mb-3 w-full" onClick={newThread}><Icon name="plus" size={16} /> New thread</button>
        <Card className="divider overflow-hidden">
          {!threads.data?.length && <Empty>No threads yet.</Empty>}
          {threads.data?.map((t) => (
            <button key={t.id} type="button" className="row w-full text-left" onClick={() => { setParams({ t: t.id }); setList(false); }}>
              <span className="truncate text-[15px] font-bold">{t.title || "Untitled"}</span>
              <span className="text-muted shrink-0 text-[12px] font-semibold">{ago(t.updated_at)}</span>
            </button>
          ))}
        </Card>
      </Sheet>
    </>
  );
}
