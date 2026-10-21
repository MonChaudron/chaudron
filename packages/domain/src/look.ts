// Présentation : nom court d'un plat, portions en clair, vignette d'une recette, avertissements regroupés.

/**
 * Nom d'un plat en deux temps : le plat, puis ce qui l'accompagne. « Blanquette de dinde aux champignons et
 * carottes, riz complet » : « Blanquette de dinde » et « aux champignons et carottes, riz complet ». Coupe à la
 * première virgule, puis, si le plat reste long, avant son premier « aux », « au », « à la »…
 */
export function splitTitle(title: string): { main: string; rest: string | null } {
  const comma = title.indexOf(', ');
  let main = comma > 0 ? title.slice(0, comma) : title;
  let rest = comma > 0 ? title.slice(comma + 2) : null;
  if (main.length > LONG) {
    const cut = main.search(/ (aux|au|à la|à l’|à l'|en) /);
    if (cut >= MIN_MAIN) {
      const garnish = main.slice(cut + 1);
      main = main.slice(0, cut);
      rest = rest ? `${garnish}, ${rest}` : garnish;
    }
  }
  return { main, rest };
}

/** Au-delà, le nom d'un plat se coupe avant sa garniture. */
const LONG = 24;
/** Mais pas au point de n'en garder qu'un mot (« Wok », « Pâtes »). */
const MIN_MAIN = 8;

/** Portions d'un plat : le total, et la part qui part en boîtes pour le lendemain midi. */
export function servingsParts(slot: { diners: number; next_lunch_covers: number; covers_next_lunch: boolean }): { total: number; boxes: number } {
  const boxes = slot.covers_next_lunch ? slot.next_lunch_covers : 0;
  return { total: slot.diners + boxes, boxes };
}

/** « 6 portions, dont 2 en boîtes ». */
export function servingsLabel(slot: { diners: number; next_lunch_covers: number; covers_next_lunch: boolean }): string {
  const { total, boxes } = servingsParts(slot);
  const base = `${total} portion${total > 1 ? 's' : ''}`;
  return boxes ? `${base}, dont ${boxes} en boîte${boxes > 1 ? 's' : ''}` : base;
}

// Vignettes

/** Teinte d'une vignette : une couleur de cuisine, la même en jour et en nuit (`vignettes` du thème). */
export type Hue = 'herb' | 'tomato' | 'saffron' | 'sea' | 'plum' | 'wheat';
/** Pictogramme d'une vignette : l'appareil principal de la façon de cuisiner. */
export type Pictogram = 'pot' | 'oven' | 'pan' | 'blender' | 'microwave' | 'bowl';

const hues: Hue[] = ['tomato', 'saffron', 'herb', 'plum', 'wheat', 'sea'];

const byEquipment: Record<string, Pictogram> = {
  cookeo_6l: 'pot',
  pressure_cooker: 'pot',
  oven: 'oven',
  airfryer: 'oven',
  hob: 'pan',
  thermomix_tm6: 'blender',
  blender: 'blender',
  microwave: 'microwave',
};

/**
 * Vignette d'une recette, sans photo : la teinte vient de son `slug` (la même sur tous les écrans, menu compris, qui
 * ne connaît pas les étiquettes) ; le pictogramme, du premier appareil de la façon de cuisiner (un bol sans appareil).
 */
export function vignetteOf(slug: string, equipment: string[]): { hue: Hue; pictogram: Pictogram } {
  let hash = 0;
  for (const c of slug) hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
  const pictogram = equipment.map((code) => byEquipment[code]).find(Boolean) ?? 'bowl';
  return { hue: hues[hash % hues.length], pictogram };
}

// Avertissements

export interface WarningLike {
  kind: string;
  message: string;
  member?: string | null;
  exclusion?: 'allergy' | 'dislike' | null;
  ingredient_name?: string | null;
}

/** Un avertissement à l'écran : une ligne. `alert` (allergie) en rouge, le reste en ambre. */
export interface Notice {
  tone: 'alert' | 'warn';
  icon: 'allergy' | 'dislike' | 'fresh' | 'info';
  text: string;
}

/** « Bastien », « Bastien et Vincent », « Elsa, Bastien et Vincent ». */
export function joinNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} et ${names[names.length - 1]}`;
}

/** Le message du serveur sans le repas qu'il rappelle (« mercredi 21 octobre soir : »), déjà affiché autour. */
export function withoutMeal(message: string): string {
  return message.replace(/^\S+ \d{1,2}(er)? \S+ (midi|soir) : /i, '').replace(/^./, (c) => c.toUpperCase());
}

/**
 * Les avertissements d'un plat, regroupés : un refus par ingrédient (« Bastien et Vincent n'aiment pas :
 * champignon »), une ligne pour tout ce qui ne tiendra pas jusque-là, puis le reste sans la date du repas.
 */
export function notices(warnings: WarningLike[]): Notice[] {
  const out: Notice[] = [];
  const exclusions = new Map<string, { kind: 'allergy' | 'dislike'; ingredient: string; members: string[] }>();
  const fresh: string[] = [];
  for (const w of warnings) {
    if (w.kind === 'exclusion' && w.exclusion && w.ingredient_name && w.member) {
      const key = `${w.exclusion}:${w.ingredient_name.toLowerCase()}`;
      const group = exclusions.get(key) ?? { kind: w.exclusion, ingredient: w.ingredient_name.toLowerCase(), members: [] };
      if (!group.members.includes(w.member)) group.members.push(w.member);
      exclusions.set(key, group);
    } else if (w.kind === 'freshness' && w.ingredient_name) {
      const name = w.ingredient_name.toLowerCase();
      if (!fresh.includes(name)) fresh.push(name);
    } else {
      out.push({ tone: 'warn', icon: 'info', text: withoutMeal(w.message) });
    }
  }
  const groups = [...exclusions.values()].sort((a, b) => (a.kind === b.kind ? 0 : a.kind === 'allergy' ? -1 : 1));
  const head: Notice[] = groups.map((g) =>
    g.kind === 'allergy'
      ? { tone: 'alert', icon: 'allergy', text: `${joinNames(g.members)} ${g.members.length > 1 ? 'sont allergiques' : 'est allergique'} : ${g.ingredient}` }
      : { tone: 'warn', icon: 'dislike', text: `${joinNames(g.members)} ${g.members.length > 1 ? 'n’aiment' : 'n’aime'} pas : ${g.ingredient}` },
  );
  if (fresh.length) head.push({ tone: 'warn', icon: 'fresh', text: `Ne tiendra pas jusque-là : ${fresh.join(', ')}` });
  return [...head, ...out];
}
