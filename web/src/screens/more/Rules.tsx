import { useState } from "react";
import { useCategoryName } from "../../components/CategoryPicker";
import { Card, Empty, Icon, PageHeader, Spinner } from "../../components/ui";
import { useRules } from "../../lib/data";
import { ago } from "../../lib/dates";
import type { Rule } from "../../lib/types";
import { RuleSheet } from "../Activity";

export function Rules() {
  const rules = useRules(true);
  const catName = useCategoryName();
  const [add, setAdd] = useState(false);
  const [edit, setEdit] = useState<Rule | null>(null);
  if (!rules.data) return <Spinner />;
  return (
    <>
      <PageHeader title="Rules" subtitle="checked in priority order on every new transaction" back="/more" />
      <Card className="divider overflow-hidden">
        {rules.data.length === 0 && <Empty>No rules yet. Make one from the review queue.</Empty>}
        {rules.data.map((r) => (
          <button key={r.id} type="button" className="row w-full text-left" onClick={() => setEdit(r)}>
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate text-[15px] font-bold">
                {r.match_type === "regex" ? <code>/{r.pattern}/</code> : `“${r.pattern}”`}
                {!r.is_active && <span className="bg-line ml-2 rounded-full px-2 py-0.5 text-[10px] font-extrabold uppercase">off</span>}
              </span>
              <span className="text-muted truncate text-[12px] font-semibold">
                {r.action === "ignore" ? "ignore" : `→ ${catName(r.category_id)}`}{r.set_note && ` · note “${r.set_note}”`} · {r.hit_count} hit{r.hit_count === 1 ? "" : "s"}{r.last_hit_at && `, last ${ago(r.last_hit_at)}`}
              </span>
            </div>
            <Icon name="chevron" size={18} className="text-faint shrink-0" />
          </button>
        ))}
      </Card>
      <button type="button" className="btn-light" onClick={() => setAdd(true)}><Icon name="plus" size={16} /> Add a rule</button>
      {add && <RuleSheet open onClose={() => setAdd(false)} />}
      {edit && <RuleSheet open onClose={() => setEdit(null)} rule={edit} />}
    </>
  );
}
