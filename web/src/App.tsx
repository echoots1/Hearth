import { BrowserRouter, Navigate, Outlet, Route, Routes } from "react-router";
import { BottomNav } from "./components/BottomNav";
import { SideNav } from "./components/SideNav";
import { Spinner } from "./components/ui";
import { useInboxCounts } from "./lib/data";
import { SessionProvider, useSession } from "./lib/session";
import { Login, Setup } from "./screens/Auth";
import { Activity } from "./screens/Activity";
import { Budget } from "./screens/Budget";
import { CategoryScreen } from "./screens/Category";
import { Chat } from "./screens/Chat";
import { Goals } from "./screens/Goals";
import { More } from "./screens/More";
import { Plan } from "./screens/Plan";
import { Accounts } from "./screens/more/Accounts";
import { ApiKeys } from "./screens/more/ApiKeys";
import { Assistant } from "./screens/more/Assistant";
import { Audit } from "./screens/more/Audit";
import { Integrations } from "./screens/more/Integrations";
import { Jobs } from "./screens/more/Jobs";
import { Notifications } from "./screens/more/Notifications";
import { Server } from "./screens/more/Server";
import { Rules } from "./screens/more/Rules";
import { SettingsScreen } from "./screens/more/Settings";
import { Users } from "./screens/more/Users";

function Shell() {
  const counts = useInboxCounts();
  return (
    <>
      {/* Phones get the bottom tab bar; from `lg` up a fixed left rail takes its place and the page widens. */}
      <SideNav badge={counts.total} />
      <div className="lg:pl-64">
        <main className="mx-auto flex min-h-full max-w-md flex-col gap-4 px-4 pb-32 pt-[max(1rem,env(safe-area-inset-top))] lg:max-w-3xl lg:gap-5 lg:px-10 lg:pt-8 lg:pb-12">
          <Outlet />
        </main>
      </div>
      <BottomNav badge={counts.total} />
    </>
  );
}

function Gate() {
  const { ready, me, needsSetup } = useSession();
  if (!ready) return <Spinner label="Starting…" />;
  if (needsSetup) return <Setup />;
  if (!me) return <Login />;
  return (
    <Routes>
      <Route element={<Shell />}>
        <Route index element={<Budget />} />
        <Route path="category/:id" element={<CategoryScreen />} />
        <Route path="activity" element={<Activity />} />
        <Route path="chat" element={<Chat />} />
        <Route path="plan" element={<Plan />} />
        <Route path="goals" element={<Goals />} />
        <Route path="more" element={<More />} />
        <Route path="more/accounts" element={<Accounts />} />
        <Route path="more/rules" element={<Rules />} />
        <Route path="more/settings" element={<SettingsScreen />} />
        <Route path="more/server" element={<Server />} />
        <Route path="more/assistant" element={<Assistant />} />
        <Route path="more/notifications" element={<Notifications />} />
        <Route path="more/users" element={<Users />} />
        <Route path="more/keys" element={<ApiKeys />} />
        <Route path="more/integrations" element={<Integrations />} />
        <Route path="more/jobs" element={<Jobs />} />
        <Route path="more/audit" element={<Audit />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <SessionProvider>
        <Gate />
      </SessionProvider>
    </BrowserRouter>
  );
}
