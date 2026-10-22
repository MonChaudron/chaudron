import assert from 'node:assert/strict';
import { test } from 'node:test';

import { applyPending, type Block, type PendingOp } from './shopping.ts';

interface Item {
  key: string;
  label: string;
  checked: boolean;
  manual: boolean;
  block: Block;
  qty?: number | null;
  grown?: number | null;
}

const list: { week_start: string; items: Item[] } = {
  week_start: '2026-10-18',
  items: [
    { key: 'oignon', label: 'Oignon', checked: false, manual: false, block: 'recipes', grown: 2 },
    { key: 'riz', label: 'Riz', checked: true, manual: false, block: 'recipes' },
  ],
};

const op = (kind: PendingOp['kind'], key: string, extra: Partial<PendingOp> = {}): PendingOp => ({
  op_id: `${kind}-${key}`,
  at: '2026-10-17T10:00:00Z',
  week_start: '2026-10-18',
  kind,
  key,
  ...extra,
});

test('les cases en attente s’appliquent sur la liste lue du serveur', () => {
  const next = applyPending(list, [op('check', 'oignon'), op('uncheck', 'riz')]);
  assert.equal(next.items[0].checked, true);
  // Cocher efface l'avertissement de hausse : on vient de prendre la bonne quantité.
  assert.equal(next.items[0].grown, null);
  assert.equal(next.items[1].checked, false);
  // La liste d'origine ne bouge pas.
  assert.equal(list.items[0].checked, false);
});

test('ajouts, modifications et retraits à la main', () => {
  let next = applyPending(list, [op('add', 'manual:1', { label: 'Yaourts', block: 'breakfast_desserts' })]);
  assert.deepEqual(
    next.items.map((i) => i.label),
    ['Oignon', 'Riz', 'Yaourts'],
  );
  assert.equal(next.items[2].manual, true);
  assert.equal(next.items[2].block, 'breakfast_desserts');
  next = applyPending(next, [op('edit', 'manual:1', { label: 'Yaourts nature', qty: 12 }), op('check', 'manual:1')]);
  assert.equal(next.items[2].label, 'Yaourts nature');
  assert.equal(next.items[2].qty, 12);
  assert.equal(next.items[2].checked, true);
  next = applyPending(next, [op('remove', 'manual:1')]);
  assert.equal(next.items.length, 2);
  // Un ajout déjà reçu du serveur n'est pas doublé.
  const known = { ...list, items: [...list.items, { key: 'manual:1', label: 'Yaourts', checked: false, manual: true, block: 'special' } as Item] };
  assert.equal(applyPending(known, [op('add', 'manual:1', { label: 'Yaourts' })]).items.length, 3);
});

test('seules les opérations de ce menu comptent', () => {
  const next = applyPending(list, [op('check', 'oignon', { week_start: '2026-10-11' })]);
  assert.equal(next.items[0].checked, false);
});

test('un remplacement choisi hors ligne change la ligne, et se défait', () => {
  const fish = {
    week_start: '2026-11-02',
    items: [
      {
        key: 'cod',
        label: 'Cabillaud',
        ingredient: 'cod',
        checked: false,
        manual: false,
        block: 'recipes' as Block,
        qty: 900,
        substitution: null,
        replaces: null,
        substitutes: [{ id: 's1', ingredient: 'lieu_noir', label: 'Lieu noir', qty: 990, note: 'aussi bon' }],
      },
    ],
  };
  const pending = (substitution_id: string | null): PendingOp => ({
    op_id: `substitute-${substitution_id}`,
    at: '2026-11-01T10:00:00Z',
    week_start: '2026-11-02',
    kind: 'substitute',
    key: 'cod',
    substitution_id,
  });
  const swapped = applyPending(fish, [pending('s1')]);
  assert.equal(swapped.items[0].label, 'Lieu noir');
  assert.equal(swapped.items[0].qty, 990);
  assert.equal(swapped.items[0].substitution, 's1');
  assert.deepEqual(swapped.items[0].replaces, { ingredient: 'cod', label: 'Cabillaud', qty: 900 });
  const back = applyPending(swapped, [pending(null)]);
  assert.equal(back.items[0].label, 'Cabillaud');
  assert.equal(back.items[0].qty, 900);
  assert.equal(back.items[0].substitution, null);
  // Un remplacement que la ligne ne propose pas (plus) ne change rien.
  assert.equal(applyPending(fish, [pending('autre')]).items[0].label, 'Cabillaud');
});
