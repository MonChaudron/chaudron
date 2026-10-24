// Thèmes de Chaudron (design §6) : quatre directions, chacune en jour et en nuit, au choix sur chaque appareil.
// Contrastes vérifiés (WCAG) : textes et liens au moins 4,5:1 sur le fond et sur les cartes, dans les huit palettes.
// Polices servies par Chaudron (SIL Open Font License 1.1, apps/app/public/fonts/OFL-*.txt).

import type { Hue } from './look.ts';
import type { Grade } from './nutrition.ts';

export type Mode = 'light' | 'dark';
export type ThemeId = 'chartreuse' | 'basilic' | 'safran' | 'encre';

export interface Palette {
  bg: string;
  card: string;
  text: string;
  muted: string;
  line: string;
  /** Fond du bouton principal, et texte posé dessus. */
  action: string;
  onAction: string;
  /** Contour du bouton principal, quand son fond se distingue mal du fond de page. */
  actionBorder: string;
  link: string;
  /** Étiquettes et pastilles sélectionnées. */
  tagBg: string;
  tagText: string;
  alert: string;
  /** Avertissement sans gravité (n'aime pas, fraîcheur), en texte : ambre. */
  warn: string;
  /** Fond des pastilles neutres (portions, durée) et des zones calmes. */
  chip: string;
  /** Le o du logo. */
  logoO: string;
}

/** Graisses, sous la forme attendue par React Native. */
export type Weight = '400' | '500' | '600' | '700' | '800' | '900';

export interface Theme {
  id: ThemeId;
  name: string;
  description: string;
  fonts: { title: string; text: string };
  weights: { title: Weight; medium: Weight; strong: Weight };
  /** Hauteur des minuscules de la police de titre, en em : cale le o du logo. */
  xHeight: number;
  radius: { card: number; button: number; tag: number };
  palettes: Record<Mode, Palette>;
}

/**
 * Pastilles de prix (design-prix.md) : vert pour le tiers le moins cher, ambre pour le milieu, rouge pour le plus
 * cher. Communes aux quatre thèmes : le code couleur doit se lire pareil partout.
 */
export const priceTierColors: Record<Mode, Record<'low' | 'mid' | 'high', { bg: string; fg: string }>> = {
  light: {
    low: { bg: '#D8F0DC', fg: '#1B5E2A' },
    mid: { bg: '#FBE8C8', fg: '#6E4700' },
    high: { bg: '#F7D6D2', fg: '#8F2718' },
  },
  dark: {
    low: { bg: '#1F3A26', fg: '#A8E6B8' },
    mid: { bg: '#3D2F12', fg: '#F4C77A' },
    high: { bg: '#42201B', fg: '#FFB3A6' },
  },
};

/**
 * Pastilles de la note d'équilibre (design-nutrition.md), de A à E : du vert au rouge, en tons du thème, jamais
 * ceux du logo officiel du Nutri-Score. Communes aux quatre thèmes.
 */
export const gradeColors: Record<Mode, Record<Grade, { bg: string; fg: string }>> = {
  light: {
    a: { bg: '#C8EBC9', fg: '#14532D' },
    b: { bg: '#E2F3B5', fg: '#3F5208' },
    c: { bg: '#FBEFB8', fg: '#6B5300' },
    d: { bg: '#FBDDC0', fg: '#7A3B00' },
    e: { bg: '#F6CFC8', fg: '#8A2216' },
  },
  dark: {
    a: { bg: '#1C3A24', fg: '#9FE3B0' },
    b: { bg: '#2C3812', fg: '#D4EF8A' },
    c: { bg: '#3B3410', fg: '#F1D873' },
    d: { bg: '#40270F', fg: '#F7B77A' },
    e: { bg: '#44201A', fg: '#FFAD9E' },
  },
};

/** D'où viennent les calories d'un repas : quatre teintes distinctes, communes aux quatre thèmes. */
export const energySplitColors: Record<Mode, { protein: string; carbs: string; fat: string; fiber: string }> = {
  light: { protein: '#2F6B3A', carbs: '#D9A21B', fat: '#C2552D', fiber: '#8AA05A' },
  dark: { protein: '#7FD39A', carbs: '#F2C14E', fat: '#F08A63', fiber: '#B5C98A' },
};

export const themes: Record<ThemeId, Theme> = {
  chartreuse: {
    id: 'chartreuse',
    name: 'Chartreuse',
    description: 'Vert-jaune pâle, boutons chartreuse, titres larges en Unbounded.',
    fonts: { title: 'Unbounded', text: 'Atkinson Hyperlegible Next' },
    weights: { title: '700', medium: '600', strong: '800' },
    xHeight: 0.58,
    radius: { card: 22, button: 16, tag: 999 },
    palettes: {
      light: {
        // Fond à peine teinté : le chartreuse se garde pour ce qui compte (ce soir, l'action, la progression).
        bg: '#F2F5E4', card: '#FFFFFB', text: '#1C2208', muted: '#4F5833', line: '#DFE6C3',
        // Fond chartreuse à 1,3:1 sur le fond de page : bouton cerné.
        action: '#C3E534', onAction: '#1C2208', actionBorder: '#1C2208',
        link: '#4A6400', tagBg: '#283210', tagText: '#D7F55A', alert: '#B23A1E', warn: '#875200', chip: '#E9EFCF', logoO: '#4A6400',
      },
      dark: {
        bg: '#11140A', card: '#1C2111', text: '#EEF3DC', muted: '#AEB78F', line: '#2F361A',
        action: '#C9F23F', onAction: '#151A06', actionBorder: 'transparent',
        link: '#C9F23F', tagBg: '#313D10', tagText: '#DAF76C', alert: '#FF9C82', warn: '#F4BC5E', chip: '#272E18', logoO: '#C9F23F',
      },
    },
  },
  basilic: {
    id: 'basilic',
    name: 'Basilic',
    description: 'Vert sauge, boutons basilic ; une seule police, la plus lisible de loin.',
    fonts: { title: 'Atkinson Hyperlegible Next', text: 'Atkinson Hyperlegible Next' },
    weights: { title: '700', medium: '600', strong: '800' },
    xHeight: 0.52,
    radius: { card: 20, button: 14, tag: 999 },
    palettes: {
      light: {
        bg: '#E4EEDF', card: '#F9FBF7', text: '#17241B', muted: '#46564B', line: '#CFDDCB',
        action: '#2E7D4F', onAction: '#FFFFFF', actionBorder: 'transparent',
        // Plus foncé que le bouton : à 4,2:1 sur le fond, le vert basilic ne suffisait pas pour du texte.
        link: '#286E45', tagBg: '#C9E3CF', tagText: '#1A4D30', alert: '#B23A1E', warn: '#875200', chip: '#DDEAD8', logoO: '#286E45',
      },
      dark: {
        bg: '#101813', card: '#1A251E', text: '#E6EEE8', muted: '#A7B6AB', line: '#2B3A31',
        action: '#74C795', onAction: '#0E1B13', actionBorder: 'transparent',
        link: '#74C795', tagBg: '#24402D', tagText: '#BFE6CB', alert: '#FF9C82', warn: '#F4BC5E', chip: '#223129', logoO: '#74C795',
      },
    },
  },
  safran: {
    id: 'safran',
    name: 'Safran',
    description: 'Abricot et crème, étiquettes safran, titres en Bricolage Grotesque.',
    fonts: { title: 'Bricolage Grotesque', text: 'Source Sans 3' },
    weights: { title: '700', medium: '600', strong: '700' },
    xHeight: 0.52,
    radius: { card: 24, button: 999, tag: 999 },
    palettes: {
      light: {
        bg: '#FAE3C2', card: '#FFF8EE', text: '#2A1E14', muted: '#6A5444', line: '#EBD2AE',
        action: '#1F6F78', onAction: '#FFFFFF', actionBorder: 'transparent',
        link: '#1F6F78', tagBg: '#F6C35B', tagText: '#3A2A10', alert: '#A3361F', warn: '#7A4500', chip: '#F6DDB9', logoO: '#1F6F78',
      },
      dark: {
        bg: '#1C1510', card: '#2A2019', text: '#F7EBDD', muted: '#C4AE98', line: '#3D3026',
        action: '#F4B740', onAction: '#22180C', actionBorder: 'transparent',
        link: '#F4B740', tagBg: '#3D3220', tagText: '#F6D58A', alert: '#FF9C82', warn: '#F4BC5E', chip: '#382A20', logoO: '#F4B740',
      },
    },
  },
  encre: {
    id: 'encre',
    name: 'Encre',
    description: 'Pervenche et blanc, bleu encre et étiquettes corail, tout en Lexend.',
    fonts: { title: 'Lexend', text: 'Lexend' },
    weights: { title: '500', medium: '500', strong: '700' },
    xHeight: 0.5,
    radius: { card: 16, button: 10, tag: 8 },
    palettes: {
      light: {
        bg: '#E1E6FA', card: '#FFFFFF', text: '#141A33', muted: '#4B536E', line: '#CBD3F2',
        action: '#2747D6', onAction: '#FFFFFF', actionBorder: 'transparent',
        link: '#2747D6', tagBg: '#FFDCCF', tagText: '#8A2E12', alert: '#B0301A', warn: '#875200', chip: '#E9EDFC', logoO: '#2747D6',
      },
      dark: {
        bg: '#0E1330', card: '#182046', text: '#E7EAFB', muted: '#A9B1D6', line: '#2A3368',
        action: '#8FA3FF', onAction: '#0E1330', actionBorder: 'transparent',
        link: '#8FA3FF', tagBg: '#45283F', tagText: '#FFB49C', alert: '#FF9C82', warn: '#F4BC5E', chip: '#222A58', logoO: '#8FA3FF',
      },
    },
  },
};

export const themeIds = Object.keys(themes) as ThemeId[];

/**
 * Teintes des vignettes de recettes (`vignetteOf`), communes aux quatre thèmes : des couleurs de cuisine, douces en
 * jour, sourdes en nuit. Le pictogramme se dessine dans la couleur du texte.
 */
export const vignettes: Record<Mode, Record<Hue, string>> = {
  light: { herb: '#D3E7B4', tomato: '#F6CDBD', saffron: '#F5DF9F', sea: '#C7DFEA', plum: '#E6D0E7', wheat: '#EBDCC2' },
  dark: { herb: '#2D3D1E', tomato: '#4A2A21', saffron: '#463818', sea: '#1D3440', plum: '#3A273F', wheat: '#3C3223' },
};
export const defaultTheme: ThemeId = 'chartreuse';

/** Espacements : 4, 8, 12, 16, 24, 32, 48. */
export const space = { 1: 4, 2: 8, 3: 12, 4: 16, 5: 24, 6: 32, 7: 48 } as const;

/** Hauteur minimale d'une cible tactile. */
export const touch = 52;

export const textSize = { small: 15, body: 18, button: 17, h2: 22, h1: 30 } as const;
