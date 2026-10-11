/**
 * The HTTP client. Reads and writes go through `/api/tools/{name}` (the same
 * registry the MCP server exposes); auth, keys, users, push and proposals
 * have their own endpoints.
 */

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

let onUnauthorized: (() => void) | null = null;
export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorized = fn;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  if (res.status === 401 && !path.startsWith("/api/auth/")) onUnauthorized?.();
  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { error: text };
    }
  }
  if (!res.ok) {
    const msg =
      (data && typeof data === "object" && "error" in data && String((data as { error: unknown }).error)) ||
      `${res.status} ${res.statusText}`;
    throw new ApiError(res.status, String(msg));
  }
  return data as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body ?? {}),
  del: <T>(path: string) => request<T>("DELETE", path),
};

/** Call a registry tool as the signed-in person. */
export function tool<T>(name: string, args: Record<string, unknown> = {}): Promise<T> {
  return request<T>("POST", `/api/tools/${name}`, args);
}
