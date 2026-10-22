// La skill de bout en bout, avec une fausse API Chaudron en mémoire (agenda, cuisson en cours).
const test = require('node:test');
const assert = require('node:assert/strict');

const { creer } = require('../index');
const { NonLie } = require('../chaudron');

const FOYER = '01a149d5-e7cb-775e-b5a1-e0521d174465';
const portions = (starch) => ({ protein: 1, starch, veg: 1, fat: 1, other: 1 });

const chili = {
  slug: 'chili',
  title: 'Chili con carne',
  ingredients: [
    { ingredient: 'minced_beef', name: 'Bœuf haché', qty: 150, unit: 'g', role: 'protein', optional: false, per_batch: false },
    { ingredient: 'rice', name: 'Riz', qty: 50, unit: 'g', role: 'starch', optional: false, per_batch: false },
    { ingredient: 'garlic', name: 'Ail', qty: 1, unit: 'piece', role: 'other', optional: false, per_batch: true },
  ],
  variants: [
    {
      label: 'Cocotte',
      equipment: ['hob'],
      active_min: 20,
      phases: [
        { title: 'Les légumes', text: 'Oignon.', spoken_text: 'On émince l’oignon.', timer_s: null, steps: [] },
        {
          title: 'La viande',
          text: 'Le bœuf.',
          timer_s: 600,
          steps: [{ text: 'Feu vif.' }, { text: 'Écraser.', timer_s: 360 }],
        },
        { title: 'Servir', text: 'Coriandre.', timer_s: null, steps: [] },
      ],
    },
    {
      label: 'Cookeo',
      equipment: ['cookeo_6l'],
      active_min: 10,
      phases: [{ title: 'Sous pression', text: 'Tout dans la cuve.', timer_s: 480, steps: [] }],
    },
  ],
};

const curry = {
  slug: 'curry',
  title: 'Curry de poulet au lait de coco',
  ingredients: [{ ingredient: 'chicken', name: 'Poulet', qty: 150, unit: 'g', role: 'protein', optional: false, per_batch: false }],
  variants: [{ label: 'Cookeo', equipment: ['cookeo_6l'], active_min: 15, phases: [{ title: 'Cuire', text: 'Cuire.', steps: [] }] }],
};

const creneau = (date, recette, variant, servings, eaters) => ({
  date,
  meal: 'dinner',
  recipe: { slug: recette.slug, title: recette.title },
  variant: { label: variant, active_min: 20 },
  servings,
  scale: { protein: servings, starch: servings + 2, veg: servings, fat: servings, other: servings },
  eaters,
  express: false,
});

/** Fausse API : l'agenda du 18 octobre, et `/cooking` qui ramène la place dans les bornes, comme le serveur. */
function fausseApi({ lie = true } = {}) {
  // Notes du cahier : `{ body, phase, step }` (step nul : la phase), ou `{ body }` sur la recette entière.
  const etat = { cuisson: null, appels: [], notes: [] };
  const agenda = {
    slots: [
      creneau('2026-10-18', chili, 'Cocotte', 6, [
        { name: 'Alice', at: 'meal', portions: portions(1) },
        { name: 'Léo', at: 'meal', portions: portions(2) },
        { name: 'Alice', at: 'box', portions: portions(1) },
      ]),
      creneau('2026-10-20', curry, 'Cookeo', 4, []),
    ],
    recipes: [chili, curry],
  };
  const vue = (place) => {
    const recette = agenda.recipes.find((r) => r.slug === place.recipe);
    const variante = recette.variants.find((v) => v.label === place.variant) || recette.variants[0];
    const phase = Math.max(0, Math.min(place.phase, variante.phases.length - 1));
    const gestes = variante.phases[phase].steps.length;
    const step = place.mode === 'guided' && gestes ? Math.max(0, Math.min(place.step || 0, gestes - 1)) : null;
    const p = variante.phases[phase];
    const g = step === null ? null : p.steps[step];
    return {
      recipe: recette,
      variant: variante.label,
      date: place.date || null,
      meal: place.meal || null,
      servings: 6,
      mode: place.mode,
      phase,
      step,
      phase_count: variante.phases.length,
      step_count: gestes,
      current: g
        ? { title: p.title, text: g.text, spoken_text: g.spoken_text || null, timer_s: g.timer_s || null }
        : { title: p.title, text: p.text, spoken_text: p.spoken_text || null, timer_s: p.timer_s || null },
      ingredients: recette.ingredients.map((l) => ({ ...l, qty: l.per_batch ? l.qty : l.qty * 6 })),
      step_notes: etat.notes
        .filter((n) => n.phase === phase && (step === null || n.step === step))
        .map((n) => ({ body: n.body })),
      recipe_notes: etat.notes.filter((n) => n.phase === undefined).map((n) => ({ body: n.body })),
    };
  };
  const client = (base, jeton) => {
    assert.equal(jeton, 'jeton-alexa');
    const garde = (f) => (...args) => {
      if (!lie) return Promise.reject(new NonLie('révoqué'));
      etat.appels.push(f.name);
      return Promise.resolve(f(...args));
    };
    return {
      foyer: garde(function foyer() {
        return FOYER;
      }),
      agenda: garde(function agendaDu(foyer, depuis, jours) {
        assert.equal(depuis, '2026-10-17');
        assert.equal(jours, 8);
        return agenda;
      }),
      cuisson: garde(function cuisson() {
        return etat.cuisson && vue(etat.cuisson);
      }),
      cuisiner: garde(function cuisiner(foyer, place) {
        const variant = place.variant || (agenda.slots.find((s) => s.recipe.slug === place.recipe) || {}).variant.label;
        const c = vue({ ...place, variant });
        etat.cuisson = { recipe: place.recipe, variant: c.variant, date: place.date, meal: place.meal, mode: c.mode, phase: c.phase, step: c.step };
        return c;
      }),
      terminer: garde(function terminer() {
        etat.cuisson = null;
      }),
      remarquer: garde(function remarquer(foyer, texte) {
        if (!etat.cuisson) return null;
        const note = { body: texte, phase: etat.cuisson.phase, step: etat.cuisson.step };
        etat.notes.push(note);
        return { body: texte };
      }),
    };
  };
  return { etat, client };
}

/** Une conversation : chaque phrase est une requête, la session suit. */
function conversation(api, { ecran = true, permis = false } = {}) {
  const skill = creer({ client: api.client, maintenant: () => new Date('2026-10-18T17:00:00Z') }).create();
  let attributes = {};
  let nouvelle = true;
  const envoyer = async (request) => {
    const envelope = {
      version: '1.0',
      session: { new: nouvelle, sessionId: 's', application: { applicationId: 'a' }, user: { userId: 'u' }, attributes },
      context: {
        System: {
          application: { applicationId: 'a' },
          user: {
            userId: 'u',
            accessToken: 'jeton-alexa',
            permissions: permis ? { scopes: { 'alexa::alerts:timers:skill:readwrite': { status: 'GRANTED' } } } : {},
          },
          device: { deviceId: 'd', supportedInterfaces: ecran ? { 'Alexa.Presentation.APL': {} } : {} },
          apiEndpoint: 'https://api.eu.amazonalexa.com',
          apiAccessToken: 'x',
        },
      },
      request: { requestId: 'r', timestamp: '2026-10-18T17:00:00Z', locale: 'fr-FR', ...request },
    };
    nouvelle = false;
    const reponse = await skill.invoke(envelope);
    attributes = reponse.sessionAttributes || attributes;
    const r = reponse.response;
    const apl = (r.directives || []).find((d) => d.type === 'Alexa.Presentation.APL.RenderDocument');
    return {
      texte: r.outputSpeech ? r.outputSpeech.ssml.replace(/<\/?speak>/g, '') : '',
      vue: apl && apl.datasources.donnees.d,
      fin: r.shouldEndSession,
      micro: !!r.reprompt,
      directives: r.directives || [],
      carte: r.card,
    };
  };
  const dire = (name, slots = {}) =>
    envoyer({
      type: 'IntentRequest',
      intent: {
        name,
        confirmationStatus: 'NONE',
        slots: Object.fromEntries(
          Object.entries(slots).map(([nom, id]) => [
            nom,
            {
              name: nom,
              value: id,
              resolutions: { resolutionsPerAuthority: [{ status: { code: 'ER_SUCCESS_MATCH' }, values: [{ value: { id, name: id } }] }] },
            },
          ]),
        ),
      },
    });
  return { lancer: () => envoyer({ type: 'LaunchRequest' }), dire, toucher: (...args) => envoyer({ type: 'Alexa.Presentation.APL.UserEvent', arguments: args }) };
}

test('ouvrir : le plat de ce soir, les noms courts des plats', async () => {
  const api = fausseApi();
  const r = await conversation(api).lancer();
  assert.match(r.texte, /^Ce soir : Chili con carne\. 6 portions, 20 minutes de travail, 3 étapes\./);
  assert.equal(r.vue.vue, 'recette');
  assert.deepEqual(r.vue.ingredients[0], { nom: 'Bœuf haché', qte: '900 g' }); // 150 g × 6 (échelle du créneau)
  assert.equal(r.fin, undefined, 'session ouverte, micro fermé');
  assert.equal(r.micro, false);
  const entites = r.directives.find((d) => d.type === 'Dialog.UpdateDynamicEntities');
  const curryEntite = entites.types[0].values.find((v) => v.id === 'curry');
  assert.ok(curryEntite.name.synonyms.includes('le curry'));
});

test('cuisiner : commencer, suivant, détaille, accélère, précédent, la fin', async () => {
  const api = fausseApi();
  const s = conversation(api);
  await s.lancer();
  let r = await s.dire('StartCookingIntent');
  assert.equal(r.texte, 'Étape 1 sur 3, Les légumes. On émince l’oignon.');
  assert.equal(r.vue.position, 'Étape 1 / 3');
  assert.deepEqual(api.etat.cuisson, { recipe: 'chili', variant: 'Cocotte', date: '2026-10-18', meal: 'dinner', mode: 'fast', phase: 0, step: null });

  r = await s.dire('AMAZON.NextIntent');
  assert.match(r.texte, /^Étape 2 sur 3, La viande\. Le bœuf\. Minuteur : 10 minutes\./);
  assert.equal(r.vue.minuteur, '⏱ 10 min');

  r = await s.dire('GuidedModeIntent');
  assert.equal(r.texte, 'En détail. La viande, geste 1 sur 2. Feu vif.');
  r = await s.dire('AMAZON.NextIntent');
  assert.match(r.texte, /^La viande, geste 2 sur 2\. Écraser\. Minuteur : 6 minutes/);
  r = await s.dire('AMAZON.PreviousIntent');
  assert.equal(r.texte, 'La viande, geste 1 sur 2. Feu vif.');
  r = await s.dire('FastModeIntent');
  assert.match(r.texte, /^En mode rapide\. Étape 3 sur 3, Servir\. Coriandre\. C’était la dernière étape/);
  r = await s.dire('AMAZON.NextIntent');
  assert.equal(r.texte, 'C’était la dernière étape. Bon appétit !');
  r = await s.dire('GoToStepIntent', { numero: '1' });
  assert.match(r.texte, /^Étape 1 sur 3/);

  // On ferme, on revient : la place est sur le serveur.
  r = await s.dire('AMAZON.StopIntent');
  assert.equal(r.fin, true);
  r = await conversation(api).lancer();
  assert.equal(r.texte, 'On reprend Chili con carne. Étape 1 sur 3, Les légumes. On émince l’oignon.');
});

test('choisir un plat par son nom ou son jour ; ingrédients et service', async () => {
  const s = conversation(fausseApi());
  let r = await s.dire('ChooseRecipeIntent', { plat: 'curry' });
  assert.match(r.texte, /^Curry de poulet au lait de coco\. 4 portions/);
  r = await s.dire('ChooseRecipeIntent', { jour: 'ce_soir' });
  assert.match(r.texte, /^Chili con carne\./);
  r = await s.dire('IngredientsIntent');
  assert.match(r.texte, /^Pour 6 portions : Bœuf haché, 900 grammes ; Riz, 400 grammes ; Ail, 1\./);
  r = await s.dire('ServeIntent');
  assert.equal(r.texte, '6 portions. À table : Alice et Léo. En boîte pour demain midi : Alice. Double féculent pour Léo.');
  r = await s.dire('ChooseRecipeIntent', { plat: 'inconnu' });
  assert.match(r.texte, /^Je n’ai pas trouvé ce plat dans le menu\. Au menu : ce soir, Chili con carne ; mardi soir, Curry/);
  assert.equal(r.micro, true, 'on attend un choix');
});

test('changer de variante : « je n’ai pas le Cookeo », en cuisinant', async () => {
  const api = fausseApi();
  const s = conversation(api);
  await s.lancer();
  let r = await s.dire('ChangeVariantIntent', { appareil: 'hob' });
  assert.match(r.texte, /^On passe à : Cookeo, 10 minutes de travail, 10 de moins\. Chili con carne\./);
  r = await s.dire('StartCookingIntent');
  assert.match(r.texte, /^Étape 1 sur 1, Sous pression\./);
  r = await s.dire('ChangeVariantIntent', { appareil: 'cookeo_6l' });
  assert.match(r.texte, /^On passe à : Cocotte, 20 minutes de travail, 10 de plus\. On reprend au début\. Étape 1 sur 3/);
  assert.equal(api.etat.cuisson.variant, 'Cocotte');
});

test('la musique et la pause rendent la main', async () => {
  const s = conversation(fausseApi());
  for (const nom of ['MusicIntent', 'MusicSearchIntent', 'AMAZON.PauseIntent']) {
    const r = await s.dire(nom);
    assert.match(r.texte, /^Je te rends la main/);
    assert.equal(r.fin, true);
  }
});

test('minuteur : la permission d’abord', async () => {
  const api = fausseApi();
  const s = conversation(api);
  await s.lancer();
  await s.dire('StartCookingIntent');
  let r = await s.dire('StartTimerIntent');
  assert.equal(r.texte, 'Pas de minuteur à cette étape.');
  await s.dire('AMAZON.NextIntent');
  r = await s.dire('StartTimerIntent');
  const demande = r.directives.find((d) => d.type === 'Connections.SendRequest');
  assert.equal(demande.payload.permissionScope, 'alexa::alerts:timers:skill:readwrite');
});

test('toucher l’écran', async () => {
  const s = conversation(fausseApi());
  await s.lancer();
  let r = await s.toucher('menu');
  assert.equal(r.vue.vue, 'menu');
  assert.equal(r.vue.items[0].jour, 'Ce soir');
  r = await s.toucher('recette', r.vue.items[1].id);
  assert.match(r.texte, /^Curry/);
  r = await s.toucher('commencer');
  assert.equal(r.vue.vue, 'etape');
  r = await s.toucher('suivant');
  assert.match(r.texte, /dernière étape/);
});

test('sans liaison, ou liaison retirée : la carte de liaison', async () => {
  const r = await conversation(fausseApi({ lie: false })).lancer();
  assert.match(r.texte, /lie ton compte Chaudron/);
  assert.equal(r.carte.type, 'LinkAccount');
  assert.equal(r.fin, true);
});

test('sans écran : on garde le micro ouvert', async () => {
  const r = await conversation(fausseApi(), { ecran: false }).lancer();
  assert.equal(r.vue, undefined);
  assert.equal(r.micro, true);
});

test('« Alexa, remarque » : dans le cahier, à l’étape en cours', async () => {
  const api = fausseApi();
  const c = conversation(api);
  await c.lancer();
  await c.dire('StartCookingIntent');
  await c.dire('AMAZON.NextIntent');
  const r = await c.dire('RemarkIntent', { texte: 'ça déborde, moins de bouillon' });
  assert.match(r.texte, /^C’est noté dans le cahier, à l’étape 2\./);
  assert.deepEqual(api.etat.notes, [{ body: 'ça déborde, moins de bouillon', phase: 1, step: null }]);
  assert.equal(r.vue.notes, '1 note');
  assert.equal(r.fin, undefined);

  await c.dire('GuidedModeIntent');
  const geste = await c.dire('RemarkIntent', { texte: 'feu moins vif' });
  assert.match(geste.texte, /au geste 1 de la viande/);
});

test('remarque sans cuisson, ou sans texte', async () => {
  const api = fausseApi();
  const c = conversation(api);
  await c.lancer();
  const sans = await c.dire('RemarkIntent', { texte: 'trop salé' });
  assert.match(sans.texte, /Ouvre d’abord une recette/);
  assert.deepEqual(api.etat.notes, []);
  await c.dire('StartCookingIntent');
  const vide = await c.dire('RemarkIntent');
  assert.match(vide.texte, /Dis par exemple : Alexa, remarque : ça déborde/);
});

test('les notes : un badge à l’écran, lues seulement quand on les demande', async () => {
  const api = fausseApi();
  api.etat.notes.push(
    { body: 'Chez nous : 14 minutes.', phase: 0, step: null },
    { body: 'Émincer plus fin.', phase: 0, step: null },
    { body: 'Les enfants adorent.' },
  );
  const c = conversation(api);
  await c.lancer();
  const etape = await c.dire('StartCookingIntent');
  assert.equal(etape.vue.notes, '2 notes');
  for (const r of [etape, await c.dire('AMAZON.RepeatIntent'), await c.lancer()]) {
    assert.doesNotMatch(r.texte, /14 minutes|plus fin|adorent/, 'jamais d’office');
  }
  const lues = await c.dire('ReadNotesIntent');
  assert.equal(lues.texte, 'Deux notes sur cette étape. Chez nous : 14 minutes. Et : Émincer plus fin. Sur la recette : Les enfants adorent.');

  await c.dire('AMAZON.NextIntent');
  const ailleurs = await c.dire('ReadNotesIntent');
  assert.equal(ailleurs.texte, 'Rien sur cette étape. Sur la recette : Les enfants adorent.');
  assert.equal(ailleurs.vue.notes, '');
});

test('les notes : trois au plus, le reste sur le téléphone', async () => {
  const api = fausseApi();
  for (const body of ['Un.', 'Deux.', 'Trois.', 'Quatre.', 'Cinq.']) api.etat.notes.push({ body, phase: 0, step: null });
  const c = conversation(api);
  await c.lancer();
  await c.dire('StartCookingIntent');
  const r = await c.dire('ReadNotesIntent');
  assert.equal(r.texte, 'Cinq notes sur cette étape. Un. Et : Deux. Et : Trois. Et deux autres, sur le téléphone.');
  const rien = fausseApi();
  const c2 = conversation(rien);
  await c2.lancer();
  await c2.dire('StartCookingIntent');
  assert.equal((await c2.dire('ReadNotesIntent')).texte, 'Aucune note sur cette recette.');
});
