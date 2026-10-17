import type { Member, MenuSlot, Recipe, RecipeLine, Variant, Warning } from '@chaudron/api';
import {
  dayLabel,
  type Eater,
  equipment as equipmentLabels,
  formatDuration,
  formatQuantity,
  ingredientStates,
  label,
  scaleLine,
  scaleWith,
  seasonText,
  servingsText,
  slotEaters,
  space,
  todayIso,
  variantDelta,
} from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useCached } from '../../lib/useCached';
import { Back, Body, Card, Columns, ErrorText, LinkButton, Loading, Pill, Row, Screen, Stack, Tag, Title } from '../../ui';
import { mealLabels, SlotBadges, Warnings, warningsOf } from '../../ui/menu';

export default function RecipeScreen() {
  const { slug, slot, menu } = useLocalSearchParams<{ slug: string; slot?: string; menu?: string }>();
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
  return <RecipeView householdId={home.id} slug={slug} slotId={slot} weekStart={menu} />;
}

/** Ouverte depuis un créneau (`?slot=…&menu=…`) : la variante prévue, les quantités et les avertissements du créneau. */
function RecipeView({ householdId, slug, slotId, weekStart }: { householdId: string; slug: string; slotId?: string; weekStart?: string }) {
  const recipe = useCached(`recipe:${householdId}:${slug}`, () =>
    api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } }),
  );
  const members = useCached(`members:${householdId}`, () => api.GET('/api/v1/households/{id}/members', { params: { path: { id: householdId } } }));
  const planned = useCached(`menu:${householdId}:${weekStart ?? ''}`, async () =>
    slotId && weekStart
      ? api.GET('/api/v1/households/{id}/menus/{week_start}', { params: { path: { id: householdId, week_start: weekStart } } })
      : { data: null },
  );
  const [chosen, setChosen] = useState<string | null>(null);
  const slot = planned.data?.slots.find((s) => s.id === slotId) ?? null;
  const slotWarnings = slot && planned.data ? warningsOf(planned.data.warnings, slot.id) : [];
  const back = slotId ? <Back href="/" label="Cette semaine" /> : <Back href="/recettes" label="Recettes" />;

  const data = recipe.data;
  if (!data) {
    return (
      <Screen>
        {back}
        {recipe.error ? <ErrorText>{recipe.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const base = data.variants[0];
  // Sans choix, la première variante que le foyer peut faire avec son matériel.
  const doable = data.variants.find((v) => v.missing_equipment.length === 0) ?? base;
  const variant = data.variants.find((v) => v.id === chosen) ?? data.variants.find((v) => v.label === slot?.variant.label) ?? doable;

  return (
    <Screen>
      <Router.Screen options={{ title: `${data.title} · Chaudron` }} />
      <Row style={{ justifyContent: 'space-between' }}>
        {back}
        <LinkButton kind="quiet" label="Modifier" href={`/recettes/modifier?slug=${data.slug}`} />
      </Row>
      <Stack gap={space[3]}>
        <Title>{data.title}</Title>
        {data.description ? <Body tone="muted" style={{ maxWidth: 720 }}>{data.description}</Body> : null}
        <Meta recipe={data} />
      </Stack>
      {slot ? <Planned slot={slot} warnings={slotWarnings} /> : null}

      <Variants recipe={data} variant={variant} base={base} onChoose={setChosen} />

      <Columns weights={[2, 3]}>
        <Ingredients lines={data.ingredients} members={members.data} slot={slot} />
        <Steps variant={variant} />
      </Columns>
    </Screen>
  );
}

function Meta({ recipe }: { recipe: Recipe }) {
  const season = seasonText(recipe.season_months);
  if (!recipe.tags.length && !season && !recipe.source) return null;
  return (
    <Row gap={space[3]}>
      {recipe.tags.map((t) => (
        <Tag key={t}>{t}</Tag>
      ))}
      {season ? <Body tone="muted" size={15}>De saison {season}</Body> : null}
      {recipe.source ? <Body tone="muted" size={15}>{recipe.source}</Body> : null}
    </Row>
  );
}

/** Les façons de cuisiner : on bascule, l'écart avec la variante par défaut s'affiche tout de suite. */
function Variants({ recipe, variant, base, onChoose }: { recipe: Recipe; variant: Variant; base: Variant; onChoose: (id: string) => void }) {
  const { palette, fonts, weights } = useTheme();
  const tools = variant.equipment.map((code) => label(equipmentLabels, `equipment.${code}`));
  const missing = variant.missing_equipment.map((code) => label(equipmentLabels, `equipment.${code}`));
  return (
    <Card>
      {recipe.variants.length > 1 ? (
        <View accessibilityRole="radiogroup" aria-label="Façon de cuisiner" style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
          {recipe.variants.map((v) => (
            <Pill
              key={v.id}
              role="radio"
              label={v.label}
              selected={v.id === variant.id}
              onPress={() => onChoose(v.id)}
            />
          ))}
        </View>
      ) : null}
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', alignItems: 'baseline', columnGap: space[5], rowGap: space[2] }}>
        <Figure value={formatDuration(variant.active_min)} caption="actives" />
        <Figure value={formatDuration(variant.total_min)} caption="en tout" />
        <Figure value={String(variant.vessels)} caption={variant.vessels > 1 ? 'récipients' : 'récipient'} />
        {variant.id !== base.id ? (
          <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: palette.link }}>
            {variantDelta(base, variant)} par rapport à « {base.label} »
          </Text>
        ) : null}
      </View>
      {tools.length ? <Body tone="muted" size={16}>Avec : {tools.join(', ')}</Body> : null}
      {missing.length ? (
        <Body tone="alert" size={16}>
          Il te manque : {missing.join(', ')}.{recipe.variants.length > 1 ? ' Choisis une autre façon de la cuisiner.' : ''}
        </Body>
      ) : null}
    </Card>
  );
}

function Figure({ value, caption }: { value: string; caption: string }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: space[1] }}>
      <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 30, color: palette.text }}>{value}</Text>
      <Body tone="muted" size={15}>{caption}</Body>
    </View>
  );
}

/** Le repas prévu : jour, portions, avertissements. */
function Planned({ slot, warnings }: { slot: MenuSlot; warnings: Warning[] }) {
  return (
    <Card>
      <Title level={2}>{`${dayLabel(slot.date, todayIso())} ${mealLabels[slot.meal]}`}</Title>
      <SlotBadges slot={slot} />
      {slot.note ? <Body size={16}>{slot.note}</Body> : null}
      <Warnings warnings={warnings} />
    </Card>
  );
}

/**
 * Quantités pour tout le foyer (barème de chacun), ou pour le créneau ouvert depuis un menu ; détail par membre à la
 * demande.
 */
function Ingredients({ lines, members, slot }: { lines: RecipeLine[]; members?: Member[]; slot: MenuSlot | null }) {
  const [detail, setDetail] = useState(false);
  const eaters: Eater[] = slot ? slotEaters(slot) : (members ?? []).map((m) => ({ name: m.name, portions: m.portions }));
  const who = slot
    ? `Pour ${servingsText(slot)} portions${eaters.length ? ` : ${eaters.map((e) => e.name).join(', ')}` : ''}.`
    : eaters.length
      ? `Pour le foyer : ${eaters.map((e) => e.name).join(', ')}.`
      : 'Pour une portion standard : ajoute les membres du foyer pour avoir les quantités de chacun.';
  return (
    <Card>
      <Title level={2}>Ingrédients</Title>
      <Body tone="muted" size={15}>{who}</Body>
      <View>
        {lines.map((line, i) => (
          <IngredientRow key={`${line.ingredient}-${i}`} line={line} eaters={eaters} slot={slot} detail={detail} last={i === lines.length - 1} />
        ))}
      </View>
      {eaters.length > 1 ? (
        <Pressable onPress={() => setDetail(!detail)} accessibilityRole="button" style={{ alignSelf: 'flex-start', paddingVertical: space[2] }}>
          <Body size={16} style={{ textDecorationLine: 'underline' }}>{detail ? 'Masquer le détail par membre' : 'Voir le détail par membre'}</Body>
        </Pressable>
      ) : null}
    </Card>
  );
}

function IngredientRow({ line, eaters, slot, detail, last }: { line: RecipeLine; eaters: Eater[]; slot: MenuSlot | null; detail: boolean; last: boolean }) {
  const { palette, fonts, weights } = useTheme();
  // Pour un créneau, le total vient de son multiplicateur (qui tient compte d'un nombre fixé) ; le détail, des mangeurs.
  const byEater = scaleLine(line, eaters);
  const scaled = slot ? { total: scaleWith(line, slot.scale).total, perEater: byEater.perEater } : byEater;
  // La forme d'achat seulement quand elle change les courses : sec va de soi (riz, épices).
  const shown = line.state === 'frozen' || line.state === 'canned' || line.state === 'cooked';
  const notes = [shown ? ingredientStates[line.state].toLowerCase() : null, line.optional ? 'facultatif' : null, line.note]
    .filter(Boolean)
    .join(' ; ');
  return (
    <View style={{ paddingVertical: space[3], gap: 2, borderBottomWidth: last ? 0 : 1, borderBottomColor: palette.line }}>
      <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: space[3] }}>
        <Text style={{ flex: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: palette.text }}>{line.name}</Text>
        <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 18, color: line.qty == null ? palette.muted : palette.text, fontVariant: ['tabular-nums'] }}>
          {formatQuantity(scaled.total, line.unit)}
        </Text>
      </View>
      {notes ? <Body tone="muted" size={14}>{notes}</Body> : null}
      {detail && scaled.perEater.length ? (
        <Body tone="muted" size={14}>
          {scaled.perEater.map((e) => `${e.name} ${formatQuantity(e.qty, line.unit)}`).join(', ')}
        </Body>
      ) : null}
    </View>
  );
}

/** Le déroulé : les phases du mode rapide, numérotées, et leurs gestes à déplier. */
function Steps({ variant }: { variant: Variant }) {
  const { palette, fonts, weights } = useTheme();
  const [open, setOpen] = useState<number | null>(null);
  return (
    <Card>
      <Title level={2}>Déroulé</Title>
      <View style={{ gap: space[5] }}>
        {variant.phases.map((phase, i) => (
          <View key={`${variant.id}-${i}`} style={{ flexDirection: 'row', gap: space[4] }}>
            <Text style={{ width: 36, fontFamily: fonts.title, fontWeight: weights.title, fontSize: 30, lineHeight: 34, color: palette.link }}>{i + 1}</Text>
            <View style={{ flex: 1, gap: space[2] }}>
              <Row gap={space[2]}>
                <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 19, lineHeight: 25, color: palette.text }}>{phase.title}</Text>
                {phase.day_offset < 0 ? <Tag>{phase.day_offset === -1 ? 'La veille' : `${-phase.day_offset} jours avant`}</Tag> : null}
                {phase.timer_s ? <Tag>{formatDuration(Math.round(phase.timer_s / 60))}</Tag> : null}
              </Row>
              <Body size={17}>{phase.text}</Body>
              {phase.steps.length ? (
                <Pressable onPress={() => setOpen(open === i ? null : i)} accessibilityRole="button" style={{ alignSelf: 'flex-start', paddingVertical: space[1] }}>
                  <Body tone="muted" size={15} style={{ textDecorationLine: 'underline' }}>
                    {open === i ? 'Replier les gestes' : `Pas à pas : ${phase.steps.length} geste${phase.steps.length > 1 ? 's' : ''}`}
                  </Body>
                </Pressable>
              ) : null}
              {open === i ? (
                <View style={{ gap: space[2], paddingLeft: space[3], borderLeftWidth: 2, borderLeftColor: palette.line }}>
                  {phase.steps.map((step, j) => (
                    <Body key={j} size={16}>
                      {step.text}
                      {step.timer_s ? ` (${formatDuration(Math.max(1, Math.round(step.timer_s / 60)))})` : ''}
                      {step.video_url ? ' ▶ geste en vidéo' : ''}
                    </Body>
                  ))}
                </View>
              ) : null}
            </View>
          </View>
        ))}
      </View>
    </Card>
  );
}
