import { useEffect, useState } from "react";
import { ConfigForm } from "../../components/ConfigForm";
import { Card, CardHead, ErrorNote, PageHeader, Toggle } from "../../components/ui";
import { api } from "../../lib/api";
import { currentSubscription, pushSupported, subscribePush, unsubscribePush } from "../../lib/push";
import { useQuery } from "../../lib/query";
import { useSession } from "../../lib/session";

type Source = "app" | "env" | null;
type Status = {
  encryption_configured: boolean;
  ntfy: { configured: boolean; source: Source; url: string | null; topic: string | null; has_token: boolean };
  smtp: { configured: boolean; source: Source; host: string | null; port: number | null; user: string | null; has_pass: boolean; from: string | null; alert_to: string | null };
};

export function Notifications() {
  const { me } = useSession();
  const owner = me?.role === "owner";
  const status = useQuery<Status>("integrations.status", () => api.get("/api/integrations/status"));
  const [on, setOn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const supported = pushSupported();
  useEffect(() => {
    currentSubscription().then((s) => setOn(!!s)).catch(() => {});
  }, []);
  const toggle = async (v: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (v) await subscribePush();
      else await unsubscribePush();
      setOn(v);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const standalone = window.matchMedia("(display-mode: standalone)").matches;
  const s = status.data;
  const label = (c: { configured: boolean; source: Source }) => (c.configured ? (c.source === "env" ? "from environment" : "on") : "off");
  return (
    <>
      <PageHeader title="Notifications" back="/more" />
      <Card className="flex flex-col gap-3 p-4">
        <Toggle label="Push on this device" hint={supported ? "Alerts, questions from Claude, and the low-point warning." : "This browser can't receive web push."} checked={on} onChange={toggle} />
        {busy && <span className="text-muted text-[12px] font-semibold">Working…</span>}
        <ErrorNote error={error} onClose={() => setError(null)} />
      </Card>
      {!standalone && (
        <Card className="flex flex-col gap-1 p-4">
          <span className="text-[13px] font-extrabold">Install Hearth</span>
          <p className="text-muted text-[13px] font-semibold">Add it to your home screen for a full-screen app with its own icon. On iPhone and iPad, notifications only work once it's installed: Share → Add to Home Screen, then turn push on from inside the installed app.</p>
        </Card>
      )}
      {s && (
        <Card className="divider overflow-hidden">
          <CardHead title="ntfy" right={label(s.ntfy)} />
          <div className="flex flex-col gap-3 p-4">
            <p className="text-muted text-[13px] font-semibold">Every alert as a push through <a className="underline" href="https://ntfy.sh" target="_blank" rel="noreferrer">ntfy</a>: subscribe to a topic in the ntfy app, put the same topic here. Works with ntfy.sh or your own server.</p>
            <ConfigForm
              name="ntfy"
              canEdit={owner}
              configured={s.ntfy.configured}
              source={s.ntfy.source}
              hasSecret={s.ntfy.has_token}
              encryptionConfigured={s.encryption_configured}
              initial={{ url: s.ntfy.url ?? "https://ntfy.sh", topic: s.ntfy.topic ?? "", token: "" }}
              fields={[
                { key: "url", label: "Server", type: "url", placeholder: "https://ntfy.sh" },
                { key: "topic", label: "Topic", required: true, placeholder: "hearth-household-a7f3", hint: "Treat it like a password: anyone who knows it can read the alerts." },
                { key: "token", label: "Access token", secret: true, hint: "Only if the topic is protected." },
              ]}
            />
          </div>
        </Card>
      )}
      {s && (
        <Card className="divider overflow-hidden">
          <CardHead title="Email" right={label(s.smtp)} />
          <div className="flex flex-col gap-3 p-4">
            <p className="text-muted text-[13px] font-semibold">Plain-text alerts over SMTP with STARTTLS. Any mail provider's SMTP settings work.</p>
            <ConfigForm
              name="smtp"
              canEdit={owner}
              configured={s.smtp.configured}
              source={s.smtp.source}
              hasSecret={s.smtp.has_pass}
              encryptionConfigured={s.encryption_configured}
              testLabel="Send a test email"
              initial={{ host: s.smtp.host ?? "", port: String(s.smtp.port ?? 587), user: s.smtp.user ?? "", pass: "", from: s.smtp.from ?? "", alert_to: s.smtp.alert_to ?? me?.email ?? "" }}
              fields={[
                { key: "host", label: "SMTP host", required: true, placeholder: "smtp.example.com" },
                { key: "port", label: "Port", type: "number", placeholder: "587" },
                { key: "user", label: "Username", placeholder: "usually the sending address" },
                { key: "pass", label: "Password", secret: true },
                { key: "from", label: "From address", type: "email", required: true, placeholder: "hearth@example.com" },
                { key: "alert_to", label: "Send alerts to", type: "email", required: true },
              ]}
            />
          </div>
        </Card>
      )}
      <ErrorNote error={status.error?.message} />
      <Card className="flex flex-col gap-1 p-4">
        <span className="text-[13px] font-extrabold">Timing</span>
        <p className="text-muted text-[13px] font-semibold">Quiet hours and the nag schedule live under Settings.</p>
      </Card>
    </>
  );
}
