import { useEffect, useRef } from 'react';

import { apiBase } from './api';

export interface HouseholdEvent {
  kind: 'shopping' | 'menu' | 'resync';
  week_start?: string;
  cursor?: number;
}

/**
 * Écoute le flux SSE du foyer (« il y a du nouveau ») tant que l'écran est monté. EventSource se reconnecte seul ;
 * à la reconnexion, `onEvent` reçoit `resync` pour tout relire.
 */
export function useHouseholdEvents(householdId: string, onEvent: (event: HouseholdEvent) => void) {
  const handler = useRef(onEvent);
  useEffect(() => {
    handler.current = onEvent;
  });
  useEffect(() => {
    if (typeof EventSource === 'undefined') return;
    const source = new EventSource(`${apiBase}/api/v1/households/${householdId}/events`, { withCredentials: true });
    let opened = false;
    const listen = (kind: HouseholdEvent['kind']) => (e: MessageEvent) => {
      let data: Partial<HouseholdEvent> = {};
      try {
        data = JSON.parse(e.data);
      } catch {
        // Données illisibles : on relit tout.
      }
      handler.current({ ...data, kind });
    };
    source.addEventListener('shopping', listen('shopping'));
    source.addEventListener('menu', listen('menu'));
    source.addEventListener('resync', listen('resync'));
    source.onopen = () => {
      if (opened) handler.current({ kind: 'resync' });
      opened = true;
    };
    return () => source.close();
  }, [householdId]);
}

/** Identifiant d'une opération, choisi par le téléphone (rejouer la même opération ne fait rien). */
export function newId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID();
  const hex = Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16));
  hex[12] = '4';
  hex[16] = ((parseInt(hex[16], 16) & 3) | 8).toString(16);
  const s = hex.join('');
  return `${s.slice(0, 8)}-${s.slice(8, 12)}-${s.slice(12, 16)}-${s.slice(16, 20)}-${s.slice(20)}`;
}
