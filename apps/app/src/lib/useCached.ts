import { errorMessage } from '@chaudron/api';
import { useCallback, useEffect, useState } from 'react';

import { getItem, setItem } from './store';

interface Result<T> {
  data?: T;
  error?: unknown;
  response?: Response;
}

interface Cached<T> {
  key: string;
  data?: T;
  error: string | null;
  /** La donnée vient de l'appareil : le serveur n'a pas répondu. */
  offline: boolean;
}

/**
 * Comme `useLoad`, mais local d'abord : la dernière copie gardée sur l'appareil s'affiche tout de suite, puis la
 * réponse du serveur la remplace et est gardée à son tour. Sans réseau, la copie reste, avec `offline`.
 */
export function useCached<T>(key: string, load: () => Promise<Result<T>>) {
  const [state, setState] = useState<Cached<T>>({ key: '', error: null, offline: false });

  const fetchFresh = useCallback(async () => {
    try {
      const result = await load();
      if (result.data !== undefined) {
        void setItem(key, result.data);
        setState({ key, data: result.data, error: null, offline: false });
      } else {
        setState((s) => ({ key, data: s.key === key ? s.data : undefined, error: errorMessage(result.error), offline: false }));
      }
    } catch {
      // Pas de réseau : la copie de l'appareil, si elle existe.
      setState((s) => ({ key, data: s.key === key ? s.data : undefined, error: s.key === key && s.data !== undefined ? null : 'Hors ligne, et rien de gardé sur cet appareil.', offline: true }));
    }
    // `load` dépend de `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  useEffect(() => {
    let active = true;
    void getItem<T>(key).then((stored) => {
      if (active && stored !== undefined) setState((s) => (s.key === key && s.data !== undefined ? s : { key, data: stored, error: null, offline: false }));
    });
    // Après la lecture de la copie : le serveur la remplace s'il répond.
    void Promise.resolve().then(() => (active ? fetchFresh() : undefined));
    return () => {
      active = false;
    };
  }, [key, fetchFresh]);

  const setData = useCallback(
    (data: T) => {
      void setItem(key, data);
      setState({ key, data, error: null, offline: false });
    },
    [key],
  );

  const current = state.key === key ? state : { data: undefined, error: null, offline: false };
  return { data: current.data, error: current.error, offline: current.offline, reload: fetchFresh, setData };
}
