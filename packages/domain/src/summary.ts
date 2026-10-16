// Résumés lisibles d'un membre : barème et présence, pour les listes.
import { portionRoles, weekdaysShort } from './labels.ts';

type PortionRole = keyof typeof portionRoles;
export type Portions = Record<PortionRole, number>;
export interface Presence {
  lunch: boolean[];
  dinner: boolean[];
}

/** Facteur à la française : « ×2 », « ×0,6 ». */
export function formatFactor(factor: number): string {
  const rounded = Math.round(factor * 100) / 100;
  return `×${String(rounded).replace('.', ',')}`;
}

/** Ce qui s'écarte d'une portion standard : « féculents ×2 ». Vide si tout est à 1. */
export function portionsSummary(portions: Portions): string {
  const roles = Object.keys(portionRoles) as PortionRole[];
  const differing = roles.filter((role) => Math.abs(portions[role] - 1) > 0.001);
  if (differing.length === 0) return '';
  // Tout au même facteur (un enfant à 0,6) : un seul chiffre.
  const factors = new Set(differing.map((role) => portions[role]));
  if (differing.length >= 4 && factors.size === 1) {
    const others = roles.filter((role) => !differing.includes(role));
    const except = others.length ? ` sauf ${others.map((r) => portionRoles[r].toLowerCase()).join(', ')}` : '';
    return `portions ${formatFactor(portions[differing[0]])}${except}`;
  }
  return differing.map((role) => `${portionRoles[role].toLowerCase()} ${formatFactor(portions[role])}`).join(', ');
}

function days(present: boolean[]): string {
  return present
    .map((here, i) => (here ? weekdaysShort[i] : null))
    .filter(Boolean)
    .join(', ');
}

/** « Tous les repas », « Midi : mer., sam. », « Jamais le soir »… */
export function presenceSummary(presence: Presence): string {
  const all = (meal: boolean[]) => meal.every(Boolean);
  const none = (meal: boolean[]) => !meal.some(Boolean);
  if (all(presence.lunch) && all(presence.dinner)) return 'Tous les repas';
  if (none(presence.lunch) && none(presence.dinner)) return 'Jamais là';
  const parts: string[] = [];
  for (const [name, meal] of [['Midi', presence.lunch], ['Soir', presence.dinner]] as const) {
    if (all(meal)) continue;
    parts.push(none(meal) ? `jamais le ${name.toLowerCase()}` : `${name.toLowerCase()} : ${days(meal)}`);
  }
  const text = parts.join(' · ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Pour chercher sans se soucier des accents ni des majuscules : « oeuf » trouve « Œuf ». */
export function searchKey(text: string): string {
  return text
    .toLowerCase()
    .replaceAll('œ', 'oe')
    .replaceAll('æ', 'ae')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}
