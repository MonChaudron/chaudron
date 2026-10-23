// Courses hors ligne : les opérations en attente (cases cochées au magasin sans réseau) s'appliquent sur la liste
// lue du serveur, pour que l'écran ne revienne pas en arrière avant leur envoi.

export type Block = 'recipes' | 'breakfast_desserts' | 'special';

/** Opération mise en file (`POST /shopping/ops`). */
export interface PendingOp {
  op_id: string;
  at: string;
  week_start: string;
  kind: 'check' | 'uncheck' | 'add' | 'edit' | 'remove' | 'substitute';
  key: string;
  label?: string | null;
  qty?: number | null;
  block?: Block | null;
  /** `substitute` : le remplacement choisi ; nul, retour à l'ingrédient. */
  substitution_id?: string | null;
}

/** Un remplacement proposé sur une ligne. */
interface Offer {
  id: string;
  ingredient: string;
  label: string;
  qty?: number | null;
  price_cents?: number | null;
}

interface Item {
  key: string;
  label: string;
  checked: boolean;
  manual: boolean;
  block: Block;
  qty?: number | null;
  grown?: number | null;
  ingredient?: string | null;
  substitutes?: Offer[];
  substitution?: string | null;
  replaces?: { ingredient: string; label: string; qty?: number | null; price_cents?: number | null } | null;
  price_cents?: number | null;
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
      case 'substitute':
        items = items.map((i) => {
          if (i.key !== op.key) return i;
          // La ligne d'origine : celle d'avant un remplacement déjà choisi.
          const base = i.replaces ?? { ingredient: i.ingredient ?? '', label: i.label, qty: i.qty ?? null, ...(i.price_cents !== undefined ? { price_cents: i.price_cents } : {}) };
          if (!op.substitution_id) return { ...i, label: base.label, qty: base.qty, ingredient: base.ingredient, price_cents: base.price_cents ?? null, substitution: null, replaces: null };
          const offer = i.substitutes?.find((o) => o.id === op.substitution_id);
          if (!offer) return i;
          return { ...i, label: offer.label, qty: offer.qty ?? null, ingredient: offer.ingredient, price_cents: offer.price_cents ?? null, substitution: offer.id, replaces: base };
        });
        break;
    }
  }
  return { ...list, items };
}
