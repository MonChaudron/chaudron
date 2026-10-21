import assert from 'node:assert/strict';
import { test } from 'node:test';

import { joinNames, notices, servingsLabel, splitTitle, vignetteOf, withoutMeal } from './look.ts';

test('nom d’un plat : le plat, puis ce qui l’accompagne', () => {
  assert.deepEqual(splitTitle('Sauté de porc au cidre, pommes et carottes, riz'), { main: 'Sauté de porc au cidre', rest: 'pommes et carottes, riz' });
  assert.deepEqual(splitTitle('Blanquette de dinde aux champignons et carottes, riz complet'), {
    main: 'Blanquette de dinde',
    rest: 'aux champignons et carottes, riz complet',
  });
  assert.deepEqual(splitTitle('Omelette épaisse aux champignons et pommes de terre, salade'), {
    main: 'Omelette épaisse',
    rest: 'aux champignons et pommes de terre, salade',
  });
  assert.deepEqual(splitTitle('Chili con carne'), { main: 'Chili con carne', rest: null });
  // Pas de coupe qui ne laisse qu'un mot.
  assert.deepEqual(splitTitle('Wok aux légumes d’automne et nouilles complètes'), { main: 'Wok aux légumes d’automne et nouilles complètes', rest: null });
});

test('portions en clair', () => {
  assert.equal(servingsLabel({ diners: 4, next_lunch_covers: 2, covers_next_lunch: true }), '6 portions, dont 2 en boîtes');
  assert.equal(servingsLabel({ diners: 4, next_lunch_covers: 1, covers_next_lunch: true }), '5 portions, dont 1 en boîte');
  assert.equal(servingsLabel({ diners: 4, next_lunch_covers: 2, covers_next_lunch: false }), '4 portions');
  assert.equal(servingsLabel({ diners: 1, next_lunch_covers: 0, covers_next_lunch: false }), '1 portion');
});

test('vignette : pictogramme du premier appareil, teinte stable', () => {
  assert.equal(vignetteOf('colin', ['hob', 'oven']).pictogram, 'pan');
  assert.equal(vignetteOf('assiette', []).pictogram, 'bowl');
  assert.equal(vignetteOf('chili', ['cookeo_6l']).pictogram, 'pot');
  // La teinte ne dépend que de la recette : la même avec une autre façon de la cuisiner.
  assert.equal(vignetteOf('chili', ['cookeo_6l']).hue, vignetteOf('chili', ['oven']).hue);
  assert.notEqual(vignetteOf('chili', []).hue, vignetteOf('dahl', []).hue);
});

test('prénoms', () => {
  assert.equal(joinNames(['Elsa']), 'Elsa');
  assert.equal(joinNames(['Elsa', 'Bastien', 'Vincent']), 'Elsa, Bastien et Vincent');
});

test('avertissements regroupés, sans la date du repas', () => {
  const dislike = (member: string) => ({
    kind: 'exclusion',
    exclusion: 'dislike' as const,
    member,
    ingredient_name: 'Champignon',
    message: `mercredi 21 octobre soir : ${member} (n'aime pas) : champignon dans Omelette.`,
  });
  assert.deepEqual(
    notices([
      dislike('Bastien'),
      dislike('Vincent'),
      { kind: 'exclusion', exclusion: 'allergy', member: 'Elsa', ingredient_name: 'Arachide', message: '…' },
      { kind: 'freshness', ingredient_name: 'Saumon fumé', message: '…' },
      { kind: 'freshness', ingredient_name: 'Pain', message: '…' },
      { kind: 'missing_equipment', message: 'jeudi 1er octobre midi : il manque le four.' },
    ]),
    [
      { tone: 'alert', icon: 'allergy', text: 'Elsa est allergique : arachide' },
      { tone: 'warn', icon: 'dislike', text: 'Bastien et Vincent n’aiment pas : champignon' },
      { tone: 'warn', icon: 'fresh', text: 'Ne tiendra pas jusque-là : saumon fumé, pain' },
      { tone: 'warn', icon: 'info', text: 'Il manque le four.' },
    ],
  );
  assert.equal(withoutMeal('Rien à retirer.'), 'Rien à retirer.');
});
