import assert from 'node:assert/strict';
import { test } from 'node:test';

import { formatFactor, portionsSummary, presenceSummary, searchKey } from './summary.ts';

const all = Array(7).fill(true);
const none = Array(7).fill(false);

test('facteurs à la française', () => {
  assert.equal(formatFactor(2), '×2');
  assert.equal(formatFactor(0.6), '×0,6');
  assert.equal(formatFactor(0.6000000238), '×0,6');
});

test('barème : seulement ce qui change', () => {
  assert.equal(portionsSummary({ protein: 1, starch: 1, veg: 1, fat: 1, other: 1 }), '');
  assert.equal(portionsSummary({ protein: 1, starch: 2, veg: 1, fat: 1, other: 1 }), 'féculents ×2');
  assert.equal(
    portionsSummary({ protein: 0.6, starch: 1, veg: 0.6, fat: 0.6, other: 0.6 }),
    'portions ×0,6 sauf féculents',
  );
  assert.equal(
    portionsSummary({ protein: 1.5, starch: 2, veg: 1, fat: 1, other: 1 }),
    'protéines ×1,5, féculents ×2',
  );
});

test('présence', () => {
  assert.equal(presenceSummary({ lunch: all, dinner: all }), 'Tous les repas');
  assert.equal(presenceSummary({ lunch: none, dinner: none }), 'Jamais là');
  const teen = [false, false, true, false, false, true, false];
  assert.equal(presenceSummary({ lunch: teen, dinner: all }), 'Midi : mer., sam.');
  assert.equal(presenceSummary({ lunch: all, dinner: none }), 'Jamais le soir');
});

test('recherche sans accents', () => {
  assert.equal(searchKey('Œuf'), 'oeuf');
  assert.equal(searchKey(' Épinard '), 'epinard');
  assert.equal(searchKey('Pois chiches'), 'pois chiches');
});
