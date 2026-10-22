import { errorMessage } from '@chaudron/api';
import { diffRecipes, space } from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, ErrorText, Loading, Pill, Row, Screen, Stack, Title } from '../../ui';
import { ChangeView } from '../../ui/changes';

/**
 * Une copie (`/recettes/originale?slug=…`) : ce qui a changé dans l'originale depuis la version copiée. On reprend
 * élément par élément (il remplace le sien), puis « Tout est vu ».
 */
export default function Upstream() {
  const { slug } = useLocalSearchParams<{ slug: string }>();
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (!home || !slug) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <Changes householdId={home.id} slug={slug} />;
}

function Changes({ householdId, slug }: { householdId: string; slug: string }) {
  const router = useRouter();
  const copy = useLoad(() => api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } }), [householdId, slug]);
  const from = copy.data?.copied_from;
  const version = (n: number) =>
    from
      ? api.GET('/api/v1/households/{id}/recipes/{slug}/versions/{number}', { params: { path: { id: householdId, slug: from.slug, number: n } } })
      : Promise.resolve({ data: undefined });
  const base = useLoad(() => version(from?.version ?? 0), [householdId, from?.slug, from?.version]);
  const current = useLoad(() => version(from?.current_version ?? 0), [householdId, from?.slug, from?.current_version]);
  const ingredients = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const [chosen, setChosen] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const take = async (keys: string[], seen: boolean) => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/upstream/take', {
      params: { path: { id: householdId, slug } },
      body: { keys, seen },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.replace(`/recettes/${slug}`);
  };

  if (!copy.data || (from && (!base.data || !current.data))) {
    return (
      <Screen>
        <Back href={`/recettes/${slug}`} label="Ta version" />
        {copy.error ?? base.error ?? current.error ? <ErrorText>{copy.error ?? base.error ?? current.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const names = Object.fromEntries((ingredients.data ?? []).map((i) => [i.code, i.name]));
  const changes = from && base.data && current.data ? diffRecipes(base.data.content, current.data.content, names).filter((c) => c.key) : [];
  const toggle = (key: string) => setChosen(chosen.includes(key) ? chosen.filter((k) => k !== key) : [...chosen, key]);

  return (
    <Screen>
      <Router.Screen options={{ title: `L’originale a changé · ${copy.data.title} · Chaudron` }} />
      <Back href={`/recettes/${slug}`} label={copy.data.title} />
      <Stack gap={space[1]}>
        <Title>L’originale a changé</Title>
        <Body tone="muted" size={16}>
          {from ? `« ${from.title} », de la version ${from.version} à la version ${from.current_version}. Ce que tu reprends remplace le tien ; le reste de ta version ne bouge pas.` : 'Ce n’est pas une copie.'}
        </Body>
      </Stack>
      <View style={{ gap: space[3], maxWidth: 820 }}>
        {changes.length === 0 ? <Body tone="muted">Rien qui touche les étapes ou les ingrédients.</Body> : null}
        {changes.map((c) => (
          <Card key={`${c.key}-${c.kind}`}>
            <ChangeView
              change={c}
              action={<Pill label={chosen.includes(c.key ?? '') ? 'Je reprends' : 'Reprendre'} selected={chosen.includes(c.key ?? '')} onPress={() => toggle(c.key ?? '')} />}
            />
          </Card>
        ))}
      </View>
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label={`Reprendre (${chosen.length})`} busy={busy} disabled={!chosen.length} onPress={() => take(chosen, true)} />
        <Button label="Tout est vu" busy={busy} onPress={() => take([], true)} />
      </Row>
    </Screen>
  );
}
