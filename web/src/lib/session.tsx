/** Who is signed in, shared by every screen. */
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, setUnauthorizedHandler } from "./api";
import { clearCache } from "./query";
import type { AuthStatus, Me } from "./types";

type Session = {
  me: Me | null;
  needsSetup: boolean;
  ready: boolean;
  refresh: () => Promise<void>;
  signOut: () => Promise<void>;
  setMe: (m: Me) => void;
};

const Ctx = createContext<Session | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<Me | null>(null);
  const [needsSetup, setNeedsSetup] = useState(false);
  const [ready, setReady] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const s = await api.get<AuthStatus>("/api/auth/status");
      setNeedsSetup(s.needs_setup);
      if (s.signed_in) {
        setMe(await api.get<Me>("/api/auth/me"));
      } else {
        setMe(null);
      }
    } catch {
      setMe(null);
    } finally {
      setReady(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    setUnauthorizedHandler(() => {
      clearCache();
      setMe(null);
    });
  }, [refresh]);

  const signOut = useCallback(async () => {
    await api.post("/api/auth/logout").catch(() => {});
    clearCache();
    setMe(null);
  }, []);

  const value = useMemo(
    () => ({ me, needsSetup, ready, refresh, signOut, setMe: (m: Me) => { setMe(m); setNeedsSetup(false); } }),
    [me, needsSetup, ready, refresh, signOut],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useSession(): Session {
  const s = useContext(Ctx);
  if (!s) throw new Error("useSession outside SessionProvider");
  return s;
}
