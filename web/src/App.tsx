import { useEffect, useState } from "react";

type Health = { status: string; version: string; database: string };

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/health")
      .then(async (r) => setHealth((await r.json()) as Health))
      .catch((e: unknown) => setError(String(e)));
  }, []);

  return (
    <div className="mx-auto flex min-h-full max-w-md flex-col px-4 pb-8 pt-[max(1rem,env(safe-area-inset-top))]">
      <header className="flex items-baseline gap-2 py-3">
        <span className="text-2xl font-semibold tracking-tight">Hearth</span>
        <span className="text-sm text-stone-500">household money, on plan.</span>
      </header>
      <main className="mt-6 rounded-2xl border border-stone-200 bg-white p-4 shadow-sm">
        <h1 className="text-base font-medium">Server status</h1>
        {error && <p className="mt-2 text-sm text-red-600">{error}</p>}
        {!health && !error && <p className="mt-2 text-sm text-stone-500">Checking…</p>}
        {health && (
          <dl className="mt-2 grid grid-cols-2 gap-y-1 text-sm">
            <dt className="text-stone-500">Status</dt>
            <dd>{health.status}</dd>
            <dt className="text-stone-500">Database</dt>
            <dd>{health.database}</dd>
            <dt className="text-stone-500">Version</dt>
            <dd>{health.version}</dd>
          </dl>
        )}
      </main>
    </div>
  );
}
