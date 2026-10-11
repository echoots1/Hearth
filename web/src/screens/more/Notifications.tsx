import { useEffect, useState } from "react";
import { Card, ErrorNote, PageHeader, Toggle } from "../../components/ui";
import { currentSubscription, pushSupported, subscribePush, unsubscribePush } from "../../lib/push";

export function Notifications() {
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
      <Card className="flex flex-col gap-1 p-4">
        <span className="text-[13px] font-extrabold">Other channels</span>
        <p className="text-muted text-[13px] font-semibold">ntfy and email are set in the server's environment (NTFY_TOPIC, SMTP_HOST). Quiet hours and the nag schedule live under Settings.</p>
      </Card>
    </>
  );
}
