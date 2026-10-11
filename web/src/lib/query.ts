/**
 * A very small read cache: `useQuery(key, fetcher)` shares results by key,
 * `invalidate(prefix)` refetches everything whose key starts with the prefix.
 * Enough for one household on one screen at a time; no library needed.
 */
import { useCallback, useEffect, useRef, useState } from "react";

type Entry = { data?: unknown; error?: Error; promise?: Promise<unknown>; at: number };
const cache = new Map<string, Entry>();
const listeners = new Map<string, Set<() => void>>();

function notify(key: string) {
  listeners.get(key)?.forEach((fn) => fn());
}

async function load(key: string, fetcher: () => Promise<unknown>) {
  const entry = cache.get(key) ?? { at: 0 };
  const p = fetcher()
    .then((data) => {
      cache.set(key, { data, at: Date.now() });
      notify(key);
      return data;
    })
    .catch((error: Error) => {
      cache.set(key, { ...(cache.get(key) ?? { at: 0 }), error, promise: undefined });
      notify(key);
      throw error;
    });
  cache.set(key, { ...entry, promise: p, error: undefined });
  return p;
}

const fetchers = new Map<string, () => Promise<unknown>>();

/** Refetch every live query whose key starts with any of the prefixes. */
export function invalidate(...prefixes: string[]) {
  for (const [key, fetcher] of fetchers) {
    if (prefixes.length === 0 || prefixes.some((p) => key.startsWith(p))) {
      void load(key, fetcher).catch(() => {});
    }
  }
}

export function clearCache() {
  cache.clear();
  fetchers.clear();
}

export type Query<T> = {
  data: T | undefined;
  error: Error | undefined;
  loading: boolean;
  refetch: () => Promise<void>;
};

export function useQuery<T>(key: string | null, fetcher: () => Promise<T>): Query<T> {
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const [, bump] = useState(0);

  useEffect(() => {
    if (!key) return;
    const rerender = () => bump((n) => n + 1);
    const set = listeners.get(key) ?? new Set();
    set.add(rerender);
    listeners.set(key, set);
    const f = () => fetcherRef.current();
    fetchers.set(key, f);
    const entry = cache.get(key);
    if (!entry || (!entry.promise && Date.now() - entry.at > 1500)) {
      void load(key, f).catch(() => {});
    }
    return () => {
      set.delete(rerender);
      if (set.size === 0) {
        listeners.delete(key);
        fetchers.delete(key);
      }
    };
  }, [key]);

  const refetch = useCallback(async () => {
    if (!key) return;
    await load(key, () => fetcherRef.current()).catch(() => {});
  }, [key]);

  const entry = key ? cache.get(key) : undefined;
  return {
    data: entry?.data as T | undefined,
    error: entry?.error,
    loading: !!key && (!entry || (!!entry.promise && entry.data === undefined)),
    refetch,
  };
}

/** Run a write, then refetch the given prefixes. Returns [run, busy, error]. */
export function useMutation<A extends unknown[], R>(
  fn: (...args: A) => Promise<R>,
  invalidates: string[] = [""],
) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(
    async (...args: A): Promise<R | undefined> => {
      setBusy(true);
      setError(null);
      try {
        const r = await fn(...args);
        invalidate(...invalidates);
        return r;
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return undefined;
      } finally {
        setBusy(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [fn, invalidates.join("|")],
  );
  return { run, busy, error, clearError: () => setError(null) };
}
