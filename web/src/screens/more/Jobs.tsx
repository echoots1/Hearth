import { Card, ErrorNote, PageHeader, Spinner } from "../../components/ui";
import { tool } from "../../lib/api";
import { ago } from "../../lib/dates";
import { useMutation, useQuery } from "../../lib/query";

type JobRun = { id: string; status: string; started_at: string; finished_at: string | null; summary: Record<string, unknown> | null; error: string | null; triggered_by: string };
type JobStatus = { job: string; description: string; every_seconds: number; last_run: JobRun | null; due: boolean };

function every(s: number) {
  if (s % 86400 === 0) return `daily`;
  if (s % 3600 === 0) return `every ${s / 3600}h`;
  return `every ${Math.round(s / 60)} min`;
}

export function Jobs() {
  const jobs = useQuery<JobStatus[]>("jobs.status", () => tool("jobs.status"));
  const run = useMutation((name: string) => tool("jobs.run", { name }), ["jobs.", "alerts.", "transactions.", "budget.", "balance."]);
  if (!jobs.data) return <Spinner />;
  return (
    <>
      <PageHeader title="Jobs" subtitle="they detect and compute; they never decide" back="/more" />
      {jobs.data.map((j) => (
        <Card key={j.job} className="flex flex-col gap-2 p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="flex flex-col gap-0.5">
              <span className="text-[15px] font-bold"><code>{j.job}</code> <span className="text-muted text-[12px] font-semibold">{every(j.every_seconds)}</span></span>
              <span className="text-muted text-[12px] font-semibold">{j.description}</span>
            </div>
            <button type="button" className="chip-light shrink-0" disabled={run.busy} onClick={() => run.run(j.job)}>Run now</button>
          </div>
          {j.last_run ? (
            <div className={`text-[12px] font-semibold ${j.last_run.status === "failed" ? "text-attention" : "text-muted"}`}>
              last {j.last_run.status} {ago(j.last_run.finished_at ?? j.last_run.started_at)} · {j.last_run.triggered_by}
              {j.last_run.error && ` · ${j.last_run.error}`}
              {j.last_run.summary && <pre className="bg-card-head mt-1 overflow-x-auto rounded-lg px-2 py-1 text-[11px]">{JSON.stringify(j.last_run.summary)}</pre>}
            </div>
          ) : (
            <span className="text-muted text-[12px] font-semibold">never run</span>
          )}
        </Card>
      ))}
      <ErrorNote error={run.error} />
    </>
  );
}
