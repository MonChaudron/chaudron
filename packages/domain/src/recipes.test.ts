import assert from 'node:assert/strict';
import { test } from 'node:test';

import { formatDuration, formatQuantity, scaleLine, seasonText, variantDelta } from './recipes.ts';

const adult = { protein: 1, starch: 1, veg: 1, fat: 1, other: 1 };
const teen = { protein: 1, starch: 2, veg: 1, fat: 1, other: 1 };
const child = { protein: 0.6, starch: 1, veg: 0.6, fat: 0.6, other: 0.6 };
const family = [
  { name: 'Alice', portions: adult },
  { name: 'Bruno', portions: adult },
  { name: 'Léo', portions: teen },
  { name: 'Zoé', portions: child },
];

test('quantités pour le foyer, selon le barème de chacun', () => {
  // 50 g de riz par portion standard : 50 + 50 + 100 + 50.
  const rice = scaleLine({ qty: 50, unit: 'g', role: 'starch', per_batch: false }, family);
  assert.equal(rice.total, 250);
  assert.deepEqual(
    rice.perEater.map((e) => [e.name, e.qty]),
    [
      ['Alice', 50],
      ['Bruno', 50],
      ['Léo', 100],
      ['Zoé', 50],
    ],
  );
  // 150 g de bœuf : l'enfant à 0,6.
  assert.equal(scaleLine({ qty: 150, unit: 'g', role: 'protein', per_batch: false }, family).total, 540);
});

test('ce qui ne suit pas les portions, et le « au goût »', () => {
  const lemon = scaleLine({ qty: 1, unit: 'piece', role: 'other', per_batch: true }, family);
  assert.equal(lemon.total, 1);
  assert.deepEqual(lemon.perEater, []);
  const salt = scaleLine({ qty: null, unit: 'pinch', role: 'other', per_batch: false }, family);
  assert.equal(salt.total, null);
  assert.deepEqual(salt.perEater, []);
  // Personne dans le foyer : la portion standard.
  assert.equal(scaleLine({ qty: 150, unit: 'g', role: 'protein', per_batch: false }, []).total, 150);
});

test('quantités lisibles', () => {
  assert.equal(formatQuantity(null, 'g'), 'au goût');
  assert.equal(formatQuantity(7.4, 'g'), '7 g');
  assert.equal(formatQuantity(112, 'g'), '110 g');
  assert.equal(formatQuantity(543, 'g'), '540 g');
  assert.equal(formatQuantity(1250, 'g'), '1,25 kg');
  assert.equal(formatQuantity(2000, 'g'), '2 kg');
  assert.equal(formatQuantity(20, 'ml'), '20 ml');
  assert.equal(formatQuantity(1500, 'ml'), '1,5 l');
  assert.equal(formatQuantity(0.5, 'piece'), '½');
  assert.equal(formatQuantity(2.3, 'piece'), '2 ¼');
  assert.equal(formatQuantity(2.75, 'piece'), '2 ¾');
  assert.equal(formatQuantity(3, 'piece'), '3');
  assert.equal(formatQuantity(0.1, 'piece'), '¼');
  assert.equal(formatQuantity(1, 'tbsp'), '1 c. à s.');
  assert.equal(formatQuantity(1.5, 'tsp'), '1 ½ c. à c.');
  assert.equal(formatQuantity(1, 'pinch'), '1 pincée');
  assert.equal(formatQuantity(2, 'pinch'), '2 pincées');
});

test('durées', () => {
  assert.equal(formatDuration(35), '35 min');
  assert.equal(formatDuration(60), '1 h');
  assert.equal(formatDuration(65), '1 h 05');
  assert.equal(formatDuration(150), '2 h 30');
});

test('écart entre deux variantes', () => {
  const cookeo = { active_min: 15, total_min: 35, vessels: 1 };
  assert.equal(variantDelta(cookeo, { active_min: 20, total_min: 60, vessels: 2 }), '+25 min, +1 récipient');
  assert.equal(variantDelta(cookeo, { active_min: 10, total_min: 25, vessels: 1 }), '−10 min');
  assert.equal(variantDelta(cookeo, { active_min: 15, total_min: 35, vessels: 3 }), '+2 récipients');
  assert.equal(variantDelta(cookeo, { active_min: 15, total_min: 35, vessels: 0 }), '−1 récipient');
  assert.equal(variantDelta(cookeo, cookeo), 'même temps');
});

test('saison', () => {
  assert.equal(seasonText([10, 11, 12, 1, 2]), 'd’octobre à février');
  assert.equal(seasonText([3, 4, 5]), 'de mars à mai');
  assert.equal(seasonText([8]), 'en août');
  assert.equal(seasonText([1, 6, 7]), 'en janvier, juin, juillet');
  assert.equal(seasonText([]), '');
  assert.equal(seasonText([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]), '');
});
