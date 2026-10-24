import assert from 'node:assert/strict';
import { test } from 'node:test';

import { factorText, formatGrams, formatKcal, formatSalt, formatServe, gradeLetter, gradeMissingText, portionLine, serveLine, type NutritionView, weighed } from './nutrition.ts';

// Espace insécable avant l'unité, comme les prix.
const nb = (text: string) => text.replace(/ (kcal|g)\b/g, '\u00a0$1');

const boeuf: NutritionView = {
  grade: 'a',
  portion: 'member',
  complete: true,
  cooked_g: 583,
  missing: [],
  amounts: { energy_kcal: 536, protein_g: 39.3 },
  serve: [
    { role: 'protein', label: 'Paleron de bœuf', grams: 103 },
    { role: 'starch', label: 'Pomme de terre', grams: 139 },
    { role: 'veg', label: 'légumes', grams: 281 },
  ],
};

test('arrondis_des_apports', () => {
  assert.equal(formatKcal(536), nb('~540 kcal'));
  assert.equal(formatKcal(1234), nb('~1 230 kcal'));
  assert.equal(formatGrams(39.3), nb('39 g'));
  assert.equal(formatServe(103), nb('~105 g'));
  assert.equal(formatServe(2), nb('~5 g'));
  assert.equal(formatSalt(2.94), nb('2,9 g'));
});

test('ligne_de_ta_portion', () => {
  assert.equal(portionLine(boeuf), nb('Ta portion : ~540 kcal · 39 g de protéines'));
  assert.equal(portionLine({ ...boeuf, portion: 'standard' }), nb('Une portion : ~540 kcal · 39 g de protéines'));
  assert.equal(portionLine({ ...boeuf, complete: false }), nb('Ta portion : au moins ~540 kcal · 39 g de protéines'));
});

test('poids_a_servir', () => {
  assert.equal(serveLine(boeuf), nb('À servir : paleron de bœuf ~105 g · pomme de terre ~140 g · légumes ~280 g'));
  assert.equal(serveLine({ ...boeuf, serve: [] }), '');
});

test('ce_qui_fait_la_note', () => {
  assert.equal(factorText({ criterion: 'salt', points: 5, ingredient: 'Cube de bouillon', amount: 1.94 }), nb('le sel (cube de bouillon : 1,9 g)'));
  assert.equal(factorText({ criterion: 'energy', points: 1, ingredient: 'Paleron de bœuf', amount: 216 }), nb('l’énergie (paleron de bœuf : ~220 kcal)'));
  assert.equal(factorText({ criterion: 'fvl', points: 1, ingredient: 'Carotte', amount: 200 }), nb('les fruits et légumes (carotte : 200 g)'));
  assert.equal(factorText({ criterion: 'fiber', points: 2, ingredient: null, amount: null }), nb('les fibres'));
});

test('lettre_de_la_note', () => {
  assert.equal(gradeLetter('b'), 'B');
});

test('rien_a_peser_rien_a_montrer', () => {
  const empty: NutritionView = { ...boeuf, grade: null, cooked_g: 0, amounts: { energy_kcal: 0, protein_g: 0 }, serve: [] };
  assert.equal(weighed(empty), false);
  assert.equal(weighed(boeuf), true);
  assert.equal(weighed({ ...empty, complete: false, missing: ['Citron confit'] }), true, 'un manque se dit');
});

test('note_indisponible_dit_pourquoi', () => {
  assert.equal(gradeMissingText({ ...boeuf, grade: null, complete: false, missing: ['Citron confit'] }), 'Note indisponible : il manque des données pour citron confit.');
  assert.equal(gradeMissingText(boeuf), null);
});
