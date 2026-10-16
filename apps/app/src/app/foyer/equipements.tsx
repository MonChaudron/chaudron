import { errorMessage } from '@chaudron/api';
import { equipment as equipmentLabels, label, space } from '@chaudron/domain';
import { Stack as Router, useRouter } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, ErrorText, Loading, Pill, Row, Screen, Title, useWide } from '../../ui';

/** Le matériel du foyer : on coche, puis on valide. */
export default function Equipment() {
  const { me } = useSession();
  const router = useRouter();
  const wide = useWide();
  const householdId = me?.households[0]?.id;
  const catalog = useLoad(() => api.GET('/api/v1/equipment'), []);
  const owned = useLoad(
    () =>
      householdId
        ? api.GET('/api/v1/households/{id}/equipment', { params: { path: { id: householdId } } })
        : Promise.resolve({ data: undefined }),
    [householdId],
  );
  const [draft, setDraft] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const saved = owned.data?.codes ?? [];
  const codes = draft ?? saved;
  const changed = draft !== null && [...draft].sort().join() !== [...saved].sort().join();

  const toggle = (code: string) =>
    setDraft(codes.includes(code) ? codes.filter((c) => c !== code) : [...codes, code]);

  const save = async () => {
    if (!householdId) return;
    setBusy(true);
    const { data, error: failure } = await api.PUT('/api/v1/households/{id}/equipment', {
      params: { path: { id: householdId } },
      body: { codes },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.replace('/foyer');
  };

  return (
    <Screen>
      <Router.Screen options={{ title: 'Le matériel · Chaudron' }} />
      <Back href="/foyer" label="Le foyer" />
      <Title>Le matériel</Title>
      <Card style={{ maxWidth: 760 }}>
        <Body tone="muted" size={16}>Ce que tu as en cuisine. Les recettes proposeront la variante qui va avec.</Body>
        {!catalog.data || !owned.data ? (
          <Loading />
        ) : (
          <Row>
            {catalog.data.map((item) => (
              <Pill
                key={item.code}
                label={label(equipmentLabels, item.name_key)}
                selected={codes.includes(item.code)}
                onPress={() => toggle(item.code)}
              />
            ))}
          </Row>
        )}
      </Card>
      <View style={wide ? { alignSelf: 'flex-start', width: 320, gap: space[3] } : { gap: space[3] }}>
        <ErrorText>{error}</ErrorText>
        <Button kind="primary" label="Valider" busy={busy} disabled={!changed} onPress={save} />
      </View>
    </Screen>
  );
}
