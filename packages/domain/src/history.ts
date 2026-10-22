// Historique des recettes : la différence entre deux versions, élément par élément (par leur clé stable), et mot à
// mot dans les textes.
/** Contenu d'une version : la recette telle qu'on la publie, clés comprises (champs facultatifs comme dans l'API). */
export interface VersionLine {
  key?: string | null;
  ingredient: string;
  state?: string | null;
  qty?: number | null;
  unit?: string;
  role?: string;
  optional?: boolean;
  per_batch?: boolean;
  note?: string | null;
}

export interface VersionStep {
  key?: string | null;
  text: string;
  spoken_text?: string | null;
  timer_s?: number | null;
  video_url?: string | null;
  equipment?: string | null;
  params?: unknown;
}

export interface VersionPhase {
  key?: string | null;
  title: string;
  text: string;
  spoken_text?: string | null;
  timer_s?: number | null;
  day_offset?: number;
  steps?: readonly VersionStep[];
}

export interface VersionVariant {
  key?: string | null;
  label: string;
  equipment?: readonly string[];
  active_min?: number;
  total_min?: number;
  vessels?: number | null;
  is_default?: boolean;
  phases: readonly VersionPhase[];
}

export interface VersionContent {
  slug?: string | null;
  title: string;
  description?: string;
  tags?: readonly string[];
  season_months?: readonly number[];
  source?: string | null;
  ingredients: readonly VersionLine[];
  variants: readonly VersionVariant[];
  message?: string | null;
}

export type ChangeKind = 'added' | 'removed' | 'changed' | 'moved';

export interface FieldChange {
  field: string;
  before: unknown;
  after: unknown;
}

/** Un élément qui a changé : où (« Cookeo › 2. La farce »), et les champs touchés. */
export interface RecipeChange {
  kind: ChangeKind;
  where: string;
  key?: string;
  fields: FieldChange[];
}

export interface Piece {
  kind: 'same' | 'added' | 'removed';
  text: string;
}

type Op = ['same', number, number] | ['removed', number] | ['added', number];

/** Plus longue sous-suite commune : les opérations pour passer de `a` à `b`, retraits avant ajouts. */
function lcs<T>(a: readonly T[], b: readonly T[], eq: (x: T, y: T) => boolean): Op[] {
  const n = a.length;
  const m = b.length;
  const table: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i][j] = eq(a[i], b[j]) ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && eq(a[i], b[j])) {
      ops.push(['same', i++, j++]);
    } else if (j >= m || (i < n && table[i + 1][j] >= table[i][j + 1])) {
      ops.push(['removed', i++]);
    } else {
      ops.push(['added', j++]);
    }
  }
  return ops;
}

/** Différence mot à mot : morceaux identiques, retirés, ajoutés (les espaces suivent les mots). */
export function diffWords(before: string, after: string): Piece[] {
  const tokens = (text: string) => text.split(/(\s+)/).filter((t) => t !== '');
  const a = tokens(before);
  const b = tokens(after);
  const pieces: Piece[] = [];
  const push = (kind: Piece['kind'], text: string) => {
    const last = pieces[pieces.length - 1];
    if (last?.kind === kind) last.text += text;
    else pieces.push({ kind, text });
  };
  for (const op of lcs(a, b, (x, y) => x === y)) {
    if (op[0] === 'same') push('same', a[op[1]]);
    else if (op[0] === 'removed') push('removed', a[op[1]]);
    else push('added', b[op[1]]);
  }
  return pieces;
}

const same = (x: unknown, y: unknown) => JSON.stringify(x ?? null) === JSON.stringify(y ?? null);

function fields(before: object, after: object, names: readonly string[]): FieldChange[] {
  const b = before as Record<string, unknown>;
  const a = after as Record<string, unknown>;
  return names.filter((name) => !same(b[name], a[name])).map((field) => ({ field, before: b[field] ?? null, after: a[field] ?? null }));
}

const LINE_FIELDS = ['ingredient', 'state', 'qty', 'unit', 'role', 'optional', 'per_batch', 'note'] as const;
const VARIANT_FIELDS = ['label', 'equipment', 'active_min', 'total_min', 'vessels', 'is_default'] as const;
const PHASE_FIELDS = ['title', 'text', 'spoken_text', 'timer_s', 'day_offset'] as const;
const STEP_FIELDS = ['text', 'spoken_text', 'timer_s', 'video_url', 'equipment'] as const;

/**
 * Compare deux listes d'éléments par leur clé : ajoutés, retirés, changés, déplacés (le moins de déplacements
 * possible). `visit` décrit un élément présent des deux côtés, à sa nouvelle place.
 */
function compare<T extends { key?: string | null }>(
  before: readonly T[],
  after: readonly T[],
  where: (item: T, index: number) => string,
  visit: (before: T, after: T, index: number) => RecipeChange[],
): RecipeChange[] {
  const keyOf = (item: T, index: number) => item.key ?? `#${index}`;
  const beforeKeys = before.map(keyOf);
  const afterKeys = after.map(keyOf);
  const kept = new Set(
    lcs(beforeKeys, afterKeys, (x, y) => x === y)
      .filter((op) => op[0] === 'same')
      .map((op) => afterKeys[op[2] as number]),
  );
  const changes: RecipeChange[] = [];
  after.forEach((item, index) => {
    const key = afterKeys[index];
    const old = beforeKeys.indexOf(key);
    if (old < 0) {
      changes.push({ kind: 'added', where: where(item, index), key, fields: [] });
      return;
    }
    const inner = visit(before[old], item, index);
    if (!kept.has(key) && !inner.some((c) => c.key === key)) {
      changes.push({ kind: 'moved', where: where(item, index), key, fields: [] });
    }
    changes.push(...inner);
  });
  before.forEach((item, index) => {
    if (!afterKeys.includes(beforeKeys[index])) {
      changes.push({ kind: 'removed', where: where(item, index), key: beforeKeys[index], fields: [] });
    }
  });
  return changes;
}

/**
 * Ce qui a changé d'une version à l'autre. `names` : noms des ingrédients par code (sinon le code).
 */
export function diffRecipes(before: VersionContent, after: VersionContent, names: Record<string, string> = {}): RecipeChange[] {
  const changes: RecipeChange[] = [];
  const top = fields(before, after, ['title', 'description', 'tags', 'season_months', 'source']);
  if (top.length) changes.push({ kind: 'changed', where: 'Recette', fields: top });

  const lineWhere = (l: VersionLine) => `Ingrédient : ${names[l.ingredient] ?? l.ingredient}`;
  changes.push(
    ...compare(before.ingredients, after.ingredients, lineWhere, (b, a) => {
      const f = fields(b, a, LINE_FIELDS);
      return f.length ? [{ kind: 'changed', where: lineWhere(a), key: a.key ?? undefined, fields: f }] : [];
    }),
  );

  changes.push(
    ...compare(
      before.variants,
      after.variants,
      (v) => v.label,
      (bv, av) => {
        const out: RecipeChange[] = [];
        const f = fields(bv, av, VARIANT_FIELDS);
        if (f.length) out.push({ kind: 'changed', where: av.label, key: av.key ?? undefined, fields: f });
        const phaseWhere = (p: VersionPhase, i: number) => `${av.label} › ${i + 1}. ${p.title}`;
        out.push(
          ...compare(bv.phases, av.phases, phaseWhere, (bp, ap, i) => {
            const inner: RecipeChange[] = [];
            const pf = fields(bp, ap, PHASE_FIELDS);
            if (pf.length) inner.push({ kind: 'changed', where: phaseWhere(ap, i), key: ap.key ?? undefined, fields: pf });
            const stepWhere = (_: VersionStep, j: number) => `${phaseWhere(ap, i)} › geste ${j + 1}`;
            inner.push(
              ...compare(bp.steps ?? [], ap.steps ?? [], stepWhere, (bs, as, j) => {
                const sf = fields(bs, as, STEP_FIELDS);
                return sf.length ? [{ kind: 'changed', where: stepWhere(as, j), key: as.key ?? undefined, fields: sf }] : [];
              }),
            );
            return inner;
          }),
        );
        return out;
      },
    ),
  );
  return changes;
}

/** Nom d'un champ, pour l'affichage d'une différence. */
export const fieldLabels: Record<string, string> = {
  title: 'Titre',
  description: 'Présentation',
  tags: 'Étiquettes',
  season_months: 'Saison',
  source: 'Source',
  ingredient: 'Ingrédient',
  state: 'Forme',
  qty: 'Quantité',
  unit: 'Unité',
  role: 'Rôle',
  optional: 'Facultatif',
  per_batch: 'Par fournée',
  note: 'Note',
  label: 'Nom',
  equipment: 'Matériel',
  active_min: 'Temps actif',
  total_min: 'Temps total',
  vessels: 'Récipients',
  is_default: 'Par défaut',
  text: 'Texte',
  spoken_text: 'Texte lu par Alexa',
  timer_s: 'Minuteur',
  day_offset: 'Jour',
  video_url: 'Vidéo',
};
