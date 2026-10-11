import { Card, Empty, PageHeader, Spinner } from "../../components/ui";
import { tool } from "../../lib/api";
import { useQuery } from "../../lib/query";
import { ago } from "../../lib/dates";
import type { AuditRow } from "../../lib/types";

export function Audit() {
  const rows = useQuery<AuditRow[]>("audit.list:80", () => tool("audit.list", { limit: 80 }));
  if (!rows.data) return <Spinner />;
  return (
    <>
      <PageHeader title="Audit log" subtitle="every write, newest first" back="/more" />
      <Card className="divider overflow-hidden">
        {rows.data.length === 0 && <Empty>Nothing yet.</Empty>}
        {rows.data.map((r) => (
          <div key={r.id} className="flex flex-col gap-0.5 px-4 py-2.5">
            <span className="text-[13px] font-bold">{r.action} <span className="text-muted font-semibold">· {r.entity}</span></span>
            <span className="text-muted text-[11px] font-semibold">{r.actor_kind}{r.actor_id ? ` ${r.actor_id.slice(0, 8)}` : ""} · {ago(r.at)}</span>
          </div>
        ))}
      </Card>
    </>
  );
}
