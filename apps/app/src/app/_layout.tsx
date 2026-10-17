import { Stack } from 'expo-router';
import { Platform } from 'react-native';

import { SessionProvider } from '../lib/session';
import { ThemeProvider, useTheme } from '../lib/theme';

function Screens() {
  const { palette } = useTheme();
  return (
    <Stack
      screenOptions={{
        headerShown: false,
        contentStyle: { backgroundColor: palette.bg },
        title: 'Chaudron',
      }}
    />
  );
}

/** Service worker : l'appli s'ouvre sans réseau. Seulement l'appli construite (pas sous `expo start`). */
if (Platform.OS === 'web' && process.env.NODE_ENV === 'production' && typeof navigator !== 'undefined' && 'serviceWorker' in navigator) {
  const register = () => void navigator.serviceWorker.register('/sw.js').catch(() => undefined);
  // Le code des écrans se charge souvent après l'événement `load` : on n'attend pas un événement passé.
  if (document.readyState === 'complete') register();
  else window.addEventListener('load', register);
}

export default function Layout() {
  return (
    <ThemeProvider>
      <SessionProvider>
        <Screens />
      </SessionProvider>
    </ThemeProvider>
  );
}
