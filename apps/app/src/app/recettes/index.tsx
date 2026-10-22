import type { RecipeSummary } from '@chaudron/api';
import { equipment as equipmentLabels, formatDuration, label, space, vignetteOf } from '@chaudron/domain';
import { Redirect, Stack as Router, useRouter } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';
import { Pressable, ScrollView, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useLoad } from '../../lib/useLoad';
import { Body, Card, DishName, ErrorText, Field, LinkButton, Loading, Pill, Row, Screen, Stack, Tag, Title, useWide, Vignette } from '../../ui';

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
  const [scope, setScope] = useState<'household' | 'community' | null>(null);
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
        params: { path: { id: householdId }, query: { q: search || undefined, tag: tag ?? undefined, scope: scope ?? undefined } },
      }),
    [householdId, search, tag, scope],
  );
  const community = (all.data ?? []).filter((r) => r.community).length;
  const own = (all.data?.length ?? 0) - community;
  const tags = useMemo(() => [...new Set((all.data ?? []).flatMap((r) => r.tags))].sort((a, b) => a.localeCompare(b, 'fr')), [all.data]);
  // Une famille de déclinaisons tient en une tuile : la préférée du foyer, sinon la première ; « + 2 déclinaisons ».
  const list = useMemo(() => {
    const shown: (RecipeSummary & { others: number })[] = [];
    const seen = new Map<string, RecipeSummary & { others: number }>();
    for (const r of filtered.data ?? []) {
      if (!r.family_id) {
        shown.push({ ...r, others: 0 });
        continue;
      }
      const first = seen.get(r.family_id);
      if (!first) {
        const tile = { ...r, others: 0 };
        seen.set(r.family_id, tile);
        shown.push(tile);
      } else if (r.preferred && !first.preferred) {
        Object.assign(first, r, { others: first.others + 1 });
      } else {
        first.others += 1;
      }
    }
    return filtered.data ? shown : undefined;
  }, [filtered.data]);
  const empty = all.data?.length === 0;

  return (
    <Screen>
      <Router.Screen options={{ title: 'Recettes · Chaudron' }} />
      <View style={{ flexDirection: 'row', alignItems: wide ? 'flex-end' : 'flex-start', justifyContent: 'space-between', gap: space[4] }}>
        <Stack gap={space[2]} style={{ flex: 1 }}>
          <Title>Recettes</Title>
          {all.data?.length ? (
            <Body tone="muted" size={16}>
              {community
                ? `${own} recette${own > 1 ? 's' : ''} du foyer et ${community} de la communauté, chacune avec ses façons de la cuisiner.`
                : `${own} recette${own > 1 ? 's' : ''} du foyer, chacune avec ses façons de la cuisiner.`}
            </Body>
          ) : null}
        </Stack>
        {!empty ? <LinkButton label={wide ? 'Nouvelle recette' : 'Nouvelle'} href="/recettes/modifier" /> : null}
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
          <Stack gap={space[3]}>
            <View style={wide ? { maxWidth: 520 } : undefined}>
              <Field label="Chercher" hideLabel placeholder="Chercher un plat, un ingrédient : chili, lentilles…" value={query} onChangeText={setQuery} autoCorrect={false} />
            </View>
            {community ? (
              <View accessibilityRole="radiogroup" aria-label="Recettes à montrer" style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
                <Pill label="Toutes" role="radio" selected={scope === null} onPress={() => setScope(null)} />
                <Pill label="Du foyer" role="radio" selected={scope === 'household'} onPress={() => setScope('household')} />
                <Pill label="Communauté" role="radio" selected={scope === 'community'} onPress={() => setScope('community')} />
              </View>
            ) : null}
            {/* Les étiquettes sur une ligne qui défile : la première recette reste à l'écran. */}
            {tags.length ? (
              <ScrollView horizontal showsHorizontalScrollIndicator={false} style={{ marginHorizontal: wide ? 0 : -space[4] }} contentContainerStyle={{ gap: space[2], paddingHorizontal: wide ? 0 : space[4] }}>
                <Pill label="Toutes" role="radio" selected={tag === null} onPress={() => setTag(null)} />
                {tags.map((t) => (
                  <Pill key={t} role="radio" label={t} selected={tag === t} onPress={() => setTag(tag === t ? null : t)} />
                ))}
              </ScrollView>
            ) : null}
          </Stack>
          <ErrorText>{filtered.error ?? all.error}</ErrorText>
          {list === undefined ? <Loading /> : null}
          {list?.length === 0 ? (
            <Body tone="muted">Rien ne correspond à « {search || tag} ». Essaie un ingrédient, ou retire l’étiquette.</Body>
          ) : null}
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: wide ? space[4] : space[3] }}>
            {list?.map((recipe) => <RecipeTile key={recipe.slug} recipe={recipe} wide={wide} />)}
          </View>
        </>
      )}
    </Screen>
  );
}

/**
 * Une recette de la liste : sa vignette, son nom, le temps actif et l'appareil. Deux par ligne sur téléphone, en
 * grille sur bureau. Les étiquettes restent dans les filtres.
 */
function RecipeTile({ recipe, wide }: { recipe: RecipeSummary & { others: number }; wide: boolean }) {
  const router = useRouter();
  const { palette, radius, fonts } = useTheme();
  const variant = recipe.default_variant;
  const express = variant.active_min <= EXPRESS_MIN;
  const tool = variant.equipment.map((code) => label(equipmentLabels, `equipment.${code}`))[0] ?? variant.label;
  const others = recipe.variant_count - 1;
  const look = vignetteOf(recipe.slug, variant.equipment);
  return (
    <Pressable
      onPress={() => router.push(`/recettes/${recipe.slug}`)}
      accessibilityRole="link"
      accessibilityLabel={recipe.title}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) =>
        ({
          width: wide ? undefined : `calc(50% - ${space[3] / 2}px)`,
          flexGrow: wide ? 1 : 0,
          flexBasis: wide ? 220 : undefined,
          maxWidth: wide ? 300 : undefined,
          padding: space[2],
          paddingBottom: space[3],
          gap: space[3],
          borderRadius: radius.card,
          backgroundColor: palette.card,
          borderWidth: 1.5,
          borderColor: hovered ? palette.line : 'transparent',
          opacity: pressed ? 0.85 : 1,
        }) as never
      }
    >
      <Vignette hue={look.hue} pictogram={look.pictogram} height={wide ? 112 : 92} />
      <View style={{ gap: space[2], paddingHorizontal: space[2], flex: 1 }}>
        <DishName title={recipe.title} size={16} lines={3} />
        <Text style={{ fontFamily: fonts.text, fontSize: 14, color: palette.muted }}>
          {`${formatDuration(variant.active_min)} · ${tool}${others > 0 ? ` · +${others}` : ''}`}
        </Text>
        <Row gap={space[1]}>
          {express ? <Tag tone="strong" icon="bolt">Express</Tag> : null}
          {recipe.community ? <Tag icon="people">Communauté</Tag> : null}
          {recipe.others ? <Tag>{`+ ${recipe.others} déclinaison${recipe.others > 1 ? 's' : ''}`}</Tag> : null}
        </Row>
      </View>
    </Pressable>
  );
}
