import { BrowserRouter, Navigate, Outlet, Route, Routes } from "react-router";
import { BottomNav } from "./components/BottomNav";
import { Spinner } from "./components/ui";
import { useInboxCounts } from "./lib/data";
import { SessionProvider, useSession } from "./lib/session";
import { Login, Setup } from "./screens/Auth";
import { Activity } from "./screens/Activity";
import { Budget } from "./screens/Budget";
import { CategoryScreen } from "./screens/Category";
import { Goals } from "./screens/Goals";
import { More } from "./screens/More";
import { Plan } from "./screens/Plan";
import { Accounts } from "./screens/more/Accounts";
import { ApiKeys } from "./screens/more/ApiKeys";
import { Assistant } from "./screens/more/Assistant";
import { Audit } from "./screens/more/Audit";
import { Integrations } from "./screens/more/Integrations";
import { Notifications } from "./screens/more/Notifications";
import { Rules } from "./screens/more/Rules";
import { SettingsScreen } from "./screens/more/Settings";
import { Users } from "./screens/more/Users";

function Shell() {
  const counts = useInboxCounts();
  return (
    <div className="mx-auto flex min-h-full max-w-md flex-col gap-4 px-4 pb-32 pt-[max(1rem,env(safe-area-inset-top))]">
      <Outlet />
      <BottomNav badge={counts.total} />
    </div>
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
        <Route path="plan" element={<Plan />} />
        <Route path="goals" element={<Goals />} />
        <Route path="more" element={<More />} />
        <Route path="more/accounts" element={<Accounts />} />
        <Route path="more/rules" element={<Rules />} />
        <Route path="more/settings" element={<SettingsScreen />} />
        <Route path="more/assistant" element={<Assistant />} />
        <Route path="more/notifications" element={<Notifications />} />
        <Route path="more/users" element={<Users />} />
        <Route path="more/keys" element={<ApiKeys />} />
        <Route path="more/integrations" element={<Integrations />} />
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
