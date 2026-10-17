// Courses hors ligne : les opérations en attente (cases cochées au magasin sans réseau) s'appliquent sur la liste
// lue du serveur, pour que l'écran ne revienne pas en arrière avant leur envoi.

export type Block = 'recipes' | 'breakfast_desserts' | 'special';

/** Opération mise en file (`POST /shopping/ops`). */
export interface PendingOp {
  op_id: string;
  at: string;
  week_start: string;
  kind: 'check' | 'uncheck' | 'add' | 'edit' | 'remove';
  key: string;
  label?: string | null;
  qty?: number | null;
  block?: Block | null;
}

interface Item {
  key: string;
  label: string;
  checked: boolean;
  manual: boolean;
  block: Block;
  qty?: number | null;
  grown?: number | null;
}

/** La liste, avec les opérations en attente de ce menu appliquées dans l'ordre. */
export function applyPending<I extends Item, L extends { week_start: string; items: I[] }>(list: L, pending: PendingOp[]): L {
  let items = [...list.items];
  for (const op of pending.filter((o) => o.week_start === list.week_start)) {
    switch (op.kind) {
      case 'check':
      case 'uncheck': {
        const checked = op.kind === 'check';
        items = items.map((i) => (i.key === op.key ? { ...i, checked, grown: checked ? null : i.grown } : i));
        break;
      }
      case 'add':
        if (!items.some((i) => i.key === op.key)) {
          items.push({ key: op.key, label: op.label ?? '', checked: false, manual: true, block: op.block ?? 'special', qty: op.qty ?? null, aisle: 'other', pantry: false, optional: false, recipes: [] } as unknown as I);
        }
        break;
      case 'edit':
        items = items.map((i) => (i.key === op.key ? { ...i, label: op.label ?? i.label, qty: op.qty ?? i.qty, block: op.block ?? i.block } : i));
        break;
      case 'remove':
        items = items.filter((i) => i.key !== op.key);
        break;
    }
  }
  return { ...list, items };
}
