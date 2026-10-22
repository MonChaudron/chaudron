import { errorMessage } from '@chaudron/api';
import { space } from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, ErrorText, Field, Loading, Row, Screen, Stack, Title } from '../../ui';
import { RecipePreview } from '../../ui/preview';

/** Une recette proposée : la lire, l'accepter (avec un autre slug, ou pour la modifier ensuite) ou la refuser. */
export default function SubmissionScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { me } = useSession();
  const router = useRouter();
  const detail = useLoad(
    () => (id ? api.GET('/api/v1/community/submissions/{submission_id}', { params: { path: { submission_id: id } } }) : Promise.resolve({ data: undefined })),
    [id],
  );
  const [slug, setSlug] = useState<string | null>(null);
  const [word, setWord] = useState('');
  const [refusing, setRefusing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (me === null) return <Redirect href="/connexion" />;
  if (me && !me.instance_admin) return <Redirect href="/" />;
  const data = detail.data;
  if (!data || !id) {
    return (
      <Screen>
        <Back href="/revoir" label="À revoir" />
        {detail.error ? <ErrorText>{detail.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const { submission: s, recipe } = data;
  const target = slug ?? recipe.slug;

  const accept = async (edit: boolean) => {
    setBusy(true);
    const { data: accepted, error: failure } = await api.POST('/api/v1/community/submissions/{submission_id}/accept', {
      params: { path: { submission_id: id } },
      body: { slug: target, message: word.trim() || null },
    });
    setBusy(false);
    if (!accepted) return setError(errorMessage(failure));
    router.replace(edit ? `/recettes/modifier?slug=${accepted.slug}` : '/revoir');
  };
  const refuse = async () => {
    setBusy(true);
    const { response, error: failure } = await api.POST('/api/v1/community/submissions/{submission_id}/refuse', {
      params: { path: { submission_id: id } },
      body: { message: word },
    });
    setBusy(false);
    if (!response.ok) return setError(errorMessage(failure));
    router.replace('/revoir');
  };

  return (
    <Screen>
      <Router.Screen options={{ title: `${recipe.title} · À revoir · Chaudron` }} />
      <Back href="/revoir" label="À revoir" />
      <Stack gap={space[1]}>
        <Title>{recipe.title}</Title>
        <Body tone="muted" size={16}>{`Proposée par ${s.author_name} (${s.household_name ?? 'un foyer'})`}</Body>
        {s.message ? <Body size={16}>« {s.message} »</Body> : null}
        {recipe.description ? <Body tone="muted" size={16}>{recipe.description}</Body> : null}
      </Stack>
      <RecipePreview recipe={recipe} />
      <Card style={{ maxWidth: 640 }}>
        <Title level={2}>Ta réponse</Title>
        <Field
          label="Adresse dans la communauté"
          hint="Unique sur l’instance : si un foyer a déjà ce slug, choisis-en un autre."
          value={target}
          onChangeText={setSlug}
          autoCapitalize="none"
          autoCorrect={false}
        />
        <Field
          label={refusing ? 'Un mot pour le foyer (obligatoire)' : 'Un mot pour le foyer'}
          value={word}
          onChangeText={setWord}
          multiline
          maxLength={2000}
          style={{ minHeight: 72, paddingTop: 12, textAlignVertical: 'top' }}
        />
        <ErrorText>{error}</ErrorText>
        {refusing ? (
          <Row>
            <Button kind="danger" label="Refuser" busy={busy} disabled={!word.trim()} onPress={refuse} />
            <Button kind="quiet" label="Annuler" onPress={() => setRefusing(false)} />
          </Row>
        ) : (
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
            <Button kind="primary" label="Accepter" busy={busy} onPress={() => accept(false)} />
            <Button label="Accepter et modifier" busy={busy} onPress={() => accept(true)} />
            <Button kind="quiet" label="Refuser…" onPress={() => setRefusing(true)} />
          </View>
        )}
      </Card>
    </Screen>
  );
}
