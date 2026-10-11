/**
 * Desktop navigation. Rendered from `lg` up in place of the bottom tab bar:
 * a fixed left rail with the wordmark, the main sections, the pages that live
 * under "More" on a phone, and who is signed in.
 */
import { NavLink } from "react-router";
import { useToday } from "../lib/data";
import { useSession } from "../lib/session";
import { Icon, Wordmark, type IconName } from "./ui";

type Item = { to: string; label: string; icon: IconName; hint?: string };

const main: Item[] = [
  { to: "/", label: "Budget", icon: "home" },
  { to: "/activity", label: "Activity", icon: "list" },
  { to: "/plan", label: "Plan", icon: "calendar" },
  { to: "/goals", label: "Goals", icon: "target" },
];

const manage: Item[] = [
  { to: "/more/accounts", label: "Accounts", icon: "bank" },
  { to: "/more/rules", label: "Rules", icon: "rule" },
  { to: "/more/settings", label: "Settings", icon: "settings" },
  { to: "/more/assistant", label: "Assistant", icon: "sparkle" },
  { to: "/more/notifications", label: "Notifications", icon: "bell" },
  { to: "/more/users", label: "People", icon: "users" },
  { to: "/more/integrations", label: "Integrations", icon: "plug" },
];

const server: Item[] = [
  { to: "/more/server", label: "Server", icon: "settings" },
  { to: "/more/keys", label: "API keys", icon: "key" },
  { to: "/more/jobs", label: "Jobs", icon: "clock" },
  { to: "/more/audit", label: "Audit log", icon: "list" },
];

function NavItem({ item, badge, compact }: { item: Item; badge?: number; compact?: boolean }) {
  return (
    <NavLink
      to={item.to}
      end={item.to === "/"}
      className={({ isActive }) =>
        `flex items-center gap-3 rounded-xl px-3 font-bold transition ${compact ? "h-8 text-[13px]" : "h-10 text-[14px]"} ${
          isActive ? "bg-ink text-ground" : "text-ink-2 hover:bg-card-head"
        }`
      }
    >
      <Icon name={item.icon} size={compact ? 16 : 20} />
      <span className="flex-1 truncate">{item.label}</span>
      {!!badge && <span className="bg-attention min-w-5 rounded-full px-1.5 py-0.5 text-center text-[11px] font-extrabold text-white">{badge}</span>}
    </NavLink>
  );
}

export function SideNav({ badge }: { badge?: number }) {
  const { me, signOut } = useSession();
  const today = useToday();
  const chat = today.data?.assistant.mode === "api";
  return (
    <nav
      aria-label="Main"
      className="bg-card border-line fixed inset-y-0 left-0 z-40 hidden w-64 flex-col gap-4 overflow-y-auto border-r px-4 pt-5 pb-4 lg:flex"
    >
      <div className="px-1">
        <Wordmark />
      </div>

      <div className="flex flex-col gap-0.5">
        {main.map((it) => (
          <NavItem key={it.to} item={it} badge={it.to === "/activity" ? badge : undefined} />
        ))}
        {chat && <NavItem item={{ to: "/chat", label: "Chat with Claude", icon: "sparkle" }} />}
      </div>

      <div className="flex flex-col gap-0.5">
        <span className="label px-3 pb-1">Manage</span>
        {manage.map((it) => (
          <NavItem key={it.to} item={it} compact />
        ))}
      </div>

      <div className="flex flex-col gap-0.5">
        <span className="label px-3 pb-1">Server</span>
        {server.map((it) => (
          <NavItem key={it.to} item={it} compact />
        ))}
      </div>

      <div className="border-line mt-auto flex items-center justify-between gap-2 border-t pt-3">
        <div className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-[13px] font-bold">{me?.display_name}</span>
          <span className="text-muted truncate text-[11px] font-semibold">{me?.household}</span>
        </div>
        <button type="button" onClick={() => signOut()} className="icon-btn shrink-0 shadow-none hover:bg-card-head" aria-label="Sign out" title="Sign out">
          <Icon name="logout" size={18} />
        </button>
      </div>
    </nav>
  );
}
