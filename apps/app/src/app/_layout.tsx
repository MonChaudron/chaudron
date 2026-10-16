import { Stack } from 'expo-router';

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

export default function Layout() {
  return (
    <ThemeProvider>
      <SessionProvider>
        <Screens />
      </SessionProvider>
    </ThemeProvider>
  );
}
