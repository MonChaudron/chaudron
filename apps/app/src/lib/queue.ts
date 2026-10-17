import type { PendingOp } from '@chaudron/domain';
import { useCallback, useEffect, useRef, useState } from 'react';

import { api } from './api';
import { getItem, setItem } from './store';

/**
 * File des opérations de courses : écrites sur l'appareil, envoyées dès que le serveur répond (tout de suite, au
 * retour du réseau, et toutes les 20 s tant qu'il en reste). Le serveur les rejoue sans doublon (`op_id`) et garde la
 * dernière écriture, article par article.
 */
export function useShoppingQueue(householdId: string, onFlushed: () => void) {
  const key = `queue:${householdId}`;
  const [pending, setPending] = useState<PendingOp[]>([]);
  const [error, setError] = useState<string | null>(null);
  const queue = useRef<PendingOp[]>([]);
  const flushing = useRef(false);
  const flushed = useRef(onFlushed);
  useEffect(() => {
    flushed.current = onFlushed;
  });

  const save = useCallback(
    (next: PendingOp[]) => {
      queue.current = next;
      setPending(next);
      void setItem(key, next);
    },
    [key],
  );

  const flush = useCallback(async () => {
    if (flushing.current || queue.current.length === 0) return;
    flushing.current = true;
    const batch = queue.current;
    try {
      const { error: failure, response } = await api.POST('/api/v1/households/{id}/shopping/ops', {
        params: { path: { id: householdId } },
        body: { ops: batch },
      });
      if (response.ok || (response.status >= 400 && response.status < 500)) {
        // Envoyées, ou refusées pour de bon (menu supprimé…) : elles quittent la file.
        save(queue.current.filter((op) => !batch.includes(op)));
        setError(response.ok ? null : ((failure as { message?: string } | undefined)?.message ?? 'Opération refusée.'));
        flushed.current();
      }
    } catch {
      // Toujours hors ligne : on réessaiera.
    } finally {
      flushing.current = false;
    }
  }, [householdId, save]);

  useEffect(() => {
    let active = true;
    void getItem<PendingOp[]>(key).then((stored) => {
      if (!active) return;
      queue.current = [...(stored ?? []), ...queue.current];
      setPending(queue.current);
      void flush();
    });
    const online = () => void flush();
    if (typeof window !== 'undefined') window.addEventListener('online', online);
    const timer = setInterval(() => void flush(), 20_000);
    return () => {
      active = false;
      if (typeof window !== 'undefined') window.removeEventListener('online', online);
      clearInterval(timer);
    };
  }, [key, flush]);

  const enqueue = useCallback(
    (op: PendingOp) => {
      save([...queue.current, op]);
      void flush();
    },
    [save, flush],
  );

  return { pending, enqueue, flush, error };
}
