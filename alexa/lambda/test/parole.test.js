const test = require('node:test');
const assert = require('node:assert/strict');

const p = require('../parole');

test('jours du foyer', () => {
  assert.equal(p.aujourdhui(new Date('2026-10-18T22:30:00Z')), '2026-10-19'); // minuit passé à Paris
  assert.equal(p.decaler('2026-10-31', 1), '2026-11-01');
  assert.equal(p.nomDuJour('2026-10-18'), 'dimanche');
  assert.equal(p.quand('2026-10-18', 'dinner', '2026-10-18'), 'ce soir');
  assert.equal(p.quand('2026-10-18', 'lunch', '2026-10-18'), 'ce midi');
  assert.equal(p.quand('2026-10-19', 'dinner', '2026-10-18'), 'demain soir');
  assert.equal(p.quand('2026-10-17', 'dinner', '2026-10-18'), 'hier soir');
  assert.equal(p.quand('2026-10-22', 'dinner', '2026-10-18'), 'jeudi soir');
});

test('quantités écrites et dites', () => {
  const l = (qty, unit) => ({ qty, unit });
  assert.equal(p.quantiteEcrite(l(900, 'g')), '900 g');
  assert.equal(p.quantiteEcrite(l(1080, 'g')), '1,1 kg');
  assert.equal(p.quantiteEcrite(l(1.98, 'piece')), '2');
  assert.equal(p.quantiteEcrite(l(1.4, 'tbsp')), '1,5 c. à s.');
  assert.equal(p.quantiteEcrite(l(null, 'g')), 'au goût');
  assert.equal(p.quantiteEcrite(l(137, 'g')), '135 g');
  assert.equal(p.quantiteDite(l(900, 'g')), '900 grammes');
  assert.equal(p.quantiteDite(l(1080, 'g')), '1 kilo 100');
  assert.equal(p.quantiteDite(l(2000, 'g')), '2 kilos');
  assert.equal(p.quantiteDite(l(1.5, 'piece')), '1 et demi');
  assert.equal(p.quantiteDite(l(0.5, 'piece')), 'un demi');
  assert.equal(p.quantiteDite(l(1, 'tbsp')), '1 cuillère à soupe');
  assert.equal(p.quantiteDite(l(3, 'tsp')), '3 cuillères à café');
  assert.equal(p.duree(720), '12 minutes');
  assert.equal(p.duree(90), '1 minute 30');
  assert.equal(p.duree(45), '45 secondes');
});

test('noms de plats', () => {
  const curry = p.synonymes('Curry de poulet au lait de coco');
  for (const forme of ['curry', 'le curry', 'curry de poulet', 'le curry de poulet']) {
    assert.ok(curry.includes(forme), `${forme} absent de ${curry}`);
  }
  const hachis = p.synonymes('Hachis parmentier de bœuf, purée pommes de terre et chou-fleur');
  assert.ok(hachis.includes('le hachis parmentier'), hachis.join(' | '));
  const omelette = p.synonymes('Omelette aux herbes');
  assert.ok(omelette.includes("l'omelette"), omelette.join(' | '));
});

const place = (mode, phase, step, phase_count, step_count) => ({ mode, phase, step, phase_count, step_count });

test('avancer et reculer', () => {
  assert.deepEqual(p.suivante(place('fast', 0, null, 3, 0)), { mode: 'fast', phase: 1, step: null });
  assert.equal(p.suivante(place('fast', 2, null, 3, 0)), null);
  assert.deepEqual(p.suivante(place('guided', 1, 0, 3, 3)), { mode: 'guided', phase: 1, step: 1 });
  assert.deepEqual(p.suivante(place('guided', 1, 2, 3, 3)), { mode: 'guided', phase: 2, step: 0 });
  assert.deepEqual(p.suivante(place('guided', 0, null, 3, 0)), { mode: 'guided', phase: 1, step: 0 });
  assert.equal(p.suivante(place('guided', 2, 1, 3, 2)), null);
  assert.equal(p.precedente(place('fast', 0, null, 3, 0)), null);
  assert.deepEqual(p.precedente(place('guided', 1, 1, 3, 3)), { mode: 'guided', phase: 1, step: 0 });
  assert.deepEqual(p.precedente(place('guided', 2, 0, 3, 3)), { mode: 'guided', phase: 1, step: 999 });
  assert.deepEqual(p.etapeNumero(place('fast', 0, null, 4, 0), 3), { mode: 'fast', phase: 2, step: null });
  assert.deepEqual(p.etapeNumero(place('guided', 1, 0, 4, 5), 3), { mode: 'guided', phase: 1, step: 2 });
  assert.deepEqual(p.detaille(place('fast', 1, null, 4, 3)), { mode: 'guided', phase: 1, step: 0 });
  assert.deepEqual(p.accelere(place('guided', 1, 2, 4, 3)), { mode: 'fast', phase: 2, step: null });
  assert.deepEqual(p.accelere(place('guided', 3, 0, 4, 3)), { mode: 'fast', phase: 3, step: null });
});

test('une étape dite', () => {
  const c = {
    ...place('fast', 1, null, 3, 0),
    current: { title: 'La viande', text: 'Le bœuf.', spoken_text: 'On saisit le bœuf.', timer_s: 600 },
  };
  assert.equal(
    p.direEtape(c),
    'Étape 2 sur 3, La viande. On saisit le bœuf. Minuteur : 10 minutes. Dis : Alexa, lance le minuteur.',
  );
  const fin = { ...place('guided', 2, 1, 3, 2), current: { title: 'Servir', text: 'Coriandre.', timer_s: null } };
  assert.equal(p.direEtape(fin), 'Servir, geste 2 sur 2. Coriandre. C’était la dernière étape. Bon appétit !');
});

test('ingrédients et service', () => {
  assert.equal(
    p.direIngredients([
      { name: 'Bœuf haché', qty: 900, unit: 'g', optional: false },
      { name: 'Coriandre', qty: null, unit: 'g', optional: true },
      { name: 'Sel', qty: null, unit: 'pinch', optional: false },
    ]),
    'Bœuf haché, 900 grammes ; Sel, au goût',
  );
  const portions = (starch) => ({ protein: 1, starch, veg: 1, fat: 1, other: 1 });
  const creneau = {
    servings: 6,
    eaters: [
      { name: 'Alice', at: 'meal', portions: portions(1) },
      { name: 'Léo', at: 'meal', portions: portions(2) },
      { name: 'Zoé', at: 'meal', portions: portions(2) },
      { name: 'Alice', at: 'box', portions: portions(1) },
    ],
  };
  assert.equal(
    p.direService(creneau),
    '6 portions. À table : Alice, Léo et Zoé. En boîte pour demain midi : Alice. Double féculent pour Léo et Zoé.',
  );
  assert.equal(p.ssml('Sel & poivre <b>'), 'Sel  et  poivre b');
});
