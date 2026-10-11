import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router";
import { AccountPicker, useAccountName } from "../../components/AccountPicker";
import { ConfigForm } from "../../components/ConfigForm";
import { Card, CardHead, Empty, ErrorNote, Field, Icon, PageHeader, Sheet, SheetActions, Spinner, Toggle } from "../../components/ui";
import { api } from "../../lib/api";
import { useAccounts } from "../../lib/data";
import { ago, longDate, shortDate } from "../../lib/dates";
import { money } from "../../lib/money";
import { invalidate, useMutation, useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";
import { accountTypeLabel } from "./Accounts";

type Mapping = { date: string; description: string; amount?: string | null; debit?: string | null; credit?: string | null; external_id?: string | null; date_format: string; negate: boolean; has_header: boolean };
type Preset = { name: string; mapping: Mapping; account_id?: string | null };
type Batch = { id: string; account_id: string; filename: string; row_count: number; inserted_count: number; duplicate_count: number; rolled_back_at: string | null; created_at: string };
type PlaidItem = { id: string; institution_name: string | null; status: string; last_error: string | null; last_synced_at: string | null; consecutive_failures: number; accounts: { id: string; name: string; type: string; mask: string | null; is_active: boolean; bank_balance_cents: number | null; bank_balance_as_of: string | null }[] };
type JobRun = { status: string; started_at: string; finished_at: string | null; summary: unknown; error: string | null } | null;
type Source = "app" | "env" | null;
type Status = {
  encryption_configured: boolean;
  plaid: { configured: boolean; source: Source; env: string | null; client_id: string | null; products: string[] | null; country_codes: string[] | null; encryption_configured: boolean; items: PlaidItem[]; last_sync: JobRun };
  privacy: {
    configured: boolean; source: Source; env: string | null;
    funding_account_id: string | null;
    funding_account: { id: string; name: string; mask: string | null } | null;
    funding_error: string | null;
    last_sync: JobRun;
  };
  csv: { presets: Preset[]; batches: Batch[] };
};

declare global {
  interface Window {
    Plaid?: { create: (cfg: { token: string; onSuccess: (public_token: string) => void; onExit: () => void }) => { open: () => void } };
  }
}

export function Integrations() {
  const status = useQuery<Status>("integrations.status", () => api.get("/api/integrations/status"));
  const [csvOpen, setCsvOpen] = useState(false);
  if (!status.data) return status.error ? <ErrorNote error={status.error.message} /> : <Spinner />;
  const s = status.data;
  return (
    <>
      <PageHeader title="Integrations" subtitle="feeds in, data stays home" back="/more" />
      <PlaidCard s={s} />
      <PrivacyCard s={s} />
      <Card className="divider overflow-hidden">
        <CardHead title="CSV import" right={`${s.csv.presets.length} preset${s.csv.presets.length === 1 ? "" : "s"}`} />
        <div className="flex flex-col gap-3 p-4">
          <p className="text-muted text-[13px] font-semibold">Export from your bank, map the columns once, and Hearth skips anything it already has. Every import can be rolled back.</p>
          <button type="button" className="btn-dark" onClick={() => setCsvOpen(true)}><Icon name="plus" size={16} /> Import a CSV</button>
        </div>
        {s.csv.batches.map((b) => <BatchRow key={b.id} b={b} />)}
      </Card>
      <CsvWizard open={csvOpen} onClose={() => setCsvOpen(false)} presets={s.csv.presets} />
    </>
  );
}

function PlaidCard({ s }: { s: Status }) {
  const { me } = useSession();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sync = useMutation((id: string) => api.post(`/api/plaid/items/${id}/sync`, {}), ["integrations.", "accounts.", "transactions.", "balance.", "budget."]);
  const remove = useMutation((id: string) => api.del(`/api/plaid/items/${id}`), ["integrations.", "accounts."]);
  const link = async () => {
    setBusy(true);
    setError(null);
    try {
      if (!window.Plaid) {
        await new Promise<void>((res, rej) => {
          const el = document.createElement("script");
          el.src = "https://cdn.plaid.com/link/v2/stable/link-initialize.js";
          el.onload = () => res();
          el.onerror = () => rej(new Error("Could not load Plaid Link."));
          document.head.appendChild(el);
        });
      }
      const { link_token } = await api.post<{ link_token: string }>("/api/plaid/link-token", {});
      const handler = window.Plaid!.create({
        token: link_token,
        onSuccess: async (public_token) => {
          try {
            await api.post("/api/plaid/exchange", { public_token });
            invalidate("integrations.", "accounts.", "transactions.", "balance.", "budget.");
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          } finally {
            setBusy(false);
          }
        },
        onExit: () => setBusy(false),
      });
      handler.open();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  const p = s.plaid;
  const [settingsOpen, setSettingsOpen] = useState(!p.configured);
  const owner = me?.role === "owner";
  return (
    <Card className="divider overflow-hidden">
      <CardHead title="Bank feeds (Plaid)" right={p.configured ? `${p.env} · ${p.items.length} linked` : "not set up"} />
      {(settingsOpen || !p.configured) && (
        <div className="flex flex-col gap-3 p-4">
          <p className="text-muted text-[13px] font-semibold">
            Plaid connects your bank: transactions and balances sync every hour. Get a client ID and secret from the <a className="underline" href="https://dashboard.plaid.com" target="_blank" rel="noreferrer">Plaid dashboard</a>. Sandbox is free and uses fake banks; production needs an approved Plaid account.
          </p>
          <ConfigForm
            name="plaid"
            canEdit={owner}
            configured={p.configured}
            source={p.source}
            hasSecret={p.source === "app"}
            encryptionConfigured={s.encryption_configured}
            testLabel="Check the keys"
            onDone={() => setSettingsOpen(false)}
            initial={{ client_id: p.client_id ?? "", secret: "", env: p.env ?? "sandbox", products: (p.products ?? ["transactions"]).join(", "), country_codes: (p.country_codes ?? ["US"]).join(", ") }}
            fields={[
              { key: "client_id", label: "Client ID", required: true },
              { key: "secret", label: "Secret", secret: true, required: true, hint: "The secret for the environment chosen below." },
              { key: "env", label: "Environment", type: "select", options: [{ value: "sandbox", label: "Sandbox (fake banks, free)" }, { value: "production", label: "Production" }] },
              { key: "products", label: "Products", placeholder: "transactions", hint: "Comma-separated. Leave as is unless Plaid told you otherwise." },
              { key: "country_codes", label: "Countries", placeholder: "US" },
            ]}
          />
          {p.configured && owner && <button type="button" className="chip-light self-start" onClick={() => setSettingsOpen(false)}>Hide settings</button>}
        </div>
      )}
      {p.configured && (
        <>
          {p.items.map((it) => (
            <div key={it.id} className="flex flex-col gap-2 px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="flex flex-col gap-0.5">
                  <span className="text-[15px] font-bold">{it.institution_name ?? "Bank"}{it.status !== "ok" && <span className="bg-attention ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold text-white uppercase">error</span>}</span>
                  <span className={`text-[12px] font-semibold ${it.status === "ok" ? "text-muted" : "text-attention"}`}>
                    {it.status === "ok" ? (it.last_synced_at ? `synced ${ago(it.last_synced_at)}` : "not synced yet") : it.last_error ?? `${it.consecutive_failures} failures`}
                  </span>
                </div>
                <div className="flex gap-2">
                  <button type="button" className="chip-light" disabled={sync.busy} onClick={() => sync.run(it.id)}>Sync now</button>
                  {me?.role === "owner" && <button type="button" className="chip-light text-attention" disabled={remove.busy} onClick={() => { if (confirm("Unlink this bank? Accounts become inactive; transactions stay.")) remove.run(it.id); }}>Unlink</button>}
                </div>
              </div>
              {it.accounts.map((a) => (
                <div key={a.id} className="text-muted flex justify-between text-[12px] font-semibold">
                  <span>{a.name}{a.mask && ` ····${a.mask}`} · {accountTypeLabel(a.type)}{!a.is_active && " · inactive"}</span>
                  <span>{a.bank_balance_cents != null ? `${money(a.bank_balance_cents)} (${ago(a.bank_balance_as_of)})` : ""}</span>
                </div>
              ))}
            </div>
          ))}
          <div className="flex flex-col gap-2 p-4">
            {p.items.length > 0 && <p className="text-faint text-[11px] font-semibold">Account types are the bank's guess. Fix one under <Link className="underline" to="/more/accounts">Accounts</Link>, or ask the assistant.</p>}
            {!p.encryption_configured && <ErrorNote error="APP_ENCRYPTION_KEY is required before linking: access tokens are stored encrypted." />}
            {owner && <button type="button" className="btn-dark" disabled={busy || !p.encryption_configured} onClick={link}>{busy ? "Opening Plaid Link…" : "Link a bank"}</button>}
            <p className="text-faint text-[11px] font-semibold">Plaid Link loads from cdn.plaid.com only when you tap the button.</p>
            {owner && !settingsOpen && <button type="button" className="chip-light self-start" onClick={() => setSettingsOpen(true)}>Plaid settings</button>}
            <ErrorNote error={error || sync.error || remove.error} onClose={() => setError(null)} />
          </div>
        </>
      )}
    </Card>
  );
}

function PrivacyCard({ s }: { s: Status }) {
  const { me } = useSession();
  const owner = me?.role === "owner";
  const sync = useMutation(() => api.post("/api/privacy/sync", {}), ["integrations.", "accounts.", "transactions.", "alerts.", "balance."]);
  const { data: accounts } = useAccounts();
  const p = s.privacy;
  const [settingsOpen, setSettingsOpen] = useState(!p.configured);
  const cards = (p.last_sync?.summary as { cards?: number } | null)?.cards;
  const right = !p.configured ? "not set up" : p.funding_account ? `${p.env} · ${p.funding_account.name}` : p.env ?? "";
  // Bank accounts the cards could draw from; a card is never a funding account.
  const bankAccounts = (accounts ?? []).filter((a) => a.source !== "privacy" && a.type !== "virtual_card" && a.type !== "credit_card");
  return (
    <Card className="divider overflow-hidden">
      <CardHead title="Virtual cards (Privacy.com)" right={right} />
      <div className="flex flex-col gap-3 p-4">
        {p.configured && (
          <>
            <p className="text-muted text-[13px] font-semibold">
              Card charges post to {p.funding_account ? <b>{p.funding_account.name}</b> : "the funding account"} the moment they happen, pending until they settle
              {cards != null && ` (${cards} card${cards === 1 ? "" : "s"})`}. When the bank's own <i>PwP</i> line for a charge arrives it's paired with the card charge so nothing is counted twice. Declines become alerts.
            </p>
            {p.funding_error && <ErrorNote error={p.funding_error} />}
            <div className="flex items-center justify-between">
              <span className="text-muted text-[12px] font-semibold">{p.last_sync ? `last sync ${ago(p.last_sync.started_at)} · ${p.last_sync.status}` : "not synced yet"}</span>
              <div className="flex gap-2">
                {owner && !settingsOpen && <button type="button" className="chip-light" onClick={() => setSettingsOpen(true)}>Settings</button>}
                <button type="button" className="chip-light" disabled={sync.busy} onClick={() => sync.run()}>Sync now</button>
              </div>
            </div>
            {p.last_sync?.error && <ErrorNote error={p.last_sync.error} />}
            <ErrorNote error={sync.error} />
          </>
        )}
        {(settingsOpen || !p.configured) && (
          <>
            {!p.configured && <p className="text-muted text-[13px] font-semibold">Privacy.com charges show up on your bank account the moment a card is used, and pair with the bank's line when it arrives. Create an API key under Account → API on privacy.com (a paid plan is required for API access).</p>}
            <ConfigForm
              name="privacy"
              canEdit={owner}
              configured={p.configured}
              source={p.source}
              hasSecret={p.source === "app"}
              encryptionConfigured={s.encryption_configured}
              testLabel="Check the key"
              onDone={() => setSettingsOpen(false)}
              initial={{ api_key: "", env: p.env ?? "production", funding_account_id: p.funding_account_id ?? "" }}
              fields={[
                { key: "api_key", label: "API key", secret: true, required: true },
                { key: "env", label: "Environment", type: "select", options: [{ value: "production", label: "Production" }, { value: "sandbox", label: "Sandbox" }] },
                {
                  key: "funding_account_id", label: "Funding account", type: "select",
                  hint: "The bank account your Privacy cards draw from. Charges post here and pair with its PwP lines.",
                  options: [
                    { value: "", label: bankAccounts.filter((a) => a.type === "checking").length === 1 ? "Your checking account (automatic)" : "Choose an account…" },
                    ...bankAccounts.map((a) => ({ value: a.id, label: `${a.name}${a.mask ? ` ····${a.mask}` : ""}` })),
                  ],
                },
              ]}
            />
            {p.configured && owner && <button type="button" className="chip-light self-start" onClick={() => setSettingsOpen(false)}>Hide settings</button>}
          </>
        )}
      </div>
    </Card>
  );
}

function BatchRow({ b }: { b: Batch }) {
  const accountName = useAccountName();
  const rollback = useMutation(() => api.post(`/api/import/batches/${b.id}/rollback`, {}), ["integrations.", "transactions.", "budget.", "balance."]);
  return (
    <div className="row">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate text-[15px] font-bold">{b.filename}{b.rolled_back_at && <span className="bg-line ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">rolled back</span>}</span>
        <span className="text-muted truncate text-[12px] font-semibold">{longDate(b.created_at)} · {accountName(b.account_id)} · {b.inserted_count} added, {b.duplicate_count} skipped of {b.row_count}</span>
      </div>
      {!b.rolled_back_at && b.inserted_count > 0 && <button type="button" className="chip-light shrink-0" disabled={rollback.busy} onClick={() => { if (confirm(`Remove the ${b.inserted_count} transactions this import added?`)) rollback.run(); }}>Roll back</button>}
    </div>
  );
}

type Headers = { headers: string[]; rows: number; sample: string[][] };
type PreviewRow = { line: number; date: string | null; description: string; amount_cents: number | null; external_id: string | null; duplicate: boolean; error: string | null };
type Preview = { headers: string[]; rows: PreviewRow[]; total: number; ok: number; duplicates: number; errors: number; truncated: boolean };

function guess(headers: string[], ...needles: string[]): string {
  const h = headers.map((x) => x.toLowerCase());
  for (const n of needles) {
    const i = h.findIndex((x) => x === n);
    if (i >= 0) return headers[i];
  }
  for (const n of needles) {
    const i = h.findIndex((x) => x.includes(n));
    if (i >= 0) return headers[i];
  }
  return "";
}

function CsvWizard({ open, onClose, presets }: { open: boolean; onClose: () => void; presets: Preset[] }) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [account, setAccount] = useState<string | null>(null);
  const [filename, setFilename] = useState("upload.csv");
  const [text, setText] = useState("");
  const [hasHeader, setHasHeader] = useState(true);
  const [headers, setHeaders] = useState<Headers | null>(null);
  const [m, setM] = useState<Mapping>({ date: "", description: "", amount: "", debit: "", credit: "", external_id: "", date_format: "auto", negate: false, has_header: true });
  const [mode, setMode] = useState<"amount" | "split">("amount");
  const [preset, setPreset] = useState("");
  const [saveAs, setSaveAs] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Batch | null>(null);

  useEffect(() => {
    if (!open) { setStep(1); setText(""); setHeaders(null); setPreview(null); setDone(null); setError(null); }
  }, [open]);

  const mapping = useMemo<Mapping>(() => ({
    ...m,
    has_header: hasHeader,
    amount: mode === "amount" ? m.amount || null : null,
    debit: mode === "split" ? m.debit || null : null,
    credit: mode === "split" ? m.credit || null : null,
    external_id: m.external_id || null,
  }), [m, mode, hasHeader]);

  const readFile = async (f: File) => {
    setFilename(f.name);
    setText(await f.text());
  };

  const toMapping = async () => {
    setBusy(true);
    setError(null);
    try {
      const h = await api.post<Headers>("/api/import/csv/headers", { text, has_header: hasHeader });
      setHeaders(h);
      const p = presets.find((x) => x.name === preset);
      if (p) {
        setM({ ...p.mapping, amount: p.mapping.amount ?? "", debit: p.mapping.debit ?? "", credit: p.mapping.credit ?? "", external_id: p.mapping.external_id ?? "" });
        setMode(p.mapping.amount ? "amount" : "split");
        if (p.account_id && !account) setAccount(p.account_id);
      } else {
        const date = guess(h.headers, "date", "posted", "transaction date");
        const description = guess(h.headers, "description", "payee", "name", "memo", "details");
        const amount = guess(h.headers, "amount");
        const debit = guess(h.headers, "debit", "withdrawal", "money out");
        const credit = guess(h.headers, "credit", "deposit", "money in");
        setM({ date, description, amount, debit, credit, external_id: guess(h.headers, "id", "reference"), date_format: "auto", negate: false, has_header: hasHeader });
        setMode(amount || !(debit && credit) ? "amount" : "split");
      }
      setStep(2);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const toPreview = async () => {
    setBusy(true);
    setError(null);
    try {
      setPreview(await api.post<Preview>("/api/import/csv/preview", { account_id: account, text, mapping }));
      setStep(3);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const commit = async () => {
    setBusy(true);
    setError(null);
    try {
      const b = await api.post<Batch>("/api/import/csv", { account_id: account, filename, text, mapping, save_preset: saveAs || null });
      setDone(b);
      invalidate("integrations.", "transactions.", "budget.", "balance.", "alerts.");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const pick = (label: string, key: keyof Mapping, optional?: boolean) => (
    <Field label={label}>
      <select className="field" value={(m[key] as string) ?? ""} onChange={(e) => setM({ ...m, [key]: e.target.value })}>
        {optional && <option value="">—</option>}
        {!optional && !m[key] && <option value="">Choose…</option>}
        {headers?.headers.map((h) => <option key={h} value={h}>{h}</option>)}
      </select>
    </Field>
  );

  return (
    <Sheet open={open} onClose={onClose} title={done ? "Imported" : `Import a CSV · step ${step} of 3`}>
      {done ? (
        <>
          <p className="text-[15px] font-bold">{done.inserted_count} transactions added, {done.duplicate_count} skipped as duplicates.</p>
          <p className="text-muted mt-1 text-[13px] font-semibold">New rows went through the rules; anything unmatched is in the review queue. You can roll this import back from the Integrations page.</p>
          <SheetActions><button type="button" className="btn-dark" onClick={onClose}>Done</button></SheetActions>
        </>
      ) : step === 1 ? (
        <div className="flex flex-col gap-3">
          <Field label="Into account"><AccountPicker value={account} onChange={setAccount} /></Field>
          <Field label="File" hint="Any bank export. Nothing leaves this server.">
            <input className="field" type="file" accept=".csv,text/csv,text/plain" onChange={(e) => { const f = e.target.files?.[0]; if (f) void readFile(f); }} />
          </Field>
          <Field label="Or paste the CSV"><textarea className="field min-h-24" value={text} onChange={(e) => setText(e.target.value)} placeholder="Date,Description,Amount…" /></Field>
          <Toggle label="First row is a header" checked={hasHeader} onChange={setHasHeader} />
          {presets.length > 0 && (
            <Field label="Column preset">
              <select className="field" value={preset} onChange={(e) => setPreset(e.target.value)}>
                <option value="">Detect columns automatically</option>
                {presets.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
              </select>
            </Field>
          )}
          <ErrorNote error={error} />
          <SheetActions>
            <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
            <button type="button" className="btn-dark" disabled={busy || !account || !text.trim()} onClick={toMapping}>Next</button>
          </SheetActions>
        </div>
      ) : step === 2 ? (
        <div className="flex flex-col gap-3">
          <p className="text-muted text-[13px] font-semibold">{headers?.rows} rows. Tell Hearth which column is which.</p>
          {pick("Date", "date")}
          {pick("Description", "description")}
          <div className="flex gap-2">
            <button type="button" className={mode === "amount" ? "chip-dark" : "chip-light"} onClick={() => setMode("amount")}>One amount column</button>
            <button type="button" className={mode === "split" ? "chip-dark" : "chip-light"} onClick={() => setMode("split")}>Debit and credit columns</button>
          </div>
          {mode === "amount" ? (
            <>
              {pick("Amount", "amount")}
              <Toggle label="Charges are positive in this file" hint="Flip the sign so money out is negative." checked={m.negate} onChange={(v) => setM({ ...m, negate: v })} />
            </>
          ) : (
            <div className="grid grid-cols-2 gap-3">{pick("Debit (money out)", "debit")}{pick("Credit (money in)", "credit")}</div>
          )}
          {pick("Reference id (optional)", "external_id", true)}
          <Field label="Date format">
            <select className="field" value={m.date_format} onChange={(e) => setM({ ...m, date_format: e.target.value })}>
              <option value="auto">Detect</option><option value="%m/%d/%Y">MM/DD/YYYY</option><option value="%d/%m/%Y">DD/MM/YYYY</option><option value="%Y-%m-%d">YYYY-MM-DD</option><option value="%m/%d/%y">MM/DD/YY</option>
            </select>
          </Field>
          {headers?.sample[0] && <p className="text-faint text-[11px] font-semibold">First row: {headers.sample[0].join(" · ")}</p>}
          <ErrorNote error={error} />
          <SheetActions>
            <button type="button" className="btn-light" onClick={() => setStep(1)}>Back</button>
            <button type="button" className="btn-dark" disabled={busy || !m.date || !m.description || (mode === "amount" ? !m.amount : !m.debit || !m.credit)} onClick={toPreview}>Preview</button>
          </SheetActions>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          {preview && (
            <>
              <div className="grid grid-cols-3 gap-2">
                <div className="bg-card rounded-xl p-2.5"><div className="text-muted text-[11px] font-semibold">Will add</div><div className="text-[15px] font-bold">{preview.ok}</div></div>
                <div className="bg-card rounded-xl p-2.5"><div className="text-muted text-[11px] font-semibold">Already have</div><div className="text-[15px] font-bold">{preview.duplicates}</div></div>
                <div className="bg-card rounded-xl p-2.5"><div className="text-muted text-[11px] font-semibold">Can't read</div><div className={`text-[15px] font-bold ${preview.errors ? "text-attention" : ""}`}>{preview.errors}</div></div>
              </div>
              <div className="card divider max-h-72 overflow-y-auto">
                {preview.rows.slice(0, 60).map((r) => (
                  <div key={r.line} className={`flex items-center justify-between gap-2 px-3 py-2 text-[12px] ${r.error ? "text-attention" : r.duplicate ? "text-faint" : ""}`}>
                    <span className="truncate font-semibold">{r.date ? shortDate(r.date) : "?"} · {r.description || "?"}{r.duplicate && " · have it"}{r.error && ` · ${r.error}`}</span>
                    <span className="shrink-0 font-bold">{r.amount_cents != null ? money(r.amount_cents) : "—"}</span>
                  </div>
                ))}
                {preview.rows.length === 0 && <Empty>No rows.</Empty>}
              </div>
              <Field label="Save these columns as a preset (optional)"><input className="field" value={saveAs} onChange={(e) => setSaveAs(e.target.value)} placeholder="Demo Bank checking" /></Field>
            </>
          )}
          <ErrorNote error={error} />
          <SheetActions>
            <button type="button" className="btn-light" onClick={() => setStep(2)}>Back</button>
            <button type="button" className="btn-dark" disabled={busy || !preview || preview.ok === 0} onClick={commit}>Import {preview?.ok ?? 0}</button>
          </SheetActions>
        </div>
      )}
    </Sheet>
  );
}
