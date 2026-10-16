import type { Me, ProviderInfo } from '@chaudron/api';
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from 'react';

import { api } from './api';

interface SessionContext {
  /** `undefined` : chargement ; `null` : pas connecté. */
  me: Me | null | undefined;
  providers: ProviderInfo[];
  /** Relit le compte (après une connexion, la création d'un foyer…). */
  refresh: () => Promise<Me | null>;
  logout: () => Promise<void>;
  /** Le serveur ne répond pas. */
  offline: boolean;
}

const Context = createContext<SessionContext | null>(null);

interface Loaded {
  me: Me | null;
  providers?: ProviderInfo[];
  offline: boolean;
}

/** Le compte, et les fournisseurs de connexion si on n'est pas connecté. */
async function load(): Promise<Loaded> {
  try {
    const { data, response } = await api.GET('/api/v1/me');
    const me = response.ok && data ? data : null;
    if (me) return { me, offline: false };
    const list = await api.GET('/api/v1/auth/providers');
    return { me: null, providers: list.data ?? [], offline: false };
  } catch {
    return { me: null, offline: true };
  }
}

export function SessionProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [offline, setOffline] = useState(false);

  const apply = useCallback((state: Loaded) => {
    setOffline(state.offline);
    if (state.offline) setMe((current) => (current === undefined ? null : current));
    else setMe(state.me);
    if (state.providers) setProviders(state.providers);
    return state.me;
  }, []);

  const refresh = useCallback(async () => apply(await load()), [apply]);

  const logout = useCallback(async () => {
    await api.POST('/api/v1/auth/logout').catch(() => undefined);
    await refresh();
  }, [refresh]);

  useEffect(() => {
    let active = true;
    void load().then((state) => active && apply(state));
    return () => {
      active = false;
    };
  }, [apply]);

  const value = useMemo(
    () => ({ me, providers, refresh, logout, offline }),
    [me, providers, refresh, logout, offline],
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useSession(): SessionContext {
  const context = useContext(Context);
  if (!context) throw new Error('useSession hors de SessionProvider');
  return context;
}
