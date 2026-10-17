import assert from 'node:assert/strict';
import { test } from 'node:test';

import { addDays, copySlots, dayLabel, nextMenuStart, scaleWith, servingsText, slotEaters, weekDays } from './menus.ts';

test('dates : jours suivants, au-delà du mois et de l’année', () => {
  assert.equal(addDays('2026-10-18', 1), '2026-10-19');
  assert.equal(addDays('2026-10-31', 1), '2026-11-01');
  assert.equal(addDays('2026-12-30', 3), '2027-01-02');
  assert.equal(addDays('2026-03-29', 1), '2026-03-30'); // passage à l'heure d'été
  assert.equal(addDays('2026-10-19', -1), '2026-10-18');
  assert.deepEqual(weekDays('2026-10-18', 3), ['2026-10-18', '2026-10-19', '2026-10-20']);
});

test('nom du jour, relatif à aujourd’hui', () => {
  assert.equal(dayLabel('2026-10-18', '2026-10-18'), 'Aujourd’hui');
  assert.equal(dayLabel('2026-10-19', '2026-10-18'), 'Demain');
  assert.equal(dayLabel('2026-10-17', '2026-10-18'), 'Hier');
  assert.equal(dayLabel('2026-10-21', '2026-10-18'), 'Mercredi 21 octobre');
  assert.equal(dayLabel('2026-11-01', '2026-10-18'), 'Dimanche 1er novembre');
});

test('portions d’un créneau : « 4 + 2 = 6 »', () => {
  assert.equal(servingsText({ diners: 4, next_lunch_covers: 2, covers_next_lunch: true }), '4 + 2 = 6');
  assert.equal(servingsText({ diners: 4, next_lunch_covers: 0, covers_next_lunch: true }), '4');
  assert.equal(servingsText({ diners: 3, next_lunch_covers: 0, covers_next_lunch: false }), '3');
});

const adult = { protein: 1, starch: 1, veg: 1, fat: 1, other: 1 };
const teen = { protein: 1, starch: 2, veg: 1, fat: 1, other: 1 };

test('quantités d’un créneau, d’après son multiplicateur par rôle', () => {
  const scale = { protein: 6, starch: 8, veg: 6, fat: 6, other: 6 };
  assert.equal(scaleWith({ qty: 50, unit: 'g', role: 'starch', per_batch: false }, scale).total, 400);
  // Une ligne par plat ne suit pas les portions ; « au goût » reste sans quantité.
  assert.equal(scaleWith({ qty: 1, unit: 'piece', role: 'other', per_batch: true }, scale).total, 1);
  assert.equal(scaleWith({ qty: null, unit: 'g', role: 'other', per_batch: false }, scale).total, null);
});

test('mangeurs d’un créneau : au repas et en boîte ; aucun si un nombre est fixé', () => {
  const slot = {
    diners_fixed: false,
    next_lunch_covers_fixed: false,
    eaters: [
      { member_id: 'a', name: 'Alice', at: 'meal', portions: adult },
      { member_id: 'l', name: 'Léo', at: 'meal', portions: teen },
      { member_id: 'a', name: 'Alice', at: 'box', portions: adult },
    ],
  };
  assert.deepEqual(
    slotEaters(slot).map((e) => e.name),
    ['Alice', 'Léo', 'Alice (boîte)'],
  );
  assert.deepEqual(slotEaters({ ...slot, diners_fixed: true }), []);
});

test('refaire une semaine : mêmes plats, décalés, portions de nouveau calculées', () => {
  const slots = [
    {
      date: '2026-10-11',
      meal: 'dinner' as const,
      recipe: { slug: 'hachis' },
      variant: { label: 'Four' },
      covers_next_lunch: true,
      diners: 4,
      diners_fixed: false,
      next_lunch_covers: 3,
      next_lunch_covers_fixed: true,
      portions_date: '2026-10-10',
      note: 'riz à part',
    },
    {
      date: '2026-10-13',
      meal: 'dinner' as const,
      recipe: { slug: 'potee' },
      variant: { label: 'Cookeo' },
      covers_next_lunch: false,
      diners: 4,
      diners_fixed: false,
      next_lunch_covers: 0,
      next_lunch_covers_fixed: false,
      portions_date: null,
      note: null,
    },
  ];
  assert.deepEqual(copySlots(slots, '2026-10-11', '2026-10-25'), [
    { date: '2026-10-25', meal: 'dinner', recipe: 'hachis', variant: 'Four', covers_next_lunch: true, note: 'riz à part' },
    { date: '2026-10-27', meal: 'dinner', recipe: 'potee', variant: 'Cookeo', covers_next_lunch: false, note: null },
  ]);
});

test('début du prochain menu : le lendemain du dernier repas prévu, pas avant aujourd’hui', () => {
  assert.equal(nextMenuStart([], '2026-10-17'), '2026-10-17');
  assert.equal(nextMenuStart([{ last_date: '2026-10-23' }, { last_date: '2026-10-16' }], '2026-10-17'), '2026-10-24');
  assert.equal(nextMenuStart([{ last_date: '2026-10-01' }, { last_date: null }], '2026-10-17'), '2026-10-17');
});
