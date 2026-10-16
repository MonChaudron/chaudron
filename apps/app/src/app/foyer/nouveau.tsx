import { errorMessage } from '@chaudron/api';
import { Redirect, Stack as Router, useRouter } from 'expo-router';
import { useState } from 'react';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { Body, Button, Card, ErrorText, Field, Screen, Title } from '../../ui';

export default function NewHousehold() {
  const { me, refresh } = useSession();
  const router = useRouter();
  const [name, setName] = useState('Maison');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (me === null) return <Redirect href="/connexion" />;

  const create = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households', { body: { name } });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    await refresh();
    router.replace('/foyer');
  };

  return (
    <Screen>
      <Router.Screen options={{ title: 'Créer le foyer · Chaudron' }} />
      <Card style={{ maxWidth: 640 }}>
        <Title>Bonjour {me?.display_name}</Title>
        <Body tone="muted">Crée ton foyer : ensuite, tu y ajouteras qui mange à la maison et ton matériel, et tu inviteras ceux qui partagent ta cuisine.</Body>
        <Field label="Nom du foyer" value={name} onChangeText={setName} maxLength={80} autoFocus onSubmitEditing={create} />
        <ErrorText>{error}</ErrorText>
        <Button kind="primary" label="Créer mon foyer" busy={busy} disabled={!name.trim()} onPress={create} />
      </Card>
    </Screen>
  );
}
