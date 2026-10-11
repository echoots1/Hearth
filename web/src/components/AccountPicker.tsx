import { useAccounts } from "../lib/data";

export function AccountPicker({ value, onChange, none }: { value: string | null; onChange: (id: string | null) => void; none?: string }) {
  const { data } = useAccounts();
  return (
    <select className="field" value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}>
      {none !== undefined ? <option value="">{none}</option> : !value && <option value="">Choose an account…</option>}
      {(data ?? []).filter((a) => a.is_active).map((a) => (
        <option key={a.id} value={a.id}>{a.name}{a.mask ? ` ····${a.mask}` : ""}</option>
      ))}
    </select>
  );
}

export function useAccountName() {
  const { data } = useAccounts(true);
  const map = new Map((data ?? []).map((a) => [a.id, a]));
  return (id: string | null | undefined) => {
    const a = id ? map.get(id) : undefined;
    if (!a) return "";
    return a.type === "credit_card" || a.type === "virtual_card" ? `card ending ${a.mask ?? "…"}` : a.name;
  };
}
