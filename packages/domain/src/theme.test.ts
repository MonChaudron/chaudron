import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type Palette, themeIds, themes, vignettes } from './theme.ts';

// Contraste WCAG 2 entre deux couleurs #RRGGBB.
function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

/** Texte lisible (4,5:1) : chaque couleur de texte sur chaque fond où elle apparaît. */
const textPairs: [keyof Palette, keyof Palette][] = [
  ['text', 'bg'],
  ['text', 'card'],
  ['muted', 'bg'],
  ['muted', 'card'],
  ['link', 'bg'],
  ['link', 'card'],
  ['alert', 'bg'],
  ['alert', 'card'],
  ['warn', 'bg'],
  ['warn', 'card'],
  ['text', 'chip'],
  ['muted', 'chip'],
  ['warn', 'chip'],
  ['onAction', 'action'],
  ['tagText', 'tagBg'],
];

test('contrastes des textes dans les huit palettes', () => {
  for (const id of themeIds) {
    for (const [mode, palette] of Object.entries(themes[id].palettes)) {
      for (const [fg, bg] of textPairs) {
        const ratio = contrast(palette[fg], palette[bg]);
        assert.ok(ratio >= 4.5, `${id} ${mode} : ${fg} sur ${bg} à ${ratio.toFixed(2)}:1`);
      }
    }
  }
});

test('le bouton principal se détache du fond (3:1), sinon il est cerné', () => {
  for (const id of themeIds) {
    for (const [mode, palette] of Object.entries(themes[id].palettes)) {
      const ratio = contrast(palette.action, palette.bg);
      const outlined = palette.actionBorder !== 'transparent' && contrast(palette.actionBorder, palette.bg) >= 3;
      assert.ok(ratio >= 3 || outlined, `${id} ${mode} : bouton à ${ratio.toFixed(2)}:1, sans contour`);
    }
  }
});

test('le pictogramme d’une vignette se lit sur sa teinte (3:1)', () => {
  for (const id of themeIds) {
    for (const [mode, palette] of Object.entries(themes[id].palettes)) {
      for (const [hue, color] of Object.entries(vignettes[mode as keyof typeof vignettes])) {
        const ratio = contrast(palette.text, color);
        assert.ok(ratio >= 3, `${id} ${mode} : texte sur ${hue} à ${ratio.toFixed(2)}:1`);
      }
    }
  }
});

test('le texte du bouton principal se lit aussi sur le bandeau « ce soir » (fond action)', () => {
  for (const id of themeIds) {
    for (const [mode, palette] of Object.entries(themes[id].palettes)) {
      const ratio = contrast(palette.onAction, palette.action);
      assert.ok(ratio >= 4.5, `${id} ${mode} : ${ratio.toFixed(2)}:1`);
    }
  }
});
