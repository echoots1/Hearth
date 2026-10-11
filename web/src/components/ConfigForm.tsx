import { useEffect, useState } from "react";
import { ErrorNote, Field } from "./ui";
import { api } from "../lib/api";
import { invalidate } from "../lib/query";

export type ConfigField = {
  key: string;
  label: string;
  type?: "text" | "password" | "number" | "select" | "email" | "url";
  options?: { value: string; label: string }[];
  placeholder?: string;
  hint?: string;
  /** A stored secret: left blank on save, the stored value is kept. */
  secret?: boolean;
  /** Save is blocked while this is blank and nothing is stored. */
  required?: boolean;
};

/**
 * Owner-only settings form for one integration. Values are posted to
 * `PUT /api/integrations/{name}`; secrets are never read back, so the form
 * only knows whether one is stored.
 */
export function ConfigForm({
  name,
  fields,
  initial,
  configured,
  source,
  hasSecret,
  encryptionConfigured,
  canEdit,
  testLabel = "Send a test",
  onDone,
}: {
  name: string;
  fields: ConfigField[];
  initial: Record<string, string>;
  configured: boolean;
  source: "app" | "env" | null;
  hasSecret: boolean;
  encryptionConfigured: boolean;
  canEdit: boolean;
  testLabel?: string;
  onDone?: () => void;
}) {
  const [values, setValues] = useState<Record<string, string>>(initial);
  const [busy, setBusy] = useState<"save" | "remove" | "test" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => { setValues(initial); }, [JSON.stringify(initial)]); // eslint-disable-line react-hooks/exhaustive-deps
  const refresh = () => invalidate("integrations.", "jobs.");
  const run = async (kind: "save" | "remove" | "test", fn: () => Promise<unknown>, done?: string) => {
    setBusy(kind);
    setError(null);
    setNote(null);
    try {
      const r = (await fn()) as { detail?: string } | undefined;
      setNote(r?.detail ?? done ?? null);
      refresh();
      if (kind === "save") {
        setValues((v) => Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fields.find((f) => f.key === k)?.secret ? "" : x])));
        onDone?.();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };
  const missing = fields.filter((f) => f.required && !values[f.key] && !(f.secret && hasSecret));
  return (
    <div className="flex flex-col gap-3">
      {source === "env" && (
        <p className="text-muted text-[12px] font-semibold">Currently set from the server's environment. Saving here stores a copy in the app, encrypted, and the app's copy wins.</p>
      )}
      {!encryptionConfigured && <ErrorNote error="APP_ENCRYPTION_KEY is not set on the server, so nothing can be stored here yet." />}
      {fields.map((f) => (
        <Field key={f.key} label={f.secret && hasSecret ? `${f.label} (one is stored)` : f.label} hint={f.hint}>
          {f.type === "select" ? (
            <select className="field" value={values[f.key] ?? ""} disabled={!canEdit} onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}>
              {(f.options ?? []).map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          ) : (
            <input
              className="field"
              type={f.type ?? (f.secret ? "password" : "text")}
              value={values[f.key] ?? ""}
              disabled={!canEdit}
              placeholder={f.secret && hasSecret ? "leave blank to keep the stored one" : f.placeholder}
              autoComplete={f.secret ? "new-password" : "off"}
              onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}
            />
          )}
        </Field>
      ))}
      {canEdit && (
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn-dark" disabled={busy !== null || !encryptionConfigured || missing.length > 0} onClick={() => run("save", () => api.put(`/api/integrations/${name}`, values), "Saved.")}>
            {busy === "save" ? "Saving…" : "Save"}
          </button>
          {configured && (
            <button type="button" className="btn-light" disabled={busy !== null} onClick={() => run("test", () => api.post(`/api/integrations/${name}/test`, {}))}>
              {busy === "test" ? "Testing…" : testLabel}
            </button>
          )}
          {source === "app" && (
            <button type="button" className="btn-light text-attention" disabled={busy !== null} onClick={() => { if (confirm("Remove these settings from the app?")) void run("remove", () => api.del(`/api/integrations/${name}`), "Removed."); }}>
              Remove
            </button>
          )}
        </div>
      )}
      {note && <p className="text-income text-[12px] font-bold">{note}</p>}
      <ErrorNote error={error} onClose={() => setError(null)} />
    </div>
  );
}
