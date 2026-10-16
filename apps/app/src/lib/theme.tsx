import { defaultTheme, type Mode, type Palette, type Theme, type ThemeId, themes } from '@chaudron/domain';
import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';
import { Platform, useColorScheme } from 'react-native';

import { readSetting, writeSetting } from './storage';

/** `auto` : comme le téléphone. */
export type ThemePreference = 'auto' | Mode;

interface ThemeContext {
  theme: Theme;
  mode: Mode;
  palette: Palette;
  fonts: Theme['fonts'];
  weights: Theme['weights'];
  radius: Theme['radius'];
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
  setTheme: (id: ThemeId) => void;
}

const Context = createContext<ThemeContext | null>(null);
const MODE_KEY = 'chaudron.theme';
const THEME_KEY = 'chaudron.charte';
/** Couleur de fond appliquée par index.html avant le chargement de l'appli (pas de flash). */
const BG_KEY = 'chaudron.bg';

function storedPreference(): ThemePreference {
  const value = readSetting(MODE_KEY);
  return value === 'light' || value === 'dark' ? value : 'auto';
}

function storedTheme(): ThemeId {
  const value = readSetting(THEME_KEY);
  return value && value in themes ? (value as ThemeId) : defaultTheme;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const system = useColorScheme();
  const [preference, setPreference] = useState<ThemePreference>(storedPreference);
  const [themeId, setTheme] = useState<ThemeId>(storedTheme);
  const mode: Mode = preference === 'auto' ? (system === 'dark' ? 'dark' : 'light') : preference;
  const theme = themes[themeId];
  const palette = theme.palettes[mode];

  useEffect(() => {
    writeSetting(MODE_KEY, preference === 'auto' ? null : preference);
  }, [preference]);

  useEffect(() => {
    writeSetting(THEME_KEY, themeId === defaultTheme ? null : themeId);
  }, [themeId]);

  // Web : fond de page et barre du navigateur aux couleurs du thème.
  useEffect(() => {
    writeSetting(BG_KEY, palette.bg);
    if (Platform.OS !== 'web') return;
    document.documentElement.style.colorScheme = mode;
    document.body.style.backgroundColor = palette.bg;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', palette.bg);
  }, [mode, palette.bg]);

  const value = useMemo(
    () => ({
      theme,
      mode,
      palette,
      fonts: theme.fonts,
      weights: theme.weights,
      radius: theme.radius,
      preference,
      setPreference,
      setTheme,
    }),
    [theme, mode, palette, preference],
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useTheme(): ThemeContext {
  const context = useContext(Context);
  if (!context) throw new Error('useTheme hors de ThemeProvider');
  return context;
}
