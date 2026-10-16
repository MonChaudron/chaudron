import type { RecipeSummary } from '@chaudron/api';
import { equipment as equipmentLabels, formatDuration, label, space } from '@chaudron/domain';
import { Redirect, Stack as Router, useRouter } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useLoad } from '../../lib/useLoad';
import { Body, Card, ErrorText, Field, LinkButton, Loading, Pill, Row, Screen, Stack, Tag, Title, useWide } from '../../ui';

/** Au plus 15 min actives : un soir express. */
const EXPRESS_MIN = 15;

export default function Recipes() {
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (me && !home) return <Redirect href="/foyer/nouveau" />;
  if (!home) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <Catalog householdId={home.id} owner={home.role === 'owner'} />;
}

function Catalog({ householdId, owner }: { householdId: string; owner: boolean }) {
  const wide = useWide();
  const [query, setQuery] = useState('');
  const [search, setSearch] = useState('');
  const [tag, setTag] = useState<string | null>(null);
  // La recherche part un quart de seconde après la dernière frappe.
  useEffect(() => {
    const timer = setTimeout(() => setSearch(query.trim()), 250);
    return () => clearTimeout(timer);
  }, [query]);

  const all = useLoad(
    () => api.GET('/api/v1/households/{id}/recipes', { params: { path: { id: householdId } } }),
    [householdId],
  );
  const filtered = useLoad(
    () =>
      api.GET('/api/v1/households/{id}/recipes', {
        params: { path: { id: householdId }, query: { q: search || undefined, tag: tag ?? undefined } },
      }),
    [householdId, search, tag],
  );
  const tags = useMemo(() => [...new Set((all.data ?? []).flatMap((r) => r.tags))].sort((a, b) => a.localeCompare(b, 'fr')), [all.data]);
  const list = filtered.data;
  const empty = all.data?.length === 0;

  return (
    <Screen>
      <Router.Screen options={{ title: 'Recettes · Chaudron' }} />
      <View style={wide ? { flexDirection: 'row', alignItems: 'flex-end', justifyContent: 'space-between', gap: space[4] } : { gap: space[3] }}>
        <Stack gap={space[2]}>
          <Title>Recettes</Title>
          {all.data?.length ? (
            <Body tone="muted" size={16}>
              {all.data.length} recette{all.data.length > 1 ? 's' : ''} du foyer, chacune avec ses façons de la cuisiner.
            </Body>
          ) : null}
        </Stack>
        {!empty ? <LinkButton label="Nouvelle recette" href="/recettes/modifier" /> : null}
      </View>

      {empty ? (
        <Card style={{ maxWidth: 640 }}>
          <Title level={2}>Pas encore de recette</Title>
          <Body tone="muted" size={16}>
            Écris la première, ou laisse un assistant comme Claude les publier par l’API, avec un jeton d’accès du
            foyer.
          </Body>
          <LinkButton kind="primary" label="Écrire une recette" href="/recettes/modifier" />
          {owner ? <LinkButton label="Créer un accès pour Claude" href="/foyer/acces" /> : null}
        </Card>
      ) : (
        <>
          <View style={wide ? { flexDirection: 'row', gap: space[4], alignItems: 'flex-end' } : { gap: space[3] }}>
            <View style={wide ? { width: 420 } : undefined}>
              <Field label="Chercher" placeholder="Un plat, un ingrédient : chili, lentilles…" value={query} onChangeText={setQuery} autoCorrect={false} />
            </View>
            {tags.length ? (
              <Row style={wide ? { flex: 1, paddingBottom: 4 } : undefined}>
                {tags.map((t) => (
                  <Pill key={t} label={t} selected={tag === t} onPress={() => setTag(tag === t ? null : t)} />
                ))}
              </Row>
            ) : null}
          </View>
          <ErrorText>{filtered.error ?? all.error}</ErrorText>
          {list === undefined ? <Loading /> : null}
          {list?.length === 0 ? (
            <Body tone="muted">Rien ne correspond à « {search || tag} ». Essaie un ingrédient, ou retire l’étiquette.</Body>
          ) : null}
          <View style={wide ? { flexDirection: 'row', flexWrap: 'wrap', gap: space[4] } : { gap: space[3] }}>
            {list?.map((recipe) => <RecipeTile key={recipe.slug} recipe={recipe} wide={wide} />)}
          </View>
        </>
      )}
    </Screen>
  );
}

function RecipeTile({ recipe, wide }: { recipe: RecipeSummary; wide: boolean }) {
  const router = useRouter();
  const { palette, radius, fonts, weights } = useTheme();
  const variant = recipe.default_variant;
  const express = variant.active_min <= EXPRESS_MIN;
  const tools = variant.equipment.map((code) => label(equipmentLabels, `equipment.${code}`)).join(', ');
  const others = recipe.variant_count - 1;
  return (
    <Pressable
      onPress={() => router.push(`/recettes/${recipe.slug}`)}
      accessibilityRole="link"
      accessibilityLabel={recipe.title}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
        flexGrow: 1,
        flexBasis: wide ? 300 : undefined,
        maxWidth: wide ? 420 : undefined,
        padding: space[5],
        gap: space[3],
        borderRadius: radius.card,
        borderWidth: 1.5,
        borderColor: hovered ? palette.muted : 'transparent',
        backgroundColor: palette.card,
        opacity: pressed ? 0.85 : 1,
      })}
    >
      <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 21, lineHeight: 27, color: palette.text }}>{recipe.title}</Text>
      {/* Le temps d'abord : c'est lui qui décide du soir. */}
      <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: space[2], flexWrap: 'wrap' }}>
        <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 28, color: palette.text }}>{formatDuration(variant.active_min)}</Text>
        <Body tone="muted" size={15}>
          actives, {formatDuration(variant.total_min)} en tout
        </Body>
      </View>
      <Body tone="muted" size={15}>
        {tools || variant.label}
        {others > 0 ? ` — ou ${others} autre${others > 1 ? 's' : ''} façon${others > 1 ? 's' : ''}` : ''}
      </Body>
      {express || recipe.tags.length ? (
        <Row>
          {express ? <Tag>Express</Tag> : null}
          {recipe.tags.map((t) => (
            <Text key={t} style={{ fontFamily: fonts.text, fontSize: 14, color: palette.muted }}>
              #{t}
            </Text>
          ))}
        </Row>
      ) : null}
    </Pressable>
  );
}
