// Apports d'une portion (design-nutrition.md) : des estimations, jamais des mesures. Les apports et les poids
// s'écrivent avec « ~ » et arrondis (kcal à 10 près, grammes à l'unité, poids à servir à 5 g près, sel à 0,1 g).

export type Grade = 'a' | 'b' | 'c' | 'd' | 'e';
export type Criterion = 'energy' | 'sugars' | 'sat_fat' | 'salt' | 'protein' | 'fiber' | 'fvl';

export interface FactorView {
  criterion: Criterion;
  points: number;
  ingredient?: string | null;
  amount?: number | null;
}

/** Ce que la mise en forme lit de l'objet `nutrition` de l'API. */
export interface NutritionView {
  grade?: Grade | null;
  portion: 'member' | 'standard';
  complete: boolean;
  cooked_g: number;
  missing: string[];
  amounts: { energy_kcal: number; protein_g: number };
  serve: { role: string; label: string; grams: number }[];
}

const whole = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });
const tenth = new Intl.NumberFormat('fr-FR', { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const roundTo = (value: number, step: number) => Math.round(value / step) * step;
const lowerFirst = (text: string) => text.charAt(0).toLocaleLowerCase('fr-FR') + text.slice(1);

/** « ~540 kcal ». */
export function formatKcal(kcal: number): string {
  return `~${whole.format(roundTo(kcal, 10))} kcal`;
}

/** « 39 g ». */
export function formatGrams(g: number): string {
  return `${whole.format(Math.round(g))} g`;
}

/** Un poids cuit à servir : « ~105 g », jamais moins de 5 g. */
export function formatServe(g: number): string {
  return `~${whole.format(Math.max(5, roundTo(g, 5)))} g`;
}

/** « 2,9 g ». */
export function formatSalt(g: number): string {
  return `${tenth.format(g)} g`;
}

export function gradeLetter(grade: Grade): string {
  return grade.toUpperCase();
}

/** Quelque chose à dire : un poids, ou des ingrédients sans données. Sinon (une recette sans rien à peser), on se
 * tait plutôt que d'afficher « ~0 kcal ». */
export function weighed(n: NutritionView): boolean {
  return n.cooked_g > 0 || n.missing.length > 0;
}

/** Pourquoi la note manque ; rien quand elle est là. */
export function gradeMissingText(n: NutritionView): string | null {
  if (n.grade) return null;
  if (!n.missing.length) return 'Note indisponible : rien à peser.';
  return `Note indisponible : il manque des données pour ${n.missing.map(lowerFirst).join(', ')}.`;
}

/** « Ta portion : ~540 kcal · 39 g de protéines » ; « Une portion » sans membre relié ; « au moins » s'il manque des données. */
export function portionLine(n: NutritionView): string {
  const who = n.portion === 'member' ? 'Ta portion' : 'Une portion';
  return `${who} : ${n.complete ? '' : 'au moins '}${formatKcal(n.amounts.energy_kcal)} · ${formatGrams(n.amounts.protein_g)} de protéines`;
}

/** « À servir : paleron de bœuf ~105 g · légumes ~280 g » ; vide sans rien à servir. */
export function serveLine(n: NutritionView): string {
  if (!n.serve.length) return '';
  return `À servir : ${n.serve.map((s) => `${lowerFirst(s.label)} ${formatServe(s.grams)}`).join(' · ')}`;
}

const criterionNames: Record<Criterion, string> = {
  energy: 'l’énergie',
  sugars: 'les sucres',
  sat_fat: 'les graisses saturées',
  salt: 'le sel',
  protein: 'les protéines',
  fiber: 'les fibres',
  fvl: 'les fruits et légumes',
};

/** Un critère de la note et l'ingrédient qui y pèse le plus : « le sel (cube de bouillon : 1,9 g) ». */
export function factorText(f: FactorView): string {
  const name = criterionNames[f.criterion];
  if (!f.ingredient || f.amount == null) return name;
  const amount = f.criterion === 'energy' ? formatKcal(f.amount) : f.criterion === 'salt' ? formatSalt(f.amount) : formatGrams(f.amount);
  return `${name} (${lowerFirst(f.ingredient)} : ${amount})`;
}
