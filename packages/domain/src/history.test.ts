import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type RecipeData, recipeToInput } from './editor.ts';
import { diffRecipes, diffWords, type VersionContent, type VersionLine, type VersionPhase, type VersionStep } from './history.ts';

test('différence mot à mot', () => {
  assert.deepEqual(diffWords('Vapeur 12 min, pommes de terre en bas', 'Vapeur 14 min, pommes de terre en bas'), [
    { kind: 'same', text: 'Vapeur ' },
    { kind: 'removed', text: '12' },
    { kind: 'added', text: '14' },
    { kind: 'same', text: ' min, pommes de terre en bas' },
  ]);
  assert.deepEqual(diffWords('', 'Tout neuf'), [{ kind: 'added', text: 'Tout neuf' }]);
  assert.deepEqual(diffWords('Pareil', 'Pareil'), [{ kind: 'same', text: 'Pareil' }]);
});

const base: VersionContent = {
  title: 'Hachis parmentier',
  description: '',
  tags: ['batch'],
  season_months: [],
  source: null,
  ingredients: [
    { key: 'l1', ingredient: 'minced_beef', state: 'fresh', qty: 150, unit: 'g', role: 'protein', optional: false, per_batch: false, note: null },
    { key: 'l2', ingredient: 'potato', state: 'fresh', qty: 250, unit: 'g', role: 'starch', optional: false, per_batch: false, note: null },
  ],
  variants: [
    {
      key: 'v1', label: 'Cookeo', equipment: ['cookeo_6l'], active_min: 20, total_min: 60, vessels: 2, is_default: true,
      phases: [
        { key: 'p1', title: 'Les légumes', text: 'Vapeur 12 min.', timer_s: 720, day_offset: 0,
          steps: [{ key: 's1', text: 'Cubes de 3 cm.' }] },
        { key: 'p2', title: 'La farce', text: 'Revenir le bœuf.', day_offset: 0, steps: [] },
      ],
    },
  ],
};

function edit(change: (c: VersionContent) => void): VersionContent {
  const copy = structuredClone(base);
  change(copy);
  return copy;
}

test('rien de changé', () => {
  assert.deepEqual(diffRecipes(base, structuredClone(base)), []);
});

test('changements clé à clé, avec leur place', () => {
  const after = edit((c) => {
    c.title = 'Hachis parmentier du dimanche';
    c.ingredients[0].qty = 120;
    (c.ingredients as VersionLine[]).push({ key: 'l3', ingredient: 'onion', state: 'fresh', qty: 0.5, unit: 'piece', role: 'veg', optional: false, per_batch: false, note: null });
    c.variants[0].phases[0].text = 'Vapeur 14 min.';
    c.variants[0].phases[0].timer_s = 840;
    (c.variants[0].phases[0].steps as VersionStep[])[0].text = 'Cubes de 2 cm, pas plus.';
    (c.variants[0].phases as VersionPhase[]).splice(1, 1);
  });
  const changes = diffRecipes(base, after);
  assert.deepEqual(
    changes.map((c) => [c.kind, c.where, c.fields.map((f) => f.field)]),
    [
      ['changed', 'Recette', ['title']],
      ['changed', 'Ingrédient : minced_beef', ['qty']],
      ['added', 'Ingrédient : onion', []],
      ['changed', 'Cookeo › 1. Les légumes', ['text', 'timer_s']],
      ['changed', 'Cookeo › 1. Les légumes › geste 1', ['text']],
      ['removed', 'Cookeo › 2. La farce', []],
    ],
  );
  assert.deepEqual(changes[3].fields[1], { field: 'timer_s', before: 720, after: 840 });
});

test('une étape déplacée garde sa clé : ce n’est pas un ajout', () => {
  const after = edit((c) => {
    (c.variants[0].phases as VersionPhase[]).reverse();
  });
  const changes = diffRecipes(base, after);
  assert.deepEqual(
    changes.map((c) => c.kind),
    ['moved'],
  );
});

test('les clés passent par l’éditeur', () => {
  const input = recipeToInput({ ...(base as unknown as RecipeData), slug: 'hachis' });
  assert.equal(input.ingredients[0].key, 'l1');
  assert.equal(input.variants[0].key, 'v1');
  assert.equal(input.variants[0].phases[0].key, 'p1');
  assert.equal(input.variants[0].phases[0].steps[0].key, 's1');
});
