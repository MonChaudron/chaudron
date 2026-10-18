// Ce que la skill dit, sans réseau : jours, quantités, noms de plats, position dans la recette.
// Node 16 (Lambda Alexa-hosted) : pas de fetch, pas de Array.prototype.at.

const FUSEAU = 'Europe/Paris';
const JOURS = ['dimanche', 'lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi'];

// ---------- jours ----------

/** Le jour du foyer, `2026-10-18`. */
function aujourdhui(maintenant = new Date(), fuseau = FUSEAU) {
  return maintenant.toLocaleDateString('en-CA', { timeZone: fuseau });
}

function decaler(iso, jours) {
  const d = new Date(`${iso}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() + jours);
  return d.toISOString().slice(0, 10);
}

function nomDuJour(iso) {
  return JOURS[new Date(`${iso}T12:00:00Z`).getUTCDay()];
}

/** « ce soir », « demain midi », « jeudi soir », « hier soir ». */
function quand(date, repas, jour) {
  const moment = repas === 'lunch' ? 'midi' : 'soir';
  if (date === jour) return repas === 'lunch' ? 'ce midi' : 'ce soir';
  if (date === decaler(jour, 1)) return `demain ${moment}`;
  if (date === decaler(jour, -1)) return `hier ${moment}`;
  return `${nomDuJour(date)} ${moment}`;
}

// ---------- nombres et quantités ----------

/** `1,5` à l'écran. */
function nombreEcrit(n) {
  return String(n).replace('.', ',');
}

/** « 2 », « 1 et demi », « un demi » : la synthèse lit mal « 1,5 ». */
function nombreDit(n) {
  const entier = Math.floor(n);
  const reste = Math.round((n - entier) * 10) / 10;
  if (reste === 0) return String(entier);
  if (reste === 0.5) return entier === 0 ? 'un demi' : `${entier} et demi`;
  return nombreEcrit(n);
}

/** Arrondi lisible : demi-pièce, gramme près sous 20 g, 5 g sous 200 g, 10 g au-delà. */
function arrondir(qty, unit) {
  if (unit === 'piece' || unit === 'tbsp' || unit === 'tsp' || unit === 'pinch') {
    return Math.max(0.5, Math.round(qty * 2) / 2);
  }
  if (qty < 20) return Math.max(1, Math.round(qty));
  if (qty < 200) return Math.round(qty / 5) * 5;
  return Math.round(qty / 10) * 10;
}

const UNITES = {
  g: { court: 'g', un: 'gramme', plusieurs: 'grammes' },
  ml: { court: 'ml', un: 'millilitre', plusieurs: 'millilitres' },
  tbsp: { court: 'c. à s.', un: 'cuillère à soupe', plusieurs: 'cuillères à soupe' },
  tsp: { court: 'c. à c.', un: 'cuillère à café', plusieurs: 'cuillères à café' },
  pinch: { court: 'pincée', un: 'pincée', plusieurs: 'pincées' },
  piece: { court: '', un: '', plusieurs: '' },
};

/** Quantité à l'écran : « 900 g », « 1,1 kg », « 2 », « au goût ». */
function quantiteEcrite(ligne) {
  if (ligne.qty === null || ligne.qty === undefined) return 'au goût';
  const q = arrondir(ligne.qty, ligne.unit);
  if (ligne.unit === 'g' && q >= 1000) return `${nombreEcrit(Math.round(q / 100) / 10)} kg`;
  if (ligne.unit === 'ml' && q >= 1000) return `${nombreEcrit(Math.round(q / 100) / 10)} l`;
  const unite = UNITES[ligne.unit] || UNITES.piece;
  return unite.court ? `${nombreEcrit(q)} ${unite.court}` : nombreEcrit(q);
}

/** Quantité dite : « 900 grammes », « 1 kilo 100 », « 2 », « au goût ». */
function quantiteDite(ligne) {
  if (ligne.qty === null || ligne.qty === undefined) return 'au goût';
  const q = arrondir(ligne.qty, ligne.unit);
  if ((ligne.unit === 'g' || ligne.unit === 'ml') && q >= 1000) {
    const kilos = Math.floor(q / 1000);
    const reste = Math.round((q % 1000) / 100) * 100;
    const mot = ligne.unit === 'g' ? (kilos > 1 ? 'kilos' : 'kilo') : kilos > 1 ? 'litres' : 'litre';
    return reste ? `${kilos} ${mot} ${reste}` : `${kilos} ${mot}`;
  }
  const unite = UNITES[ligne.unit] || UNITES.piece;
  const mot = q > 1 ? unite.plusieurs : unite.un;
  return mot ? `${nombreDit(q)} ${mot}` : nombreDit(q);
}

/** « 12 minutes », « 1 minute 30 », « 45 secondes ». */
function duree(secondes) {
  const minutes = Math.floor(secondes / 60);
  const reste = secondes % 60;
  if (minutes === 0) return `${reste} secondes`;
  const m = `${minutes} minute${minutes > 1 ? 's' : ''}`;
  return reste ? `${m} ${reste}` : m;
}

// ---------- noms de plats ----------

const LIAISONS = new Set(['de', 'du', 'des', 'au', 'aux', 'à', 'a', 'en', 'et', 'avec', 'sur', 'façon', 'sauce', ',']);

/**
 * Ce qu'on dit d'un plat : « Curry de poulet au lait de coco » → « curry », « le curry », « curry de poulet »… Les
 * articles sont ajoutés sans chercher le genre : un synonyme de trop ne gêne pas la reconnaissance.
 */
function synonymes(titre) {
  const mots = titre
    .toLowerCase()
    .replace(/[,()]/g, ' ')
    .split(/\s+/)
    .filter(Boolean);
  const coupe = (depuis) => {
    let i = depuis;
    while (i < mots.length && !LIAISONS.has(mots[i])) i++;
    return i;
  };
  const formes = new Set();
  const fin1 = coupe(0);
  const tete = mots.slice(0, fin1).join(' ');
  if (tete) formes.add(tete);
  if (fin1 < mots.length - 1) {
    const fin2 = coupe(fin1 + 1);
    formes.add(mots.slice(0, fin2).join(' '));
  }
  const avecArticles = new Set();
  for (const forme of formes) {
    avecArticles.add(forme);
    for (const article of ['le', 'la', 'les']) avecArticles.add(`${article} ${forme}`);
    if (/^[aeiouyéèêh]/.test(forme)) avecArticles.add(`l'${forme}`);
  }
  avecArticles.delete(titre.toLowerCase());
  return [...avecArticles];
}

// ---------- position dans la recette ----------

/** Où aller après la place `c` (réponse de `/cooking`) ; `null` : c'était la dernière étape. */
function suivante(c) {
  if (c.mode === 'guided' && c.step !== null && c.step + 1 < c.step_count) {
    return { mode: 'guided', phase: c.phase, step: c.step + 1 };
  }
  if (c.phase + 1 >= c.phase_count) return null;
  return { mode: c.mode, phase: c.phase + 1, step: c.mode === 'guided' ? 0 : null };
}

/** Où aller avant ; `null` : on est au début. Le serveur ramène un geste trop grand au dernier geste. */
function precedente(c) {
  if (c.mode === 'guided' && c.step !== null && c.step > 0) {
    return { mode: 'guided', phase: c.phase, step: c.step - 1 };
  }
  if (c.phase === 0) return null;
  return { mode: c.mode, phase: c.phase - 1, step: c.mode === 'guided' ? 999 : null };
}

/** « Étape 4 » : la phase en mode rapide, le geste de la phase en pas à pas. */
function etapeNumero(c, n) {
  if (c.mode === 'guided' && c.step_count > 0) return { mode: 'guided', phase: c.phase, step: n - 1 };
  return { mode: c.mode, phase: n - 1, step: c.mode === 'guided' ? 0 : null };
}

/** « Détaille » : le premier geste de la phase en cours. */
function detaille(c) {
  return { mode: 'guided', phase: c.phase, step: 0 };
}

/** « Accélère » : depuis le pas à pas, la phase suivante en mode rapide. */
function accelere(c) {
  if (c.mode === 'fast') return { mode: 'fast', phase: c.phase, step: null };
  return { mode: 'fast', phase: Math.min(c.phase + 1, c.phase_count - 1), step: null };
}

function estDerniere(c) {
  return c.phase === c.phase_count - 1 && (c.mode === 'fast' || c.step === null || c.step === c.step_count - 1);
}

/** Ce que la skill lit d'une étape : position, texte parlé, minuteur, fin. */
function direEtape(c, prefixe = '') {
  const position =
    c.mode === 'guided' && c.step !== null
      ? `${c.current.title}, geste ${c.step + 1} sur ${c.step_count}.`
      : `Étape ${c.phase + 1} sur ${c.phase_count}, ${c.current.title}.`;
  const texte = c.current.spoken_text || c.current.text;
  const minuteur = c.current.timer_s
    ? ` Minuteur : ${duree(c.current.timer_s)}. Dis : Alexa, lance le minuteur.`
    : '';
  const fin = estDerniere(c) ? ' C’était la dernière étape. Bon appétit !' : '';
  return `${prefixe}${position} ${texte}${minuteur}${fin}`;
}

/** Les ingrédients, dits. */
function direIngredients(lignes) {
  return lignes
    .filter((l) => !l.optional || l.qty !== null)
    .map((l) => `${l.name}, ${quantiteDite(l)}`)
    .join(' ; ');
}

/** Qui mange, d'après les mangeurs du créneau (`eaters`). */
function direService(creneau) {
  const noms = (liste) =>
    liste.length > 1 ? `${liste.slice(0, -1).join(', ')} et ${liste[liste.length - 1]}` : liste.join('');
  const table = creneau.eaters.filter((e) => e.at === 'meal').map((e) => e.name);
  const boites = creneau.eaters.filter((e) => e.at === 'box').map((e) => e.name);
  const doubles = [...new Set(creneau.eaters.filter((e) => e.portions.starch >= 2).map((e) => e.name))];
  const phrases = [`${creneau.servings} portions.`];
  if (table.length) phrases.push(`À table : ${noms(table)}.`);
  if (boites.length) phrases.push(`En boîte pour demain midi : ${noms(boites)}.`);
  if (doubles.length) phrases.push(`Double féculent pour ${noms(doubles)}.`);
  return phrases.join(' ');
}

/** Texte prêt pour SSML. */
function ssml(texte) {
  return texte.replace(/&/g, ' et ').replace(/</g, '').replace(/>/g, '');
}

module.exports = {
  FUSEAU,
  aujourdhui,
  decaler,
  nomDuJour,
  quand,
  nombreDit,
  arrondir,
  quantiteEcrite,
  quantiteDite,
  duree,
  synonymes,
  suivante,
  precedente,
  etapeNumero,
  detaille,
  accelere,
  estDerniere,
  direEtape,
  direIngredients,
  direService,
  ssml,
};
