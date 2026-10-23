// Skill Alexa de Chaudron : le menu et les recettes du foyer, lus étape par étape sur l'Echo Show.
// La place dans la recette vit sur le serveur (`/cooking`) : on la retrouve en revenant, et le téléphone la partage.
const Alexa = require('ask-sdk-core');

const chaudron = require('./chaudron');
const p = require('./parole');
const e = require('./ecran');
const APL = require('./apl.json');

/** Adresse de l'API : `config.json`, écrit au déploiement (rien du foyer dans le code). */
function adresseApi() {
  if (process.env.CHAUDRON_API_URL) return process.env.CHAUDRON_API_URL;
  try {
    return require('./config.json').api_url;
  } catch {
    return null;
  }
}

const MINUTEURS = 'alexa::alerts:timers:skill:readwrite';
const JOURS_AGENDA = 8; // d'hier à dans six jours

// ---------- contexte d'une requête ----------

const attributs = (hi) => hi.attributesManager.getSessionAttributes();
const intention = (hi, ...noms) =>
  Alexa.getRequestType(hi.requestEnvelope) === 'IntentRequest' && noms.includes(Alexa.getIntentName(hi.requestEnvelope));

/** L'API du foyer lié ; `NonLie` sans liaison de compte. */
async function foyer(hi, outils) {
  const jeton = Alexa.getAccountLinkingAccessToken(hi.requestEnvelope);
  if (!jeton) throw new chaudron.NonLie('pas de jeton');
  const api = outils.client(adresseApi(), jeton);
  const a = attributs(hi);
  if (!a.foyer) a.foyer = await api.foyer();
  return { api, id: a.foyer, jour: p.aujourdhui(outils.maintenant()) };
}

async function agenda(f) {
  return f.api.agenda(f.id, p.decaler(f.jour, -1), JOURS_AGENDA);
}

const recetteDe = (ag, slug) => ag.recipes.find((r) => r.slug === slug);
const creneauDe = (ag, choix) =>
  choix && ag.slots.find((s) => s.date === choix.date && s.meal === choix.meal && s.recipe.slug === choix.slug);
const aVenir = (ag, jour) => ag.slots.filter((s) => s.date >= jour);

/** Le plat de ce soir, sinon de ce midi. */
const creneauDuJour = (ag, jour) =>
  ag.slots.find((s) => s.date === jour && s.meal === 'dinner') || ag.slots.find((s) => s.date === jour);

/** Quantités pour le créneau, avant de commencer (pendant la cuisson, le serveur les donne). */
function lignesPour(recette, echelle) {
  return recette.ingredients.map((l) => ({
    ...l,
    qty: l.qty === null || l.per_batch ? l.qty : l.qty * echelle[l.role],
  }));
}

// ---------- réponse ----------

const aplDispo = (hi) => !!Alexa.getSupportedInterfaces(hi.requestEnvelope)['Alexa.Presentation.APL'];

/**
 * Sur écran, sans `reprompt` : la session reste ouverte, micro fermé, tant que l'APL est affiché. On relance avec
 * « Alexa, suivant ». `question` ouvre le micro.
 */
function repondre(hi, texte, vue, { question = false, directives = [] } = {}) {
  const rb = hi.responseBuilder.speak(p.ssml(texte));
  if (aplDispo(hi) && vue) {
    rb.addDirective({
      type: 'Alexa.Presentation.APL.RenderDocument',
      token: 'chaudron',
      document: APL,
      datasources: { donnees: { d: vue } },
    });
  }
  for (const d of directives) rb.addDirective(d);
  if (question || !aplDispo(hi)) rb.reprompt(p.ssml('Alors ? Dis par exemple : le menu, ou : on commence.'));
  return rb.getResponse();
}

/** Les plats de l'agenda, reconnus par leurs noms courts (« le curry »), le temps de la session. */
function entitesPlats(ag) {
  return {
    type: 'Dialog.UpdateDynamicEntities',
    updateBehavior: 'REPLACE',
    types: [
      {
        name: 'PLAT',
        values: ag.recipes.map((r) => ({ id: r.slug, name: { value: r.title.toLowerCase(), synonyms: p.synonymes(r.title) } })),
      },
    ],
  };
}

// ---------- actions (voix et toucher) ----------

function ouvrirMenu(hi, f, ag, prefixe = '', directives = []) {
  const plats = aVenir(ag, f.jour);
  if (!plats.length) {
    return repondre(hi, `${prefixe}Rien au menu pour les jours qui viennent.`, e.vueMenu([], f.jour), { directives });
  }
  const liste = plats.map((s) => `${p.quand(s.date, s.meal, f.jour)}, ${s.recipe.title}`).join(' ; ');
  return repondre(hi, `${prefixe}Au menu : ${liste}. Lequel ?`, e.vueMenu(plats, f.jour), { question: true, directives });
}

function ouvrirRecette(hi, f, ag, creneau, prefixe = '', directives = []) {
  const recette = recetteDe(ag, creneau.recipe.slug);
  const a = attributs(hi);
  a.choix = { date: creneau.date, meal: creneau.meal, slug: creneau.recipe.slug };
  const label = a.variante && a.variante.slug === creneau.recipe.slug ? a.variante.label : creneau.variant.label;
  const variante = recette.variants.find((v) => v.label === label) || recette.variants[0];
  const vue = e.vueRecette({
    id: e.idCreneau(creneau),
    titre: recette.title,
    quand: p.quand(creneau.date, creneau.meal, f.jour),
    variante: variante.label,
    portions: creneau.servings,
    actif: variante.active_min,
    etapes: variante.phases.length,
    lignes: lignesPour(recette, creneau.scale),
  });
  return repondre(
    hi,
    `${prefixe}${recette.title}. ${creneau.servings} portions, ${variante.active_min} minutes de travail, ${variante.phases.length} étapes. Dis : Alexa, on commence. Ou : Alexa, les ingrédients.`,
    vue,
    { directives },
  );
}

/** Va à une place de la recette en cours (le serveur la ramène dans les bornes) et la lit. */
async function aller(hi, f, c, place, prefixe = '') {
  const suite = await f.api.cuisiner(f.id, {
    recipe: c.recipe.slug,
    variant: c.variant,
    date: c.date,
    meal: c.meal,
    ...place,
  });
  return repondre(hi, p.direEtape(suite, prefixe), e.vueEtape(suite));
}

async function commencer(hi, f) {
  const choix = attributs(hi).choix;
  if (!choix) return ouvrirMenu(hi, f, await agenda(f), 'Choisis d’abord un plat. ');
  const variante = attributs(hi).variante;
  const c = await f.api.cuisiner(f.id, {
    recipe: choix.slug,
    variant: variante && variante.slug === choix.slug ? variante.label : undefined,
    date: choix.date,
    meal: choix.meal,
    mode: 'fast',
    phase: 0,
  });
  return repondre(hi, p.direEtape(c), e.vueEtape(c));
}

/** La cuisson en cours, ou une réponse qui dit quoi faire sans elle. */
async function enCours(hi, f) {
  const c = await f.api.cuisson(f.id);
  if (c) return { c };
  if (attributs(hi).choix) {
    return { sans: repondre(hi, 'On n’a pas encore commencé. Dis : Alexa, on commence.', null) };
  }
  return { sans: await ouvrirMenu(hi, f, await agenda(f), 'Choisis d’abord un plat. ') };
}

// ---------- gestionnaires ----------

function creer(outils = { client: chaudron.client, maintenant: () => new Date() }) {
  const avec = (canHandle, action) => ({
    canHandle,
    async handle(hi) {
      return action(hi, await foyer(hi, outils));
    },
  });

  const Lancement = avec(
    (hi) => Alexa.getRequestType(hi.requestEnvelope) === 'LaunchRequest',
    async (hi, f) => {
      const [c, ag] = await Promise.all([f.api.cuisson(f.id), agenda(f)]);
      const plats = entitesPlats(ag);
      if (c) {
        const texte = p.direEtape(c, `On reprend ${c.recipe.title}. `);
        return repondre(hi, texte, e.vueEtape(c), { directives: [plats] });
      }
      const ceSoir = creneauDuJour(ag, f.jour);
      if (ceSoir) {
        const quand = p.quand(ceSoir.date, ceSoir.meal, f.jour);
        return ouvrirRecette(hi, f, ag, ceSoir, `${quand.charAt(0).toUpperCase()}${quand.slice(1)} : `, [plats]);
      }
      return ouvrirMenu(hi, f, ag, 'Bonjour. ', [plats]);
    },
  );

  const ListeMenu = avec(
    (hi) => intention(hi, 'ListMenuIntent', 'AMAZON.NavigateHomeIntent'),
    async (hi, f) => ouvrirMenu(hi, f, await agenda(f)),
  );

  const ChoisirRecette = avec(
    (hi) => intention(hi, 'ChooseRecipeIntent'),
    async (hi, f) => {
      const ag = await agenda(f);
      const slug = resolu(hi, 'plat');
      const jour = resolu(hi, 'jour');
      let creneau = null;
      if (slug) {
        const plats = ag.slots.filter((s) => s.recipe.slug === slug);
        creneau = plats.find((s) => s.date >= f.jour) || plats[plats.length - 1];
      } else if (jour) {
        const date = dateDuJour(jour, f.jour);
        creneau = ag.slots.find((s) => s.date === date && s.meal === 'dinner') || ag.slots.find((s) => s.date === date);
      }
      if (!creneau) return ouvrirMenu(hi, f, ag, 'Je n’ai pas trouvé ce plat dans le menu. ');
      return ouvrirRecette(hi, f, ag, creneau);
    },
  );

  const Commencer = avec((hi) => intention(hi, 'StartCookingIntent'), commencer);

  const Naviguer = avec(
    (hi) =>
      intention(
        hi,
        'AMAZON.NextIntent',
        'AMAZON.PreviousIntent',
        'GoToStepIntent',
        'GuidedModeIntent',
        'FastModeIntent',
      ),
    async (hi, f) => {
      const { c, sans } = await enCours(hi, f);
      if (!c) return sans;
      switch (Alexa.getIntentName(hi.requestEnvelope)) {
        case 'AMAZON.NextIntent': {
          const place = p.suivante(c);
          if (!place) return repondre(hi, 'C’était la dernière étape. Bon appétit !', e.vueEtape(c));
          return aller(hi, f, c, place);
        }
        case 'AMAZON.PreviousIntent': {
          const place = p.precedente(c);
          if (!place) return repondre(hi, p.direEtape(c, 'On est au début. '), e.vueEtape(c));
          return aller(hi, f, c, place);
        }
        case 'GoToStepIntent': {
          const n = parseInt(Alexa.getSlotValue(hi.requestEnvelope, 'numero'), 10);
          if (!n) return repondre(hi, p.direEtape(c, 'Je n’ai pas compris le numéro. '), e.vueEtape(c));
          return aller(hi, f, c, p.etapeNumero(c, n));
        }
        case 'GuidedModeIntent':
          return aller(hi, f, c, p.detaille(c), 'En détail. ');
        default:
          if (c.mode === 'fast') return repondre(hi, p.direEtape(c, 'On est déjà en mode rapide. '), e.vueEtape(c));
          return aller(hi, f, c, p.accelere(c), 'En mode rapide. ');
      }
    },
  );

  const Repeter = avec(
    (hi) => intention(hi, 'AMAZON.RepeatIntent'),
    async (hi, f) => {
      const c = await f.api.cuisson(f.id);
      if (c) return repondre(hi, p.direEtape(c), e.vueEtape(c));
      const ag = await agenda(f);
      const creneau = creneauDe(ag, attributs(hi).choix);
      return creneau ? ouvrirRecette(hi, f, ag, creneau) : ouvrirMenu(hi, f, ag);
    },
  );

  const Ingredients = avec(
    (hi) => intention(hi, 'IngredientsIntent'),
    async (hi, f) => {
      const c = await f.api.cuisson(f.id);
      if (c) return repondre(hi, `Pour ${c.recipe.title} : ${p.direIngredients(c.ingredients)}.`, e.vueEtape(c));
      const ag = await agenda(f);
      const creneau = creneauDe(ag, attributs(hi).choix);
      if (!creneau) return ouvrirMenu(hi, f, ag, 'Choisis d’abord un plat. ');
      const recette = recetteDe(ag, creneau.recipe.slug);
      return ouvrirRecette(hi, f, ag, creneau, `Pour ${creneau.servings} portions : ${p.direIngredients(lignesPour(recette, creneau.scale))}. `);
    },
  );

  const Service = avec(
    (hi) => intention(hi, 'ServeIntent'),
    async (hi, f) => {
      const [c, ag] = await Promise.all([f.api.cuisson(f.id), agenda(f)]);
      const choix = c && c.date ? { date: c.date, meal: c.meal, slug: c.recipe.slug } : attributs(hi).choix;
      const creneau = creneauDe(ag, choix);
      const vue = c ? e.vueEtape(c) : null;
      if (!creneau) return repondre(hi, 'Ce plat n’est pas au menu : je ne sais pas qui mange.', vue);
      return repondre(hi, p.direService(creneau), vue);
    },
  );

  const ChangerVariante = avec(
    (hi) => intention(hi, 'ChangeVariantIntent'),
    async (hi, f) => {
      const appareil = resolu(hi, 'appareil');
      const c = await f.api.cuisson(f.id);
      const ag = c ? null : await agenda(f);
      const creneau = ag && creneauDe(ag, attributs(hi).choix);
      if (!c && !creneau) return ouvrirMenu(hi, f, ag, 'Choisis d’abord un plat. ');
      const recette = c ? c.recipe : recetteDe(ag, creneau.recipe.slug);
      const actuelle = c ? c.variant : (attributs(hi).variante || {}).label || creneau.variant.label;
      const avant = recette.variants.find((v) => v.label === actuelle) || recette.variants[0];
      const autre = recette.variants.find(
        (v) => v.label !== avant.label && (!appareil || !v.equipment.includes(appareil)),
      );
      if (!autre) {
        const texte = 'Il n’y a pas d’autre façon de faire ce plat.';
        return c ? repondre(hi, texte, e.vueEtape(c)) : ouvrirRecette(hi, f, ag, creneau, `${texte} `);
      }
      const ecart = autre.active_min - avant.active_min;
      const annonce = `On passe à : ${autre.label}, ${autre.active_min} minutes de travail${
        ecart ? `, ${Math.abs(ecart)} de ${ecart > 0 ? 'plus' : 'moins'}` : ''
      }. `;
      if (c) return aller(hi, f, { ...c, variant: autre.label }, { mode: 'fast', phase: 0, step: null }, `${annonce}On reprend au début. `);
      attributs(hi).variante = { slug: recette.slug, label: autre.label };
      return ouvrirRecette(hi, f, ag, creneau, annonce);
    },
  );

  /** « Alexa, remarque : ça déborde » : une note du cahier du foyer, à l'étape en cours. Jamais une proposition. */
  const Remarque = avec(
    (hi) => intention(hi, 'RemarkIntent'),
    async (hi, f) => {
      const texte = (Alexa.getSlotValue(hi.requestEnvelope, 'texte') || '').trim();
      if (!texte) {
        const c = await f.api.cuisson(f.id);
        return repondre(hi, 'Dis par exemple : Alexa, remarque : ça déborde.', c ? e.vueEtape(c) : null);
      }
      const note = await f.api.remarquer(f.id, texte);
      if (!note) return repondre(hi, 'Ouvre d’abord une recette, et je note à l’étape où on en est.', null);
      const c = await f.api.cuisson(f.id);
      return repondre(hi, `C’est noté dans le cahier, ${p.ouOnEst(c)}.`, e.vueEtape(c));
    },
  );

  /** « Alexa, les notes » : lues à la demande, jamais d'office. */
  const LireNotes = avec(
    (hi) => intention(hi, 'ReadNotesIntent'),
    async (hi, f) => {
      const { c, sans } = await enCours(hi, f);
      if (!c) return sans;
      return repondre(hi, p.direNotes(c), e.vueEtape(c));
    },
  );

  /**
   * Une skill ne peut ni piloter le lecteur d'Alexa ni lui relayer une commande : on rend la main (session fermée),
   * la place reste sur le serveur. « Alexa, pause » est traité pareil : rien à mettre en pause dans une recette.
   */
  const Musique = {
    canHandle: (hi) => intention(hi, 'MusicIntent', 'MusicSearchIntent', 'AMAZON.PauseIntent'),
    handle: (hi) =>
      hi.responseBuilder
        .speak('Je te rends la main, redis-le à Alexa. Pour reprendre la recette : Alexa, ouvre mon chaudron.')
        .withShouldEndSession(true)
        .getResponse(),
  };

  const LancerMinuteur = avec(
    (hi) => intention(hi, 'StartTimerIntent'),
    async (hi, f) => {
      const c = await f.api.cuisson(f.id);
      if (!c || !c.current.timer_s) {
        return repondre(hi, 'Pas de minuteur à cette étape.', c ? e.vueEtape(c) : null);
      }
      return minuteur(hi, c);
    },
  );

  // Réponse à la demande de permission des minuteurs (AskFor).
  const PermissionMinuteur = avec(
    (hi) => Alexa.getRequestType(hi.requestEnvelope) === 'Connections.Response',
    async (hi, f) => {
      const accorde = (hi.requestEnvelope.request.payload || {}).status === 'ACCEPTED';
      const c = await f.api.cuisson(f.id);
      if (!accorde || !c || !c.current.timer_s) {
        const texte = accorde ? 'C’est noté.' : 'Sans la permission, je ne peux pas lancer de minuteur. On continue.';
        return repondre(hi, texte, c ? e.vueEtape(c) : null);
      }
      return minuteur(hi, c);
    },
  );

  async function minuteur(hi, c) {
    const permis = (((hi.requestEnvelope.context.System.user || {}).permissions || {}).scopes || {})[MINUTEURS];
    if (!permis || permis.status !== 'GRANTED') {
      return hi.responseBuilder
        .addDirective({
          type: 'Connections.SendRequest',
          name: 'AskFor',
          payload: { '@type': 'AskForPermissionsConsentRequest', '@version': '1', permissionScope: MINUTEURS },
          token: 'minuteur',
        })
        .getResponse();
    }
    try {
      await hi.serviceClientFactory.getTimerManagementServiceClient().createTimer({
        duration: `PT${c.current.timer_s}S`,
        timerLabel: c.current.title,
        creationBehavior: { displayExperience: { visibility: 'VISIBLE' } },
        triggeringBehavior: {
          operation: { type: 'ANNOUNCE', textToAnnounce: [{ locale: 'fr-FR', text: `${c.current.title} : c’est l’heure.` }] },
          notificationConfig: { playAudible: true },
        },
      });
      return repondre(hi, `Minuteur lancé : ${p.duree(c.current.timer_s)}.`, e.vueEtape(c));
    } catch (err) {
      console.log('minuteur KO', err && err.message);
      return repondre(hi, 'Je n’ai pas pu lancer le minuteur. Dis : Alexa, mets un minuteur.', e.vueEtape(c));
    }
  }

  const Toucher = avec(
    (hi) => Alexa.getRequestType(hi.requestEnvelope) === 'Alexa.Presentation.APL.UserEvent',
    async (hi, f) => {
      const [action, arg] = hi.requestEnvelope.request.arguments || [];
      if (action === 'recette') {
        const [date, meal, slug] = String(arg).split('|');
        const ag = await agenda(f);
        const creneau = creneauDe(ag, { date, meal, slug });
        return creneau ? ouvrirRecette(hi, f, ag, creneau) : ouvrirMenu(hi, f, ag);
      }
      if (action === 'commencer') return commencer(hi, f);
      if (action === 'menu') return ouvrirMenu(hi, f, await agenda(f));
      const { c, sans } = await enCours(hi, f);
      if (!c) return sans;
      if (action === 'etape') return aller(hi, f, c, { mode: 'fast', phase: Number(arg) - 1, step: null });
      const place = action === 'precedent' ? p.precedente(c) : p.suivante(c);
      return place ? aller(hi, f, c, place) : repondre(hi, p.direEtape(c), e.vueEtape(c));
    },
  );

  const Aide = {
    canHandle: (hi) => intention(hi, 'AMAZON.HelpIntent'),
    handle: (hi) =>
      repondre(
        hi,
        'Dis : le menu, ou le nom d’un plat, ou : celle de ce soir. Pendant la recette : Alexa, suivant. Alexa, précédent. Alexa, répète. Alexa, détaille. Alexa, les ingrédients. Alexa, remarque, suivi de ce que tu veux noter. Alexa, les notes.',
        null,
        { question: true },
      ),
  };

  const Stop = {
    canHandle: (hi) => intention(hi, 'AMAZON.StopIntent', 'AMAZON.CancelIntent'),
    handle: (hi) =>
      hi.responseBuilder
        .speak('Je garde ta place. Dis : Alexa, ouvre mon chaudron, pour reprendre.')
        .withShouldEndSession(true)
        .getResponse(),
  };

  const Incompris = {
    canHandle: (hi) => intention(hi, 'AMAZON.FallbackIntent'),
    handle: (hi) => repondre(hi, 'Je n’ai pas compris. Dis : suivant, répète, les ingrédients, ou : le menu.', null),
  };

  const FinSession = {
    canHandle: (hi) => Alexa.getRequestType(hi.requestEnvelope) === 'SessionEndedRequest',
    handle: (hi) => hi.responseBuilder.getResponse(),
  };

  const Erreur = {
    canHandle: () => true,
    handle(hi, err) {
      if (err instanceof chaudron.NonLie) {
        return hi.responseBuilder
          .speak('Pour lire tes menus, lie ton compte Chaudron : j’ai mis le lien dans l’appli Alexa.')
          .withLinkAccountCard()
          .withShouldEndSession(true)
          .getResponse();
      }
      console.log('erreur', err && err.stack);
      return hi.responseBuilder
        .speak('Chaudron ne répond pas. Réessaie dans un instant.')
        .withShouldEndSession(true)
        .getResponse();
    },
  };

  return Alexa.SkillBuilders.custom()
    .addRequestHandlers(
      Lancement,
      ListeMenu,
      ChoisirRecette,
      Commencer,
      Naviguer,
      Repeter,
      Ingredients,
      Service,
      ChangerVariante,
      Remarque,
      LireNotes,
      Musique,
      LancerMinuteur,
      PermissionMinuteur,
      Toucher,
      Aide,
      Stop,
      Incompris,
      FinSession,
    )
    .addErrorHandlers(Erreur)
    .withApiClient(new Alexa.DefaultApiClient())
    .withCustomUserAgent('chaudron/alexa');
}

// ---------- créneaux ----------

/** Valeur reconnue d'un emplacement (modèle ou entités dynamiques), sinon `null`. */
function resolu(hi, nom) {
  const slot = Alexa.getSlot(hi.requestEnvelope, nom);
  const autorites = (slot && slot.resolutions && slot.resolutions.resolutionsPerAuthority) || [];
  const trouve = autorites.find((a) => a.status.code === 'ER_SUCCESS_MATCH');
  return trouve ? trouve.values[0].value.id : null;
}

const JOURS = ['dimanche', 'lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi'];

/** `ce_soir`, `demain`, `hier` ou un jour de la semaine (le prochain, aujourd'hui compris) → date. */
function dateDuJour(id, jour) {
  if (id === 'ce_soir') return jour;
  if (id === 'demain') return p.decaler(jour, 1);
  if (id === 'hier') return p.decaler(jour, -1);
  const cible = JOURS.indexOf(id);
  for (let i = 0; i < 7; i++) {
    const date = p.decaler(jour, i);
    if (new Date(`${date}T12:00:00Z`).getUTCDay() === cible) return date;
  }
  return jour;
}

exports.creer = creer;
exports.handler = creer().lambda();
