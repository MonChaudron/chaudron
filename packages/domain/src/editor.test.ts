import assert from 'node:assert/strict';
import { test } from 'node:test';

import { fromFood, recipeToInput, roleForAisle, slugify } from './editor.ts';

test('slug tiré du titre', () => {
  assert.equal(slugify('Chili con carne'), 'chili-con-carne');
  assert.equal(slugify('  Bœuf bourguignon à l’ancienne ! '), 'boeuf-bourguignon-a-l-ancienne');
  assert.equal(slugify('Pâtes -- au thon'), 'pates-au-thon');
  assert.equal(slugify('!!!'), '');
  assert.equal(slugify('a'.repeat(100)).length, 80);
  assert.ok(!slugify(`${'a'.repeat(79)} b`).endsWith('-'));
});

test('une recette lue redevient le corps du PUT', () => {
  const recipe = {
    slug: 'chili',
    title: 'Chili',
    description: '',
    tags: ['batch'],
    season_months: [10],
    source: null,
    updated_at: '2026-10-16T10:00:00Z',
    ingredients: [
      { ingredient: 'minced_beef', ingredient_id: 'x', name: 'Bœuf haché', state: 'fresh', qty: 150, unit: 'g', role: 'protein', optional: false, per_batch: false, note: null },
      { ingredient: 'coriander', ingredient_id: 'y', name: 'Coriandre', state: 'fresh', qty: null, unit: 'g', role: 'other', optional: true, per_batch: false, note: 'au goût' },
    ],
    variants: [
      {
        id: 'v1', label: 'Cookeo', equipment: ['cookeo_6l'], missing_equipment: ['cookeo_6l'], active_min: 15, total_min: 35, vessels: 1, is_default: true,
        phases: [{ title: 'Dorer', text: 'Dorer.', spoken_text: null, timer_s: 300, day_offset: 0,
          steps: [{ text: 'Émincer.', spoken_text: null, timer_s: null, video_url: null, equipment: null, params: {} }] }],
      },
    ],
  } as const;
  assert.deepEqual(recipeToInput(recipe), {
    slug: 'chili',
    title: 'Chili',
    description: '',
    tags: ['batch'],
    season_months: [10],
    source: null,
    ingredients: [
      { ingredient: 'minced_beef', state: 'fresh', qty: 150, unit: 'g', role: 'protein', optional: false, per_batch: false, note: null },
      { ingredient: 'coriander', state: 'fresh', qty: null, unit: 'g', role: 'other', optional: true, per_batch: false, note: 'au goût' },
    ],
    variants: [
      {
        label: 'Cookeo', equipment: ['cookeo_6l'], active_min: 15, total_min: 35, vessels: 1, is_default: true,
        phases: [{ title: 'Dorer', text: 'Dorer.', spoken_text: null, timer_s: 300, day_offset: 0,
          steps: [{ text: 'Émincer.', spoken_text: null, timer_s: null, video_url: null, equipment: null, params: {} }] }],
      },
    ],
  });
});

test('ingrédient proposé depuis une ligne Ciqual', () => {
  assert.deepEqual(fromFood('Petits pois, surgelés, crus'), { name: 'Petits pois', state: 'frozen' });
  assert.deepEqual(fromFood('Carotte, appertisée, égouttée'), { name: 'Carotte', state: 'canned' });
  assert.deepEqual(fromFood('Lentille corail, sèche'), { name: 'Lentille corail', state: 'dried' });
  assert.deepEqual(fromFood('Crevette, cuite'), { name: 'Crevette', state: 'cooked' });
  assert.deepEqual(fromFood('Navet, pelé, cru'), { name: 'Navet', state: 'fresh' });
  assert.deepEqual(fromFood('Boeuf braisé'), { name: 'Boeuf braisé', state: 'fresh' });
});

test('rôle par défaut selon le rayon', () => {
  assert.equal(roleForAisle('butcher'), 'protein');
  assert.equal(roleForAisle('fish'), 'protein');
  assert.equal(roleForAisle('produce'), 'veg');
  assert.equal(roleForAisle('grocery'), 'other');
});
