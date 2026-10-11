import { useEffect, useState } from "react";
import { Card, CardHead, ErrorNote, Field, PageHeader, Spinner, Toggle } from "../../components/ui";
import { api } from "../../lib/api";
import { useMutation, useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";

type Settings = { app_url: string | null; trust_proxy: boolean | null; mcp_extra_hosts: string[]; evaluate_minutes: number | null; plaid_sync_minutes: number | null; privacy_sync_minutes: number | null };
type View = {
  settings: Settings;
  effective: { app_url: string; trust_proxy: boolean; mcp_allowed_hosts: string[]; evaluate_minutes: number; plaid_sync_minutes: number; privacy_sync_minutes: number };
  env: { app_url: string; trust_proxy: boolean; mcp_allowed_hosts: string[] | null; evaluate_minutes: number; plaid_sync_minutes: number; privacy_sync_minutes: number; jobs_enabled: boolean; port: number };
  secrets: { app_secret: "env" | "generated"; app_encryption_key: "env" | "generated"; data_dir: string };
};

export function Server() {
  const { me } = useSession();
  const owner = me?.role === "owner";
  // The view names environment values and the data directory, so only the owner gets it.
  const q = useQuery<View>(owner ? "settings.server" : null, () => api.get("/api/settings/server"));
  const [s, setS] = useState<Settings | null>(null);
  const [hosts, setHosts] = useState("");
  useEffect(() => {
    if (q.data) {
      setS(q.data.settings);
      setHosts(q.data.settings.mcp_extra_hosts.join(", "));
    }
  }, [q.data]);
  const save = useMutation((next: Settings) => api.put("/api/settings/server", next), ["settings.", "jobs."]);
  if (!owner) {
    return (
      <>
        <PageHeader title="Server" subtitle="how Hearth is reached" back="/more" />
        <ErrorNote error="Only the owner can see and change the server settings." />
      </>
    );
  }
  if (!q.data || !s) return q.error ? <ErrorNote error={q.error.message} /> : <Spinner />;
  const v = q.data;
  const num = (x: string): number | null => (x.trim() === "" ? null : Math.max(0, Math.floor(Number(x))));
  const dirty = JSON.stringify({ ...s, mcp_extra_hosts: hosts.split(",").map((h) => h.trim()).filter(Boolean) }) !== JSON.stringify(v.settings);
  const submit = () => save.run({ ...s, mcp_extra_hosts: hosts.split(",").map((h) => h.trim()).filter(Boolean) });
  const https = (s.app_url ?? v.env.app_url).startsWith("https://");
  return (
    <>
      <PageHeader title="Server" subtitle="how Hearth is reached" back="/more" />
      <Card className="divider overflow-hidden">
        <CardHead title="Address" right={v.effective.trust_proxy ? "behind a proxy" : "direct"} />
        <div className="flex flex-col gap-3 p-4">
          <Field label="Public URL" hint={`What people type to reach Hearth. Links in notifications and the connector URL are built from it. Leave blank to use the server's APP_URL (${v.env.app_url}).`}>
            <input className="field" type="url" value={s.app_url ?? ""} disabled={!owner} placeholder={v.env.app_url} onChange={(e) => setS({ ...s, app_url: e.target.value || null })} />
          </Field>
          {!https && <p className="text-attention text-[12px] font-semibold">Plain http: the installable app, push notifications and the Claude connector need an https address with a trusted certificate. See SETUP.md, "Getting HTTPS".</p>}
          <Toggle label="Behind a reverse proxy" hint="Trust X-Forwarded-For from the proxy so rate limits see each client, not the proxy." checked={s.trust_proxy ?? v.env.trust_proxy} onChange={(trust_proxy) => owner && setS({ ...s, trust_proxy })} />
          <Field label="Extra MCP host names" hint={`Comma-separated. The connector already accepts: ${v.effective.mcp_allowed_hosts.join(", ")}.`}>
            <input className="field" value={hosts} disabled={!owner} placeholder="money.lan, 192.168.1.20:3456" onChange={(e) => setHosts(e.target.value)} />
          </Field>
        </div>
      </Card>
      <Card className="divider overflow-hidden">
        <CardHead title="Schedules" right={v.env.jobs_enabled ? "scheduler on" : "scheduler off (JOBS_ENABLED=false)"} />
        <div className="grid grid-cols-3 gap-3 p-4">
          <Field label="Evaluate" hint="minutes"><input className="field" type="number" min={1} value={s.evaluate_minutes ?? ""} disabled={!owner} placeholder={String(v.env.evaluate_minutes)} onChange={(e) => setS({ ...s, evaluate_minutes: num(e.target.value) })} /></Field>
          <Field label="Plaid sync" hint="minutes"><input className="field" type="number" min={5} value={s.plaid_sync_minutes ?? ""} disabled={!owner} placeholder={String(v.env.plaid_sync_minutes)} onChange={(e) => setS({ ...s, plaid_sync_minutes: num(e.target.value) })} /></Field>
          <Field label="Privacy sync" hint="minutes"><input className="field" type="number" min={2} value={s.privacy_sync_minutes ?? ""} disabled={!owner} placeholder={String(v.env.privacy_sync_minutes)} onChange={(e) => setS({ ...s, privacy_sync_minutes: num(e.target.value) })} /></Field>
        </div>
      </Card>
      {owner && (
        <div className="flex gap-2">
          <button type="button" className="btn-dark flex-1" disabled={!dirty || save.busy} onClick={submit}>{save.busy ? "Saving…" : "Save"}</button>
        </div>
      )}
      <ErrorNote error={save.error} />
      <Card className="flex flex-col gap-1 p-4">
        <span className="text-[13px] font-extrabold">Keys</span>
        <p className="text-muted text-[13px] font-semibold">
          Session key: {v.secrets.app_secret === "env" ? "from the environment (APP_SECRET)." : `generated on first start and kept in ${v.secrets.data_dir}.`} Encryption key: {v.secrets.app_encryption_key === "env" ? "from the environment (APP_ENCRYPTION_KEY)." : `generated on first start and kept in ${v.secrets.data_dir}.`}
        </p>
        {(v.secrets.app_secret === "generated" || v.secrets.app_encryption_key === "generated") && (
          <p className="text-attention text-[12px] font-semibold">Back up that directory with the database: without the encryption key, saved integration keys, Plaid tokens and push keys cannot be read.</p>
        )}
      </Card>
    </>
  );
}
