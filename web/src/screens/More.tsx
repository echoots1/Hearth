import { Link } from "react-router";
import { Card, Icon, PageHeader, type IconName } from "../components/ui";
import { useSession } from "../lib/session";

const items: { to: string; label: string; hint: string; icon: IconName }[] = [
  { to: "/chat", label: "Chat with Claude", hint: "API mode", icon: "sparkle" },
  { to: "/more/accounts", label: "Accounts", hint: "checking, savings, cards", icon: "bank" },
  { to: "/more/rules", label: "Rules", hint: "categorize on sight", icon: "rule" },
  { to: "/more/settings", label: "Settings", hint: "paydays, floor, thresholds, quiet hours", icon: "settings" },
  { to: "/more/assistant", label: "Assistant", hint: "Pull or API mode · Backseat / Driver", icon: "sparkle" },
  { to: "/more/notifications", label: "Notifications", hint: "push on this device", icon: "bell" },
  { to: "/more/users", label: "People", hint: "who can sign in", icon: "users" },
  { to: "/more/server", label: "Server", hint: "public URL, proxy, schedules", icon: "settings" },
  { to: "/more/keys", label: "API keys", hint: "for the Claude connector", icon: "key" },
  { to: "/more/integrations", label: "Integrations", hint: "bank feeds and imports", icon: "plug" },
  { to: "/more/jobs", label: "Jobs", hint: "sync, process, evaluate, notify", icon: "clock" },
  { to: "/more/audit", label: "Audit log", hint: "every change, who and when", icon: "list" },
];

export function More() {
  const { me, signOut } = useSession();
  return (
    <>
      <PageHeader title="More" subtitle={me ? `${me.display_name} · ${me.household}` : undefined} />
      <Card className="divider overflow-hidden">
        {items.map((it) => (
          <Link key={it.to} to={it.to} className="row">
            <span className="flex items-center gap-3">
              <span className="bg-card-head text-ink flex h-9 w-9 items-center justify-center rounded-full"><Icon name={it.icon} size={18} /></span>
              <span className="flex flex-col">
                <span className="text-[15px] font-bold">{it.label}</span>
                <span className="text-muted text-[12px] font-semibold">{it.hint}</span>
              </span>
            </span>
            <Icon name="chevron" size={18} className="text-faint" />
          </Link>
        ))}
      </Card>
      <button type="button" className="btn-light" onClick={() => signOut()}><Icon name="logout" size={16} /> Sign out</button>
      <p className="text-faint text-center text-[11px] font-semibold">Hearth · household money, on plan. · MIT</p>
    </>
  );
}
