// Recettes : quantités pour le foyer, affichage, écarts entre variantes.
import type { Portions } from './summary.ts';

export type Unit = 'g' | 'ml' | 'piece' | 'tbsp' | 'tsp' | 'pinch';

/** Ligne d'une recette, pour une portion standard. */
export interface PortionLine {
  /** Nulle ou absente : « au goût ». */
  qty?: number | null;
  unit: Unit;
  role: keyof Portions;
  /** Ne suit pas le nombre de portions. */
  per_batch: boolean;
}

export interface Eater {
  name: string;
  portions: Portions;
}

export interface Scaled {
  total: number | null;
  /** Part de chacun ; vide si la quantité ne dépend pas des mangeurs. */
  perEater: { name: string; qty: number }[];
}

/**
 * Quantité d'une ligne pour ceux qui mangent : la portion standard multipliée par le facteur de chacun pour le rôle
 * de l'ingrédient (barème). Sans mangeur, la portion standard.
 */
export function scaleLine(line: PortionLine, eaters: Eater[]): Scaled {
  if (line.qty == null) return { total: null, perEater: [] };
  if (line.per_batch) return { total: line.qty, perEater: [] };
  if (eaters.length === 0) return { total: line.qty, perEater: [] };
  const qty = line.qty;
  const perEater = eaters.map((e) => ({ name: e.name, qty: qty * e.portions[line.role] }));
  return { total: perEater.reduce((sum, e) => sum + e.qty, 0), perEater };
}

const decimal = (n: number) => String(Math.round(n * 100) / 100).replace('.', ',');

/** Arrondi lisible d'une masse ou d'un volume : à l'unité sous 20, à 5 sous 250, à 10 au-delà. */
function roundMeasure(q: number): number {
  if (q < 20) return Math.max(1, Math.round(q));
  if (q < 250) return Math.round(q / 5) * 5;
  return Math.round(q / 10) * 10;
}

/** Quart le plus proche, en fraction : « ½ », « 2 ¼ » ; jamais zéro. */
function quarters(q: number): string {
  const n = Math.max(0.25, Math.round(q * 4) / 4);
  const whole = Math.floor(n);
  const fraction = ['', '¼', '½', '¾'][Math.round((n - whole) * 4)];
  if (!whole) return fraction;
  return fraction ? `${whole} ${fraction}` : String(whole);
}

/** « 540 g », « 1,25 kg », « 2 ½ », « 1 c. à s. », « au goût ». */
export function formatQuantity(qty: number | null | undefined, unit: Unit): string {
  if (qty == null) return 'au goût';
  switch (unit) {
    case 'g':
    case 'ml': {
      const big = unit === 'g' ? 'kg' : 'l';
      const q = roundMeasure(qty);
      return q >= 1000 ? `${decimal(q / 1000)} ${big}` : `${q} ${unit}`;
    }
    case 'piece':
      return quarters(qty);
    case 'tbsp':
      return `${quarters(qty)} c. à s.`;
    case 'tsp':
      return `${quarters(qty)} c. à c.`;
    case 'pinch': {
      const n = Math.max(1, Math.round(qty));
      return `${n} pincée${n > 1 ? 's' : ''}`;
    }
  }
}

/** « 35 min », « 1 h », « 1 h 05 ». */
export function formatDuration(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h} h ${String(m).padStart(2, '0')}` : `${h} h`;
}

export interface VariantTimes {
  active_min: number;
  total_min: number;
  vessels: number;
}

/** Ce que coûte une variante par rapport à une autre : « +25 min, +1 récipient », « même temps ». */
export function variantDelta(base: VariantTimes, other: VariantTimes): string {
  const parts: string[] = [];
  const minutes = other.total_min - base.total_min;
  if (minutes) parts.push(`${minutes > 0 ? '+' : '−'}${formatDuration(Math.abs(minutes))}`);
  const vessels = other.vessels - base.vessels;
  if (vessels) {
    const n = Math.abs(vessels);
    parts.push(`${vessels > 0 ? '+' : '−'}${n} récipient${n > 1 ? 's' : ''}`);
  }
  return parts.length ? parts.join(', ') : 'même temps';
}

const months = ['janvier', 'février', 'mars', 'avril', 'mai', 'juin', 'juillet', 'août', 'septembre', 'octobre', 'novembre', 'décembre'];

/** Saison d'une recette : « d’octobre à février » (mois consécutifs, même à cheval sur l'année), « en août », ou la liste. */
export function seasonText(season: number[]): string {
  const set = new Set(season.filter((m) => m >= 1 && m <= 12));
  if (set.size === 0 || set.size === 12) return '';
  const next = (m: number) => (m % 12) + 1;
  const previous = (m: number) => ((m + 10) % 12) + 1;
  const name = (m: number) => months[m - 1];
  if (set.size === 1) return `en ${name([...set][0])}`;
  // Une seule suite de mois : son premier mois n'a pas de précédent dans la saison.
  const starts = [...set].filter((m) => !set.has(previous(m)));
  if (starts.length === 1) {
    let end = starts[0];
    while (set.has(next(end))) end = next(end);
    const from = /^[aeiouyéèê]/.test(name(starts[0])) ? `d’${name(starts[0])}` : `de ${name(starts[0])}`;
    return `${from} à ${name(end)}`;
  }
  return `en ${[...set].sort((a, b) => a - b).map(name).join(', ')}`;
}
