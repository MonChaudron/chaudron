import { errorMessage } from '@chaudron/api';
import { useCallback, useEffect, useState } from 'react';

/** Résultat d'un appel d'API openapi-fetch. */
interface Result<T> {
  data?: T;
  error?: unknown;
}

interface Loaded<T> {
  key: string;
  data?: T;
  error: string | null;
}

async function run<T>(load: () => Promise<Result<T>>, key: string): Promise<Loaded<T>> {
  try {
    const result = await load();
    return result.data !== undefined
      ? { key, data: result.data, error: null }
      : { key, error: errorMessage(result.error) };
  } catch (err) {
    return { key, error: errorMessage(err) };
  }
}

/**
 * Charge une donnée de l'API, la recharge quand `deps` change ou à la demande. Pas de bibliothèque de cache : les
 * écrans du foyer sont petits, et le local-first (M3) viendra avec son propre stockage.
 */
export function useLoad<T>(load: () => Promise<Result<T>>, deps: unknown[]) {
  const key = JSON.stringify(deps);
  const [loaded, setLoaded] = useState<Loaded<T>>({ key: '', error: null });

  useEffect(() => {
    let active = true;
    void run(load, key).then((next) => active && setLoaded(next));
    return () => {
      active = false;
    };
    // `load` dépend de `deps`, résumés par `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const reload = useCallback(async () => {
    setLoaded(await run(load, key));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const setData = useCallback((data: T) => setLoaded({ key, data, error: null }), [key]);

  // Donnée d'avant un changement de `deps` : on ne la montre pas.
  const current = loaded.key === key ? loaded : { data: undefined, error: null };
  return { data: current.data, error: current.error, reload, setData };
}
