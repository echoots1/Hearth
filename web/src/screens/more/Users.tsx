import { useState } from "react";
import { Card, ErrorNote, Field, Icon, PageHeader, Sheet, SheetActions, Spinner } from "../../components/ui";
import { api } from "../../lib/api";
import { useMutation, useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";
import type { UserInfo } from "../../lib/types";

export function Users() {
  const { me } = useSession();
  const users = useQuery<UserInfo[]>("users", () => api.get("/api/users"));
  const [add, setAdd] = useState(false);
  const [pw, setPw] = useState<UserInfo | null>(null);
  const owner = me?.role === "owner";
  if (!users.data) return <Spinner />;
  return (
    <>
      <PageHeader title="People" subtitle="who can sign in" back="/more" />
      <Card className="divider overflow-hidden">
        {users.data.map((u) => (
          <div key={u.id} className="row">
            <div className="flex flex-col gap-0.5">
              <span className="text-[15px] font-bold">{u.display_name}{u.id === me?.id && <span className="text-muted"> (you)</span>}</span>
              <span className="text-muted text-[12px] font-semibold">{u.email} · {u.role}</span>
            </div>
            {(owner || u.id === me?.id) && <button type="button" className="chip-light" onClick={() => setPw(u)}>Password</button>}
          </div>
        ))}
      </Card>
      {owner && <button type="button" className="btn-light" onClick={() => setAdd(true)}><Icon name="plus" size={16} /> Add a person</button>}
      <AddUserSheet open={add} onClose={() => setAdd(false)} />
      {pw && <PasswordSheet user={pw} onClose={() => setPw(null)} />}
    </>
  );
}

function AddUserSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [f, setF] = useState({ display_name: "", email: "", password: "", role: "member" });
  const m = useMutation(() => api.post("/api/users", f), ["users"]);
  return (
    <Sheet open={open} onClose={onClose} title="Add a person">
      <div className="flex flex-col gap-3">
        <Field label="Name"><input className="field" value={f.display_name} onChange={(e) => setF({ ...f, display_name: e.target.value })} autoFocus /></Field>
        <Field label="Email"><input className="field" type="email" value={f.email} onChange={(e) => setF({ ...f, email: e.target.value })} /></Field>
        <Field label="Starting password" hint="At least 10 characters. They can change it after signing in."><input className="field" type="text" value={f.password} onChange={(e) => setF({ ...f, password: e.target.value })} /></Field>
        <Field label="Role">
          <select className="field" value={f.role} onChange={(e) => setF({ ...f, role: e.target.value })}><option value="member">Member</option><option value="owner">Owner</option></select>
        </Field>
      </div>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || !f.display_name || !f.email || f.password.length < 10} onClick={async () => { if ((await m.run()) !== undefined) { setF({ display_name: "", email: "", password: "", role: "member" }); onClose(); } }}>Add</button>
      </SheetActions>
    </Sheet>
  );
}

function PasswordSheet({ user, onClose }: { user: UserInfo; onClose: () => void }) {
  const [password, setPassword] = useState("");
  const m = useMutation(() => api.post(`/api/users/${user.id}/password`, { password }), []);
  return (
    <Sheet open onClose={onClose} title={`New password for ${user.display_name}`}>
      <Field label="New password" hint="At least 10 characters. Their other sessions are signed out."><input className="field" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus /></Field>
      <ErrorNote error={m.error} />
      <SheetActions>
        <button type="button" className="btn-light" onClick={onClose}>Cancel</button>
        <button type="button" className="btn-dark" disabled={m.busy || password.length < 10} onClick={async () => { if ((await m.run()) !== undefined) onClose(); }}>Set password</button>
      </SheetActions>
    </Sheet>
  );
}
