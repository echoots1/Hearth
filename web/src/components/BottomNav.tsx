import { NavLink } from "react-router";
import { Icon, type IconName } from "./ui";

const items: { to: string; label: string; icon: IconName }[] = [
  { to: "/", label: "Budget", icon: "home" },
  { to: "/activity", label: "Activity", icon: "list" },
  { to: "/plan", label: "Plan", icon: "calendar" },
  { to: "/goals", label: "Goals", icon: "target" },
  { to: "/more", label: "More", icon: "more" },
];

export function BottomNav({ badge }: { badge?: number }) {
  return (
    <nav
      aria-label="Main"
      className="bg-card fixed right-4 left-4 z-40 mx-auto grid h-16 max-w-md grid-cols-5 items-center rounded-[32px] px-2 shadow-[var(--shadow-nav)] lg:hidden"
      style={{ bottom: "max(1.5rem, env(safe-area-inset-bottom))" }}
    >
      {items.map((it) => (
        <NavLink
          key={it.to}
          to={it.to}
          end={it.to === "/"}
          className={({ isActive }) =>
            `relative flex h-14 flex-col items-center justify-center gap-0.5 text-[10px] ${isActive ? "text-ink font-extrabold" : "text-muted font-bold"}`
          }
        >
          <Icon name={it.icon} size={22} />
          {it.label}
          {it.to === "/activity" && !!badge && (
            <span className="bg-attention absolute top-1.5 right-[calc(50%-18px)] min-w-4 rounded-full px-1 text-center text-[10px] font-extrabold text-white">
              {badge}
            </span>
          )}
        </NavLink>
      ))}
    </nav>
  );
}
