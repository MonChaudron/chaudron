import type { VersionInfo } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { diffRecipes, diffWords, fieldLabels, formatDuration, type RecipeChange, space } from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, Columns, ErrorText, Loading, Row, Screen, Stack, Tag, Title } from '../../ui';

/** Historique d'une recette (`/recettes/historique?slug=…`) : les versions, ce qui a changé, revenir à une version. */
export default function History() {
  const { slug } = useLocalSearchParams<{ slug: string }>();
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (!home || !slug || !me) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <Versions householdId={home.id} slug={slug} admin={me.instance_admin} />;
}

const when = (iso: string) =>
  new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(iso));

function Versions({ householdId, slug, admin }: { householdId: string; slug: string; admin: boolean }) {
  const router = useRouter();
  const path = { id: householdId, slug };
  const recipe = useLoad(() => api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path } }), [householdId, slug]);
  const versions = useLoad(() => api.GET('/api/v1/households/{id}/recipes/{slug}/versions', { params: { path } }), [householdId, slug]);
  const [chosen, setChosen] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const list = versions.data;
  const number = chosen ?? list?.[0]?.number ?? null;
  const current = recipe.data?.version;
  const editable = recipe.data ? !recipe.data.community || admin : false;

  const restore = async () => {
    if (number === null) return;
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/versions/{number}/restore', {
      params: { path: { ...path, number } },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.replace(`/recettes/${slug}`);
  };

  return (
    <Screen>
      <Router.Screen options={{ title: `Historique · ${recipe.data?.title ?? slug} · Chaudron` }} />
      <Back href={`/recettes/${slug}`} label={recipe.data?.title ?? 'La recette'} />
      <Stack gap={space[1]}>
        <Title>Historique</Title>
        <Body tone="muted" size={16}>
          Chaque enregistrement garde la recette entière. Revenir à une version la republie : rien ne se perd.
        </Body>
      </Stack>
      <ErrorText>{versions.error ?? recipe.error}</ErrorText>
      {!list ? <Loading /> : null}
      {list ? (
        <Columns weights={[2, 3]}>
          <Card>
            <View>
              {list.map((v, i) => (
                <VersionRow key={v.number} version={v} current={v.number === current} selected={v.number === number} last={i === list.length - 1} onPress={() => setChosen(v.number)} />
              ))}
            </View>
          </Card>
          <Stack gap={space[4]}>
            {number !== null ? <Changes householdId={householdId} slug={slug} number={number} first={number === list[list.length - 1]?.number} /> : null}
            {number !== null && number !== current && editable ? (
              <View style={{ gap: space[2], maxWidth: 480 }}>
                <ErrorText>{error}</ErrorText>
                <Button kind="primary" label={`Revenir à la version ${number}`} busy={busy} onPress={restore} />
              </View>
            ) : null}
          </Stack>
        </Columns>
      ) : null}
    </Screen>
  );
}

function VersionRow({ version, current, selected, last, onPress }: { version: VersionInfo; current: boolean; selected: boolean; last: boolean; onPress: () => void }) {
  const { palette, fonts, weights, radius } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      accessibilityState={{ selected }}
      style={{
        paddingVertical: space[3],
        paddingHorizontal: space[3],
        marginHorizontal: -space[3],
        gap: 2,
        borderRadius: radius.button,
        backgroundColor: selected ? palette.chip : 'transparent',
        borderBottomWidth: last || selected ? 0 : 1,
        borderBottomColor: palette.line,
      }}
    >
      <Row gap={space[2]}>
        <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 17, color: palette.text }}>Version {version.number}</Text>
        {current ? <Tag tone="strong">en cours</Tag> : null}
      </Row>
      <Body tone="muted" size={15}>{`${version.author_name} · ${when(version.created_at)}`}</Body>
      {version.message ? <Body size={15}>« {version.message} »</Body> : null}
    </Pressable>
  );
}

/** Ce que la version a changé par rapport à la précédente. */
function Changes({ householdId, slug, number, first }: { householdId: string; slug: string; number: number; first: boolean }) {
  const load = (n: number) => api.GET('/api/v1/households/{id}/recipes/{slug}/versions/{number}', { params: { path: { id: householdId, slug, number: n } } });
  const version = useLoad(() => load(number), [householdId, slug, number]);
  const previous = useLoad(() => (first ? Promise.resolve({ data: null }) : load(number - 1)), [householdId, slug, number, first]);
  const ingredients = useLoad(() => api.GET('/api/v1/ingredients'), []);
  if (version.data === undefined || previous.data === undefined) return <Loading />;
  if (first || !previous.data) {
    return (
      <Card>
        <Title level={2}>Version {number}</Title>
        <Body tone="muted" size={16}>La première version gardée de la recette.</Body>
      </Card>
    );
  }
  const names = Object.fromEntries((ingredients.data ?? []).map((i) => [i.code, i.name]));
  const changes = diffRecipes(previous.data.content, version.data.content, names);
  return (
    <Card>
      <Title level={2}>{`Ce que la version ${number} a changé`}</Title>
      {changes.length === 0 ? <Body tone="muted" size={16}>Rien de visible : la recette a été republiée telle quelle.</Body> : null}
      <View style={{ gap: space[4] }}>
        {changes.map((change, i) => (
          <ChangeView key={`${change.key ?? change.where}-${i}`} change={change} />
        ))}
      </View>
    </Card>
  );
}

const kindLabels: Record<RecipeChange['kind'], string> = {
  added: 'Ajouté',
  removed: 'Retiré',
  changed: 'Modifié',
  moved: 'Déplacé',
};

const TEXT_FIELDS = new Set(['title', 'description', 'text', 'spoken_text', 'note', 'label', 'source']);

function show(value: unknown, field: string): string {
  if (value === null || value === undefined || value === '') return '—';
  if (field === 'timer_s' && typeof value === 'number') return formatDuration(Math.max(1, Math.round(value / 60)));
  if (Array.isArray(value)) return value.length ? value.join(', ') : '—';
  if (typeof value === 'boolean') return value ? 'oui' : 'non';
  return String(value);
}

function ChangeView({ change }: { change: RecipeChange }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <View style={{ gap: space[2] }}>
      <Row gap={space[2]}>
        <Tag tone={change.kind === 'removed' ? 'warn' : 'neutral'}>{kindLabels[change.kind]}</Tag>
        <Text style={{ flexShrink: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, color: palette.text }}>{change.where}</Text>
      </Row>
      {change.fields.map((f) => (
        <View key={f.field} style={{ gap: 2, paddingLeft: space[3], borderLeftWidth: 2, borderLeftColor: palette.line }}>
          <Body tone="muted" size={14}>{fieldLabels[f.field] ?? f.field}</Body>
          {TEXT_FIELDS.has(f.field) && typeof (f.before ?? '') === 'string' && typeof (f.after ?? '') === 'string' ? (
            <Text style={{ fontFamily: fonts.text, fontSize: 16, lineHeight: 24, color: palette.text }}>
              {diffWords(String(f.before ?? ''), String(f.after ?? '')).map((piece, j) => (
                <Text
                  key={j}
                  style={
                    piece.kind === 'removed'
                      ? { color: palette.muted, textDecorationLine: 'line-through' }
                      : piece.kind === 'added'
                        ? { backgroundColor: palette.chip, fontWeight: weights.strong }
                        : undefined
                  }
                >
                  {piece.text}
                </Text>
              ))}
            </Text>
          ) : (
            <Body size={16}>{`${show(f.before, f.field)} → ${show(f.after, f.field)}`}</Body>
          )}
        </View>
      ))}
    </View>
  );
}
