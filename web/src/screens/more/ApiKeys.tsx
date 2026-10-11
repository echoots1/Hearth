import { useState } from "react";
import { Card, Empty, ErrorNote, Field, Icon, PageHeader, Sheet, SheetActions, Spinner } from "../../components/ui";
import { api } from "../../lib/api";
import { ago } from "../../lib/dates";
import { useMutation, useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";
import type { ApiKey } from "../../lib/types";

export function ApiKeys() {
  const { me } = useSession();
  const keys = useQuery<ApiKey[]>("keys", () => api.get("/api/keys"));
  const [add, setAdd] = useState(false);
  const [fresh, setFresh] = useState<{ key: ApiKey; plaintext: string } | null>(null);
  const revoke = useMutation((id: string) => api.post(`/api/keys/${id}/revoke`, {}), ["keys"]);
  const owner = me?.role === "owner";
  if (!keys.data) return <Spinner />;
  const live = keys.data.filter((k) => !k.revoked_at);
  const base = window.location.origin;
  return (
    <>
      <PageHeader title="API keys" subtitle="how Claude reaches Hearth" back="/more" />
      <Card className="flex flex-col gap-1 p-4">
        <span className="text-[13px] font-extrabold">MCP endpoint</span>
        <code className="bg-card-head rounded-lg px-2 py-1 text-[12px] font-semibold break-all">{base}/mcp</code>
        <p className="text-muted text-[13px] font-semibold">Send a key as <code>Authorization: Bearer hearth_…</code>, or, for a client that can't set headers, put it in the URL: <code>{base}/mcp/&lt;key&gt;</code>. See docs/MCP.md and SETUP.md.</p>
      </Card>
      <Card className="divider overflow-hidden">
        {live.length === 0 && <Empty>No keys yet.</Empty>}
        {live.map((k) => (
          <div key={k.id} className="row">
            <div className="flex flex-col gap-0.5">
              <span className="text-[15px] font-bold">{k.name} <span className="bg-line rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">{k.scope.replace("_", " ")}</span></span>
              <span className="text-muted text-[12px] font-semibold">hearth_{k.key_prefix}… · {k.last_used_at ? `used ${ago(k.last_used_at)}` : "never used"} · created {ago(k.created_at)}</span>
            </div>
            {owner && <button type="button" className="chip-light text-attention" disabled={revoke.busy} onClick={() => { if (confirm(`Revoke “${k.name}”? Anything using it stops working.`)) revoke.run(k.id); }}>Revoke</button>}
          </div>
        ))}
      </Card>
      <ErrorNote error={revoke.error} />
      {owner && <button type="button" className="btn-light" onClick={() => setAdd(true)}><Icon name="plus" size={16} /> Create a key</button>}
      <CreateKeySheet open={add} onClose={() => setAdd(false)} onCreated={(r) => { setAdd(false); setFresh(r); }} />
      {fresh && (
        <Sheet open onClose={() => setFresh(null)} title="Your new key">
          <p className="text-muted text-[13px] font-semibold">Copy it now. Hearth stores only a hash and can't show it again.</p>
          <code className="bg-card border-line mt-3 block rounded-xl border px-3 py-2 text-[13px] font-bold break-all select-all">{fresh.plaintext}</code>
          <p className="text-muted mt-3 text-[12px] font-semibold">Connector URL with the key built in:</p>
          <code className="bg-card border-line mt-1 block rounded-xl border px-3 py-2 text-[12px] font-semibold break-all select-all">{base}/mcp/{fresh.plaintext}</code>
          <SheetActions>
            <button type="button" className="btn-light" onClick={() => setFresh(null)}>Done</button>
            <button type="button" className="btn-dark" onClick={() => navigator.clipboard?.writeText(fresh.plaintext)}>Copy key</button>
          </SheetActions>
        </Sheet>
      )}
    </>
  );
}

function CreateKeySheet({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (r: { key: ApiKey; plaintext: string }) => void }) {
  const [f, setF] = useState({ name: "claude", scope: "read_write" });
  const m = useMutation(() => api.post<{ key: ApiKey; plaintext: string }>("/api/keys", f), ["keys"]);
  return (
    <Sheet open={open} onClose={onClose} title="Create a key">
      <div className="flex flex-col gap-3">
        <Field label="Name" hint="Where it's used, e.g. “claude app”, “read-only dashboard”."><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus /></Field>
        <Field label="Scope">
          <select className="field" value={f.scope} onChange={(e) => setF({ ...f, scope: e.target.value })}>
            <option value="read_write">Read and write (through the autonomy switch)</option>
            <option value="read">Read only</option>
          </select>
        </Field>
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim()} onClick={async () => { const r = await m.run(); if (r) onCreated(r); }}>Create</button>
      </SheetActions>
    </Sheet>
  );
}
