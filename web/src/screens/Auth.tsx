import { useState, type FormEvent } from "react";
import { Card, ErrorNote, Field, Wordmark } from "../components/ui";
import { api } from "../lib/api";
import { useSession } from "../lib/session";
import type { Me } from "../lib/types";

function Frame({ children, title, intro }: { children: React.ReactNode; title: string; intro: string }) {
  return (
    <div className="mx-auto flex min-h-full max-w-md flex-col gap-5 px-4 pt-[max(2.5rem,env(safe-area-inset-top))] pb-10">
      <Wordmark />
      <Card className="flex flex-col gap-4 p-5">
        <div>
          <h1 className="text-[20px] font-extrabold">{title}</h1>
          <p className="text-muted mt-1 text-[13px] font-semibold">{intro}</p>
        </div>
        {children}
      </Card>
    </div>
  );
}

const guessTz = () => {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
};

export function Setup() {
  const { setMe } = useSession();
  const [f, setF] = useState({ setup_token: "", household_name: "", timezone: guessTz(), email: "", display_name: "", password: "" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setMe(await api.post<Me>("/api/auth/setup", f));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement>) => setF({ ...f, [k]: e.target.value });
  return (
    <Frame title="Welcome to Hearth" intro="First run: name the household and create the owner account. Everything stays on this server.">
      <form onSubmit={submit} className="flex flex-col gap-3">
        <Field label="Setup token" hint="Printed in the server log when it starts with no accounts: `docker compose logs app` (TrueNAS: Installed → hearth → Logs), the line that says “setup token”. It proves you run this server.">
          <input className="field" required value={f.setup_token} onChange={set("setup_token")} autoComplete="off" spellCheck={false} autoFocus />
        </Field>
        <Field label="Household name"><input className="field" required value={f.household_name} onChange={set("household_name")} placeholder="The Example household" /></Field>
        <Field label="Timezone" hint="IANA name, e.g. America/Chicago. Dates and paydays use it."><input className="field" required value={f.timezone} onChange={set("timezone")} /></Field>
        <Field label="Your name"><input className="field" required value={f.display_name} onChange={set("display_name")} autoComplete="name" /></Field>
        <Field label="Email"><input className="field" required type="email" value={f.email} onChange={set("email")} autoComplete="username" /></Field>
        <Field label="Password" hint="At least 10 characters."><input className="field" required type="password" minLength={10} value={f.password} onChange={set("password")} autoComplete="new-password" /></Field>
        <ErrorNote error={error} />
        <button type="submit" className="btn-dark mt-1" disabled={busy}>{busy ? "Setting up…" : "Create household"}</button>
      </form>
    </Frame>
  );
}

export function Login() {
  const { setMe } = useSession();
  const [f, setF] = useState({ email: "", password: "" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setMe(await api.post<Me>("/api/auth/login", f));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Frame title="Sign in" intro="Your household's budget, on your own server.">
      <form onSubmit={submit} className="flex flex-col gap-3">
        <Field label="Email"><input className="field" required type="email" value={f.email} onChange={(e) => setF({ ...f, email: e.target.value })} autoComplete="username" autoFocus /></Field>
        <Field label="Password"><input className="field" required type="password" value={f.password} onChange={(e) => setF({ ...f, password: e.target.value })} autoComplete="current-password" /></Field>
        <ErrorNote error={error} />
        <button type="submit" className="btn-dark mt-1" disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
    </Frame>
  );
}
