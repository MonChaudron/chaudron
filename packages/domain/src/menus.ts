// Menus : dates, portions d'un créneau, copie d'une semaine. Les dates sont des jours ISO (`2026-10-18`).
import type { Eater, PortionLine, Scaled } from './recipes.ts';
import type { Portions } from './summary.ts';

/** Jour ISO décalé de `n` jours (calcul en UTC : pas d'effet des changements d'heure). */
export function addDays(iso: string, n: number): string {
  const [y, m, d] = iso.split('-').map(Number);
  const date = new Date(Date.UTC(y, m - 1, d + n));
  return date.toISOString().slice(0, 10);
}

/** `n` jours à partir de `start`. */
export function weekDays(start: string, n: number): string[] {
  return Array.from({ length: n }, (_, i) => addDays(start, i));
}

/** Aujourd'hui, à l'heure du téléphone. */
export function todayIso(now = new Date()): string {
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 10);
}

const weekdays = ['dimanche', 'lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi'];
const months = ['janvier', 'février', 'mars', 'avril', 'mai', 'juin', 'juillet', 'août', 'septembre', 'octobre', 'novembre', 'décembre'];

/** « mercredi 21 octobre », « dimanche 1er novembre ». */
export function dateText(iso: string): string {
  const [y, m, d] = iso.split('-').map(Number);
  const weekday = weekdays[new Date(Date.UTC(y, m - 1, d)).getUTCDay()];
  return `${weekday} ${d === 1 ? '1er' : d} ${months[m - 1]}`;
}

/** Jour court : « mer. 21 ». */
export function shortDateText(iso: string): string {
  const [y, m, d] = iso.split('-').map(Number);
  const weekday = weekdays[new Date(Date.UTC(y, m - 1, d)).getUTCDay()];
  return `${weekday.slice(0, 3)}. ${d}`;
}

/** « Aujourd’hui », « Demain », « Hier », sinon « Mercredi 21 octobre ». */
export function dayLabel(iso: string, today: string): string {
  if (iso === today) return 'Aujourd’hui';
  if (iso === addDays(today, 1)) return 'Demain';
  if (iso === addDays(today, -1)) return 'Hier';
  const text = dateText(iso);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** « 4 + 2 = 6 » quand le plat fait aussi les boîtes du lendemain, sinon « 4 ». */
export function servingsText(slot: { diners: number; next_lunch_covers: number; covers_next_lunch: boolean }): string {
  if (!slot.covers_next_lunch || slot.next_lunch_covers === 0) return String(slot.diners);
  return `${slot.diners} + ${slot.next_lunch_covers} = ${slot.diners + slot.next_lunch_covers}`;
}

/** Quantité d'une ligne pour un créneau : la portion standard × le multiplicateur de son rôle. */
export function scaleWith(line: PortionLine, scale: Portions): Scaled {
  if (line.qty == null) return { total: null, perEater: [] };
  if (line.per_batch) return { total: line.qty, perEater: [] };
  return { total: line.qty * scale[line.role], perEater: [] };
}

interface SlotEaters {
  diners_fixed: boolean;
  next_lunch_covers_fixed: boolean;
  eaters: { name: string; at: string; portions: Portions }[];
}

/**
 * Qui mange ce plat, pour le détail par membre : au repas, puis en boîte le lendemain. Vide quand un nombre est
 * fixé (un invité, deux boîtes) : les quantités viennent alors du multiplicateur, pas de chacun.
 */
export function slotEaters(slot: SlotEaters): Eater[] {
  if (slot.diners_fixed || slot.next_lunch_covers_fixed) return [];
  return slot.eaters.map((e) => ({ name: e.at === 'box' ? `${e.name} (boîte)` : e.name, portions: e.portions }));
}

interface CopiedSlot {
  date: string;
  meal: 'lunch' | 'dinner';
  recipe: { slug: string };
  variant: { label: string };
  covers_next_lunch: boolean;
  note?: string | null;
}

/** Créneau à publier (`PUT …/menus/{week_start}`). */
export interface SlotDraft {
  date: string;
  meal: 'lunch' | 'dinner';
  recipe: string;
  variant: string;
  covers_next_lunch: boolean;
  note: string | null;
}

/**
 * Refaire une semaine : les mêmes plats, décalés de `from` à `to`. Les portions repartent de la présence (ni nombre
 * fixé, ni jour d'origine d'un plat déplacé).
 */
export function copySlots(slots: CopiedSlot[], from: string, to: string): SlotDraft[] {
  const days = Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
  return slots.map((s) => ({
    date: addDays(s.date, days),
    meal: s.meal,
    recipe: s.recipe.slug,
    variant: s.variant.label,
    covers_next_lunch: s.covers_next_lunch,
    note: s.note ?? null,
  }));
}

/** Premier jour proposé pour un nouveau menu : le lendemain du dernier repas prévu, et pas avant aujourd'hui. */
export function nextMenuStart(menus: { last_date?: string | null }[], today: string): string {
  const last = menus.map((m) => m.last_date).filter((d): d is string => !!d).sort().at(-1);
  if (!last) return today;
  const next = addDays(last, 1);
  return next > today ? next : today;
}
