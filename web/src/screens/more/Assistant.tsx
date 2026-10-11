import { useEffect, useState } from "react";
import { Link } from "react-router";
import { Card, CardHead, ErrorNote, Field, MoneyInput, PageHeader, Segmented, Spinner } from "../../components/ui";
import { api } from "../../lib/api";
import { useMutation, useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";
import { dollars } from "../../lib/money";
import type { AssistantSettings, AssistantStatus, Autonomy } from "../../lib/types";

const AREAS: { key: "categorizing_notes" | "plans_bills" | "debts_goals"; label: string; backseat: string; driver: string }[] = [
  { key: "categorizing_notes", label: "Categorizing & notes", backseat: "Claude proposes categories, notes and rules; you approve.", driver: "Claude categorizes, sets notes and makes rules; Undo in Activity." },
  { key: "plans_bills", label: "Plans & bills", backseat: "Claude proposes plan changes and new bills; you approve.", driver: "Claude sets plans, adds and changes bills; Undo in Activity." },
  { key: "debts_goals", label: "Debts & goals", backseat: "Claude proposes strategy or goal changes; you approve.", driver: "Claude changes strategy and goals; Undo in Activity." },
];

export function Assistant() {
  const { me } = useSession();
  const status = useQuery<AssistantStatus>("settings.assistant", () => api.get("/api/settings/assistant"));
  const [s, setS] = useState<AssistantSettings | null>(null);
  useEffect(() => { if (status.data) setS(status.data.assistant); }, [status.data]);
  const save = useMutation((next: AssistantSettings) => api.post("/api/settings/assistant", next), ["settings.", "household."]);
  const owner = me?.role === "owner";
  if (!status.data || !s) return <Spinner />;
  const set = (patch: Partial<AssistantSettings>) => { const next = { ...s, ...patch }; setS(next); void save.run(next); };
  const pos = (v: Autonomy | null) => v ?? "global";
  return (
    <>
      <PageHeader title="Assistant" subtitle="how Claude works with Hearth" back="/more" />
      {!owner && <ErrorNote error="Only the owner can change these." />}
      <Card className="divider overflow-hidden">
        <CardHead title="Mode" />
        <div className="flex flex-col gap-3 p-4">
          <Segmented grow value={s.mode} onChange={(mode) => set({ mode })} items={[{ key: "pull", label: "Pull (Claude app)" }, { key: "api", label: "API (your key)" }]} />
          <p className="text-muted text-[13px] font-semibold">
            {s.mode === "pull"
              ? "Free with a Claude subscription. Claude reaches Hearth through the connector when you chat, and a recurring task in the Claude app runs the daily pass. “Do it now” requests go to the Claude app. See SETUP.md."
              : "Hearth calls the Claude API with your own key: a chat panel in the app and a scheduled pass, billed to your Anthropic account under the cap below. Your budget data goes to Anthropic's API."}
          </p>
        </div>
      </Card>

      <Card className="divider overflow-hidden">
        <CardHead title="Backseat / Driver" right="writes only" />
        <div className="flex flex-col gap-2 p-4">
          <div className="flex items-center justify-between gap-3">
            <div className="flex flex-col"><span className="text-[15px] font-bold">Everywhere</span><span className="text-muted text-[12px] font-semibold">the global position</span></div>
            <Segmented small value={s.autonomy} onChange={(autonomy) => set({ autonomy })} items={[{ key: "backseat", label: "Backseat" }, { key: "driver", label: "Driver" }]} />
          </div>
          <p className="text-muted text-[12px] font-semibold">Backseat: a write becomes a proposal you approve, edit or decline. Driver: it applies at once and shows as “Claude did…” with Undo. Alerts and questions always apply.</p>
        </div>
        {AREAS.map((a) => {
          const effective = s[a.key] ?? s.autonomy;
          return (
            <div key={a.key} className="flex flex-col gap-2 p-4">
              <div className="flex flex-col gap-2">
                <span className="text-[15px] font-bold">{a.label}</span>
                <Segmented small grow value={pos(s[a.key])} onChange={(v) => set({ [a.key]: v === "global" ? null : (v as Autonomy) })} items={[{ key: "global", label: "Global" }, { key: "backseat", label: "Backseat" }, { key: "driver", label: "Driver" }]} />
              </div>
              <span className="text-muted text-[12px] font-semibold">{effective === "driver" ? a.driver : a.backseat}</span>
            </div>
          );
        })}
      </Card>

      {s.mode === "api" && <ApiModeCard s={s} set={set} status={status.data} />}
      <ErrorNote error={save.error} />
    </>
  );
}

function ApiModeCard({ s, set, status }: { s: AssistantSettings; set: (p: Partial<AssistantSettings>) => void; status: AssistantStatus }) {
  const [key, setKey] = useState("");
  const saveKey = useMutation(() => api.post("/api/settings/anthropic-key", { api_key: key }), ["settings."]);
  const clearKey = useMutation(() => api.del("/api/settings/anthropic-key"), ["settings."]);
  const [cap, setCap] = useState<number | null>(s.monthly_cap_cents);
  const pass = useMutation(() => api.post("/api/assistant/pass", {}), ["settings.", "jobs.", "alerts.", "assistant."]);
  const models = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];
  return (
    <Card className="divider overflow-hidden">
      <CardHead title="API mode" right={`${dollars(status.month_spent_cents)} this month · ${status.month_calls} calls`} />
      <div className="flex flex-col gap-3 p-4">
        {!status.encryption_configured && <ErrorNote error="Set APP_ENCRYPTION_KEY in the server environment first; the key is stored encrypted." />}
        <Field label={status.has_api_key ? "Anthropic API key (one is stored)" : "Anthropic API key"} hint="Stored encrypted; never shown again.">
          <div className="flex gap-2">
            <input className="field" type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder="sk-ant-…" />
            <button type="button" className="btn-dark shrink-0" disabled={!key || saveKey.busy || !status.encryption_configured} onClick={async () => { if ((await saveKey.run()) !== undefined) setKey(""); }}>Save</button>
            {status.has_api_key && <button type="button" className="btn-light shrink-0" disabled={clearKey.busy} onClick={() => clearKey.run()}>Remove</button>}
          </div>
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Chat model">
            <select className="field" value={s.chat_model} onChange={(e) => set({ chat_model: e.target.value })}>{models.map((m) => <option key={m}>{m}</option>)}</select>
          </Field>
          <Field label="Pass model">
            <select className="field" value={s.pass_model} onChange={(e) => set({ pass_model: e.target.value })}>{models.map((m) => <option key={m}>{m}</option>)}</select>
          </Field>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Monthly cap" hint="Hearth stops calling when spend reaches it.">
            <MoneyInput value={cap} onChange={setCap} />
          </Field>
          <Field label="Daily pass at"><input className="field" type="time" value={s.pass_time} onChange={(e) => set({ pass_time: e.target.value })} /></Field>
        </div>
        {cap !== s.monthly_cap_cents && cap != null && <button type="button" className="btn-dark" onClick={() => set({ monthly_cap_cents: cap })}>Save cap</button>}
        <div className="flex gap-2">
          <Link to="/chat" className="btn-light flex-1">Open chat</Link>
          <button type="button" className="btn-light flex-1" disabled={pass.busy || !status.has_api_key} onClick={() => pass.run()}>{pass.busy ? "Working…" : "Run a pass now"}</button>
        </div>
        <ErrorNote error={pass.error} />
        <p className="text-faint text-[11px] font-semibold">The pass runs daily at the time above when a key is saved. Your budget data is sent to Anthropic's API on every call.</p>
        <ErrorNote error={saveKey.error || clearKey.error} />
      </div>
    </Card>
  );
}
