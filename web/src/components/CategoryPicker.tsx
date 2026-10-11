/** A grouped <select> of categories; `none` adds an "Uncategorized" choice. */
import { useGroups } from "../lib/data";

export function CategoryPicker({ value, onChange, none, income, autoFocus }: { value: string | null; onChange: (id: string | null) => void; none?: string; income?: boolean; autoFocus?: boolean }) {
  const { data } = useGroups();
  const groups = data?.groups ?? [];
  const cats = (data?.categories ?? []).filter((c) => !c.is_archived && (income === undefined || (c.sign === "income") === income));
  return (
    <select className="field" value={value ?? ""} onChange={(e) => onChange(e.target.value || null)} autoFocus={autoFocus}>
      {none !== undefined && <option value="">{none}</option>}
      {!none && !value && <option value="">Choose a category…</option>}
      {groups.map((g) => {
        const inGroup = cats.filter((c) => c.group_id === g.id);
        if (inGroup.length === 0) return null;
        return (
          <optgroup key={g.id} label={g.name}>
            {inGroup.map((c) => (
              <option key={c.id} value={c.id}>{c.name}</option>
            ))}
          </optgroup>
        );
      })}
    </select>
  );
}

export function useCategoryName() {
  const { data } = useGroups();
  const map = new Map((data?.categories ?? []).map((c) => [c.id, c.name]));
  return (id: string | null | undefined) => (id ? (map.get(id) ?? "…") : "Uncategorized");
}
