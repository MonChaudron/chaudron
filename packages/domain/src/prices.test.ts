import assert from 'node:assert/strict';
import { test } from 'node:test';

import { aisleSubtotals, formatPrice, formatTotal, pillText, tierLabel } from './prices.ts';

test('format_total_sans_centimes', () => {
  assert.equal(formatTotal(14849), '≈ 148 €');
  assert.equal(formatTotal(14850), '≈ 149 €');
  assert.equal(formatTotal(123456), '≈ 1 235 €');
});

test('prix_au_centime', () => {
  assert.equal(formatPrice(430), '4,30 €');
  assert.equal(formatPrice(5), '0,05 €');
});

test('sous_totaux_par_rayon_et_placard', () => {
  const items = [
    { aisle: 'produce', pantry: false, price_cents: 430 },
    { aisle: 'produce', pantry: false, price_cents: 640 },
    { aisle: 'butcher', pantry: false, price_cents: 1980 },
    { aisle: 'grocery', pantry: true, price_cents: 14 },
    { aisle: 'produce', pantry: true, price_cents: 6 },
  ];
  assert.deepEqual(aisleSubtotals(items), { produce: 1070, butcher: 1980, pantry: 20 });
});

test('article_sans_prix_hors_sous_total', () => {
  const items = [
    { aisle: 'produce', pantry: false, price_cents: 430 },
    { aisle: 'produce', pantry: false, price_cents: null },
    { aisle: 'other', pantry: false },
  ];
  assert.deepEqual(aisleSubtotals(items), { produce: 430 });
});

test('libelles_des_tiers', () => {
  assert.equal(tierLabel('low'), 'bon marché');
  assert.equal(tierLabel('mid'), 'dans la moyenne');
  assert.equal(tierLabel('high'), 'cher');
});

test('prix_inconnu_sans_montant', () => {
  assert.equal(pillText(1900, true), '≈ 19 €');
  assert.equal(pillText(1900, false), '≈ 19 € +');
  assert.equal(pillText(0, false), 'prix inconnu', 'jamais « ≈ 0 € »');
});
