// Ce que l'Echo Show affiche : les données du document APL (`apl.json`), une vue à la fois.
const p = require('./parole');

const majuscule = (t) => t.charAt(0).toUpperCase() + t.slice(1);

/** Identifiant d'un créneau, pour le toucher : `2026-10-18|dinner|chili`. */
const idCreneau = (s) => `${s.date}|${s.meal}|${s.recipe.slug}`;

function vueMenu(creneaux, jour) {
  return {
    vue: 'menu',
    titre: 'Au menu',
    periode: creneaux.length ? `${p.quand(creneaux[0].date, creneaux[0].meal, jour)} et après` : 'Rien de prévu',
    items: creneaux.map((s) => ({
      id: idCreneau(s),
      jour: majuscule(p.quand(s.date, s.meal, jour)),
      titre: s.recipe.title,
      meta: [s.variant.label, `${s.servings} portions`, `${s.variant.active_min} min`, s.express ? 'express' : null]
        .filter(Boolean)
        .join(' · '),
      ceSoir: s.date === jour,
    })),
  };
}

/** Vue d'une recette avant de commencer : `lignes` déjà calculées pour les portions. */
function vueRecette({ titre, quand, variante, portions, actif, etapes, lignes, id }) {
  return {
    vue: 'recette',
    id,
    jour: quand ? quand.toUpperCase() : '',
    titre,
    meta: [variante, portions ? `${portions} portions` : null, `${actif} min de travail`, `${etapes} étapes`]
      .filter(Boolean)
      .join(' · '),
    ingredients: lignes.map((l) => ({ nom: l.name, qte: p.quantiteEcrite(l) })),
  };
}

/** Vue de l'étape en cours, d'après la réponse de `/cooking`. */
function vueEtape(c) {
  const variante = c.recipe.variants.find((v) => v.label === c.variant) || c.recipe.variants[0];
  const pasAPas = c.mode === 'guided' && c.step !== null;
  return {
    vue: 'etape',
    titre: c.recipe.title,
    position: pasAPas
      ? `${c.current.title} · geste ${c.step + 1} / ${c.step_count}`
      : `Étape ${c.phase + 1} / ${c.phase_count}`,
    texte: c.current.text,
    minuteur: c.current.timer_s ? `⏱ ${Math.round(c.current.timer_s / 60)} min` : '',
    mode: pasAPas ? 'Pas à pas' : 'Rapide',
    etapes: variante.phases.map((phase, i) => ({
      n: i + 1,
      court: phase.title,
      active: i === c.phase,
      fait: i < c.phase,
    })),
  };
}

module.exports = { idCreneau, vueMenu, vueRecette, vueEtape };
