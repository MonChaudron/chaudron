import { errorMessage } from '@chaudron/api';
import { space } from '@chaudron/domain';
import { Redirect, Stack as Router, useRouter } from 'expo-router';
import { useEffect, useState } from 'react';

import { api, loginUrl } from '../lib/api';
import { useSession } from '../lib/session';
import { ThemePicker } from '../lib/ThemePicker';
import { type ThemePreference, useTheme } from '../lib/theme';
import { Back, Body, Button, Card, Choice, Columns, ErrorText, Field, ListRow, Loading, Screen, Stack, Title } from '../ui';

const providerNames: Record<string, string> = { google: 'Google', amazon: 'Amazon', fake: 'Connexion factice' };

export default function Compte() {
  const { me, refresh, logout } = useSession();
  const router = useRouter();
  const { preference, setPreference } = useTheme();
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const available = useProvidersForLinking();

  if (me === null) return <Redirect href="/connexion" />;
  if (!me) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }

  const name = draft ?? me.display_name;
  const linked = new Set<string>(me.identities.map((i) => i.provider));
  const saveName = async () => {
    setBusy(true);
    const { data, error: failure } = await api.PATCH('/api/v1/me', { body: { display_name: name } });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setError(null);
    await refresh();
    setDraft(null);
  };

  return (
    <Screen>
      <Router.Screen options={{ title: 'Compte · Chaudron' }} />
      <Back href="/foyer" label="Le foyer" />
      <Title>Ton compte</Title>

      <Columns>
        <>
          <Card>
            <Field label="Ton nom" hint="Celui que voient les autres comptes du foyer." value={name} onChangeText={setDraft} maxLength={80} />
            <ErrorText>{error}</ErrorText>
            {name.trim() !== me.display_name ? <Button kind="primary" label="Enregistrer" busy={busy} disabled={!name.trim()} onPress={saveName} /> : null}
          </Card>

          <Card>
            <Title level={2}>Connexion</Title>
            <Stack gap={0}>
              {me.identities.map((identity, index) => (
                <ListRow
                  key={`${identity.provider}-${identity.email}`}
                  title={providerNames[identity.provider] ?? identity.provider}
                  detail={identity.email ?? undefined}
                  last={index === me.identities.length - 1}
                />
              ))}
            </Stack>
            {available
              .filter((p) => !linked.has(p))
              .map((p) => (
                <Button
                  key={p}
                  label={`Lier mon compte ${providerNames[p] ?? p}`}
                  onPress={() => globalThis.location?.assign(loginUrl(p, { returnTo: '/compte' }))}
                />
              ))}
          </Card>
        </>
        <Card>
          <Title level={2}>Affichage</Title>
          <ThemePicker />
          <Choice<ThemePreference>
            label="Jour ou nuit"
            options={[
              { value: 'auto', label: 'Comme le téléphone' },
              { value: 'light', label: 'Jour' },
              { value: 'dark', label: 'Nuit' },
            ]}
            value={preference}
            onChange={setPreference}
          />
          <Body tone="muted" size={15}>Thème et mode nuit : réglages propres à cet appareil.</Body>
        </Card>
      </Columns>

      <Stack gap={space[2]}>
        <Button
          kind="quiet"
          label="Se déconnecter"
          onPress={async () => {
            await logout();
            router.replace('/connexion');
          }}
        />
      </Stack>
    </Screen>
  );
}

/** Fournisseurs de l'instance (la session ne les charge que déconnecté). */
function useProvidersForLinking(): string[] {
  const [providers, setProviders] = useState<string[]>([]);
  useEffect(() => {
    void api.GET('/api/v1/auth/providers').then(({ data }) => setProviders((data ?? []).map((p) => p.id)));
  }, []);
  return providers;
}
