// Éditeur de recettes : slug tiré du titre, recette lue → corps du PUT.
import type { Unit } from './recipes.ts';
import type { Portions } from './summary.ts';
import { searchKey } from './summary.ts';

/** Slug valide pour le serveur : minuscules, chiffres, tirets simples, 80 caractères au plus. */
export function slugify(title: string): string {
  return searchKey(title)
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80)
    .replace(/-+$/, '');
}

type State = 'fresh' | 'frozen' | 'canned' | 'dried' | 'cooked';

export interface StepData {
  key?: string | null;
  text: string;
  spoken_text?: string | null;
  timer_s?: number | null;
  video_url?: string | null;
  equipment?: string | null;
  params?: unknown;
}

export interface PhaseData {
  key?: string | null;
  title: string;
  text: string;
  spoken_text?: string | null;
  timer_s?: number | null;
  day_offset: number;
  steps: readonly StepData[];
}

export interface VariantData {
  key?: string | null;
  label: string;
  equipment: readonly string[];
  active_min: number;
  total_min: number;
  vessels: number;
  is_default: boolean;
  phases: readonly PhaseData[];
}

export interface LineData {
  key?: string | null;
  ingredient: string;
  state: State;
  qty?: number | null;
  unit: Unit;
  role: keyof Portions;
  optional: boolean;
  per_batch: boolean;
  note?: string | null;
}

/** Recette telle que l'API la renvoie (les champs utiles à l'éditeur). */
export interface RecipeData {
  slug: string;
  title: string;
  description: string;
  tags: readonly string[];
  season_months: readonly number[];
  source?: string | null;
  ingredients: readonly LineData[];
  variants: readonly VariantData[];
}

/** La clé stable d'un élément, quand il en a une (le serveur garde la place des notes et des propositions). */
const keyOf = (item: { key?: string | null }) => (item.key ? { key: item.key } : {});

/** Corps du PUT pour republier une recette lue, sans les champs calculés par le serveur ; les clés sont gardées. */
export function recipeToInput(recipe: RecipeData) {
  return {
    slug: recipe.slug,
    title: recipe.title,
    description: recipe.description,
    tags: [...recipe.tags],
    season_months: [...recipe.season_months],
    source: recipe.source ?? null,
    ingredients: recipe.ingredients.map((l) => ({
      ...keyOf(l),
      ingredient: l.ingredient,
      state: l.state,
      qty: l.qty ?? null,
      unit: l.unit,
      role: l.role,
      optional: l.optional,
      per_batch: l.per_batch,
      note: l.note ?? null,
    })),
    variants: recipe.variants.map((v) => ({
      ...keyOf(v),
      label: v.label,
      equipment: [...v.equipment],
      active_min: v.active_min,
      total_min: v.total_min,
      vessels: v.vessels,
      is_default: v.is_default,
      phases: v.phases.map((p) => ({
        ...keyOf(p),
        title: p.title,
        text: p.text,
        spoken_text: p.spoken_text ?? null,
        timer_s: p.timer_s ?? null,
        day_offset: p.day_offset,
        steps: p.steps.map((s) => ({
          ...keyOf(s),
          text: s.text,
          spoken_text: s.spoken_text ?? null,
          timer_s: s.timer_s ?? null,
          video_url: s.video_url ?? null,
          equipment: s.equipment ?? null,
          params: s.params ?? {},
        })),
      })),
    })),
  };
}

/** Nom et forme d'achat proposés pour un ingrédient créé depuis une ligne Ciqual (« Petits pois, surgelés, crus »). */
export function fromFood(foodName: string): { name: string; state: State } {
  const name = foodName.split(',')[0].trim();
  const key = searchKey(foodName);
  const state: State = /surgel/.test(key)
    ? 'frozen'
    : /appertis|en conserve|semi-conserve/.test(key)
      ? 'canned'
      : /deshydrat|\bseche?s?\b/.test(key)
        ? 'dried'
        : /, (cuit|cuite|cuits|cuites)\b|bouilli/.test(key)
          ? 'cooked'
          : 'fresh';
  return { name, state };
}

/** Rôle proposé pour une ligne, d'après le rayon de l'ingrédient. */
export function roleForAisle(aisle: string): keyof Portions {
  if (aisle === 'butcher' || aisle === 'fish') return 'protein';
  if (aisle === 'produce' || aisle === 'frozen') return 'veg';
  return 'other';
}
