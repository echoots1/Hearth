import { useEffect, useState } from "react";
import { Card, Empty, ErrorNote, Field, Icon, PageHeader, Sheet, SheetActions, Spinner, Toggle } from "../../components/ui";
import { tool } from "../../lib/api";
import { useAccounts } from "../../lib/data";
import { ago } from "../../lib/dates";
import { money } from "../../lib/money";
import { useMutation } from "../../lib/query";
import type { Account } from "../../lib/types";

export const ACCOUNT_TYPES: { v: Account["type"]; label: string }[] = [
  { v: "checking", label: "Checking" }, { v: "savings", label: "Savings" }, { v: "credit_card", label: "Credit card" }, { v: "virtual_card", label: "Virtual card" },
  { v: "cash", label: "Cash" }, { v: "loan", label: "Loan" }, { v: "investment", label: "Investment" }, { v: "other", label: "Other" },
];

export function accountTypeLabel(type: string): string {
  return ACCOUNT_TYPES.find((t) => t.v === type)?.label ?? type;
}

export function Accounts() {
  const accounts = useAccounts(true);
  const [add, setAdd] = useState(false);
  const [edit, setEdit] = useState<Account | null>(null);
  if (!accounts.data) return <Spinner />;
  // Privacy.com cards are not accounts; the ones earlier versions made are
  // inactive until the sync removes them, and are not worth a row here.
  const rows = accounts.data.filter((a) => !(a.source === "privacy" && !a.is_active));
  return (
    <>
      <PageHeader title="Accounts" back="/more" />
      <Card className="divider overflow-hidden">
        {rows.length === 0 && <Empty>No accounts yet.</Empty>}
        {rows.map((a) => (
          <button key={a.id} type="button" className="row w-full text-left" onClick={() => setEdit(a)}>
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate text-[15px] font-bold">{a.name}{!a.is_active && <span className="bg-line ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">inactive</span>}</span>
              <span className="text-muted truncate text-[12px] font-semibold">
                {ACCOUNT_TYPES.find((t) => t.v === a.type)?.label}{a.mask && ` ····${a.mask}`} · {a.source}{a.institution_label && ` · ${a.institution_label}`}{a.include_in_cash ? " · counts as cash" : ""}
              </span>
            </div>
            <div className="flex shrink-0 flex-col items-end">
              <span className="text-[15px] font-extrabold">{money(a.reported_balance_cents)}</span>
              <span className="text-faint text-[11px] font-semibold">{a.reported_balance_as_of ? ago(a.reported_balance_as_of) : "no balance yet"}</span>
            </div>
          </button>
        ))}
      </Card>
      <button type="button" className="btn-light" onClick={() => setAdd(true)}><Icon name="plus" size={16} /> Add an account</button>
      <AccountSheet open={add || !!edit} onClose={() => { setAdd(false); setEdit(null); }} account={edit} />
    </>
  );
}

function AccountSheet({ open, onClose, account }: { open: boolean; onClose: () => void; account: Account | null }) {
  const blank = { name: "", type: "checking" as Account["type"], mask: "", institution_label: "", include_in_cash: true, is_active: true };
  const [f, setF] = useState(blank);
  useEffect(() => {
    setF(account ? { name: account.name, type: account.type, mask: account.mask ?? "", institution_label: account.institution_label ?? "", include_in_cash: account.include_in_cash, is_active: account.is_active } : blank);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account, open]);
  const m = useMutation(
    () =>
      account
        ? tool("accounts.update", { account_id: account.id, name: f.name, type: f.type, include_in_cash: f.include_in_cash, is_active: f.is_active, institution_label: f.institution_label || null })
        : tool("accounts.create", { name: f.name, type: f.type, mask: f.mask || null, institution_label: f.institution_label || null, include_in_cash: f.include_in_cash }),
    ["accounts.", "balance."],
  );
  return (
    <Sheet open={open} onClose={onClose} title={account ? account.name : "Add an account"}>
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} autoFocus={!account} placeholder="Checking" /></Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Type" hint={account && account.source !== "manual" ? "Synced accounts arrive with the bank's guess; correct it here." : undefined}>
            <select className="field" value={f.type} onChange={(e) => { const type = e.target.value as Account["type"]; setF({ ...f, type, include_in_cash: ["checking", "savings", "cash"].includes(type) }); }}>
              {ACCOUNT_TYPES.map((t) => <option key={t.v} value={t.v}>{t.label}</option>)}
            </select>
          </Field>
          {account
            ? <Field label="Last 4 digits"><input className="field" value={f.mask} disabled /></Field>
            : <Field label="Last 4 digits"><input className="field" inputMode="numeric" maxLength={4} value={f.mask} onChange={(e) => setF({ ...f, mask: e.target.value })} /></Field>}
        </div>
        <Field label="Institution (label only)"><input className="field" value={f.institution_label} onChange={(e) => setF({ ...f, institution_label: e.target.value })} /></Field>
        <Toggle label="Counts as cash" hint="Included in Cash now and the low point." checked={f.include_in_cash} onChange={(v) => setF({ ...f, include_in_cash: v })} />
        {account && <Toggle label="Active" checked={f.is_active} onChange={(v) => setF({ ...f, is_active: v })} />}
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.name.trim()} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>{account ? "Save" : "Add"}</button>
      </SheetActions>
    </Sheet>
  );
}
