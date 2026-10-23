import type { Aisle, Block, MenuSummary, ShoppingItem } from '@chaudron/api';
import { aisleSubtotals, aisles, applyPending, currentMenu, dateText, type PendingOp, formatPrice, formatQuantity, formatTotal, ingredientStates, joinNames, searchKey, shortDateText, space, splitTitle, todayIso } from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { type ReactNode, useState } from 'react';
import { Platform, Pressable, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { newId, useHouseholdEvents } from '../../lib/events';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useShoppingQueue } from '../../lib/queue';
import { useCached } from '../../lib/useCached';
import { Body, Button, Card, Columns, ErrorText, Field, Loading, Pill, Progress, Row, Screen, Stack, Title } from '../../ui';
import { Icon } from '../../ui/icons';
import { NoticeLine, TextAction } from '../../ui/menu';
import { PriceRecap } from '../../ui/prices';

export default function Shopping() {
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
  return <Lists householdId={home.id} />;
}

/** Choix du menu (celui en cours par défaut), puis sa liste. */
function Lists({ householdId }: { householdId: string }) {
  const router = useRouter();
  const { menu: chosen } = useLocalSearchParams<{ menu?: string }>();
  const menus = useCached(`menus:${householdId}`, () => api.GET('/api/v1/households/{id}/menus', { params: { path: { id: householdId } } }));
  const today = todayIso();
  if (!menus.data) {
    return (
      <Screen>
        <Title>Courses</Title>
        {menus.error ? <ErrorText>{menus.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const current = chosen ?? currentMenu(menus.data, today)?.week_start;
  if (!current) {
    return (
      <Screen>
        <Title>Courses</Title>
        <Card>
          <Body>Pas encore de menu : la liste se calcule depuis le menu de la semaine.</Body>
        </Card>
      </Screen>
    );
  }
  // Les menus proches : le précédent, celui-ci et les suivants.
  const recent = [...menus.data].sort((a, b) => a.week_start.localeCompare(b.week_start)).filter((m) => (m.last_date ?? m.week_start) >= today || m.week_start === current).slice(0, 4);
  return (
    <List
      key={current}
      householdId={householdId}
      weekStart={current}
      menus={recent}
      title={menus.data.find((m) => m.week_start === current)}
      onChoose={(weekStart) => router.setParams({ menu: weekStart })}
    />
  );
}

const blockTitles: Record<Block, string> = {
  recipes: 'Pour les recettes',
  breakfast_desserts: 'Petits-déjeuners et desserts',
  special: 'Autre',
};

function List({ householdId, weekStart, menus, title, onChoose }: { householdId: string; weekStart: string; menus: MenuSummary[]; title?: MenuSummary; onChoose: (weekStart: string) => void }) {
  const list = useCached(`shopping:${householdId}:${weekStart}`, () =>
    api.GET('/api/v1/households/{id}/menus/{week_start}/shopping', { params: { path: { id: householdId, week_start: weekStart } } }),
  );
  const queue = useShoppingQueue(householdId, () => void list.reload());
  const [hideDone, setHideDone] = useState(false);
  const [pantry, setPantry] = useState(false);
  useHouseholdEvents(householdId, (event) => {
    if (event.kind === 'resync') void queue.flush();
    if (event.kind === 'resync' || event.week_start === weekStart) void list.reload();
  });

  // Ce que dit le serveur, plus ce qui attend d'être envoyé.
  const data = list.data ? applyPending(list.data, queue.pending) : undefined;
  if (!data) {
    return (
      <Screen>
        <Title>Courses</Title>
        {list.error ? <ErrorText>{list.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }

  /** L'opération s'affiche tout de suite et part dans la file : envoyée maintenant, ou au retour du réseau. */
  const send = (op: Pick<PendingOp, 'kind' | 'key' | 'label' | 'block' | 'substitution_id'>) => queue.enqueue({ ...op, op_id: newId(), at: new Date().toISOString(), week_start: weekStart });
  /** Un remplacement pour ce menu (les plats de la ligne en tiennent compte), ou le retour à l'ingrédient. */
  const substitute = (item: ShoppingItem, id: string | null) => send({ kind: 'substitute', key: item.key, substitution_id: id });
  const toggle = (item: ShoppingItem) => send({ kind: item.checked ? 'uncheck' : 'check', key: item.key });
  const add = (block: Block, label: string) => send({ kind: 'add', key: `manual:${newId()}`, label, block });
  const remove = (item: ShoppingItem) => send({ kind: 'remove', key: item.key });
  const waiting = queue.pending.length;

  const recipes = data.items.filter((i) => i.block === 'recipes' && !i.pantry);
  const stock = data.items.filter((i) => i.block === 'recipes' && i.pantry);
  const done = recipes.filter((i) => i.checked).length;
  const shown = (items: ShoppingItem[]) => (hideDone ? items.filter((i) => !i.checked) : items);
  const byAisle = new Map<Aisle, ShoppingItem[]>();
  for (const item of shown(recipes)) byAisle.set(item.aisle, [...(byAisle.get(item.aisle) ?? []), item]);
  // Sous-totaux de tout le rayon, même ce qui est masqué parce que pris.
  const subtotals = aisleSubtotals(data.items.filter((i) => i.block === 'recipes'));
  const subtotal = (section: string) => (subtotals[section] ? <Body tone="muted" size={15}>{formatTotal(subtotals[section])}</Body> : null);

  return (
    <Screen>
      <Router.Screen options={{ title: 'Courses · Chaudron' }} />
      <Stack gap={space[3]}>
        <Title>Courses</Title>
        <Body tone="muted" size={16}>
          {`${title?.title || 'Menu'}${title?.first_date && title.last_date ? `, du ${shortDateText(title.first_date)} au ${shortDateText(title.last_date)}` : ''}`}
        </Body>
        {recipes.length ? (
          <Stack gap={space[2]}>
            <Progress value={done / recipes.length} label={`${done} sur ${recipes.length} pris`} />
            <Body size={15}>{done === recipes.length ? 'Tout est pris.' : `${done} sur ${recipes.length} pris, ${recipes.length - done} à prendre`}</Body>
          </Stack>
        ) : null}
        {menus.length > 1 ? (
          <Row gap={space[2]}>
            {menus.map((m) => (
              <Pill key={m.week_start} role="radio" label={`Menu du ${shortDateText(m.week_start)}`} selected={m.week_start === weekStart} onPress={() => onChoose(m.week_start)} />
            ))}
          </Row>
        ) : null}
        <Row>
          <Pill label="Masquer ce qui est pris" selected={hideDone} onPress={() => setHideDone(!hideDone)} />
        </Row>
        {data.price ? <PriceRecap price={data.price} /> : null}
      </Stack>
      {list.offline || waiting ? (
        <Body tone="muted" size={15}>
          {[list.offline ? 'Hors ligne : la liste gardée sur cet appareil.' : null, waiting ? `${waiting} changement${waiting > 1 ? 's' : ''} en attente d’envoi.` : null].filter(Boolean).join(' ')}
        </Body>
      ) : null}
      <ErrorText>{queue.error}</ErrorText>
      <Promo items={recipes} onChoose={substitute} />
      <Columns weights={[3, 2]}>
        <Stack gap={space[5]}>
          {[...byAisle].map(([aisle, items]) => (
            <Card key={aisle} style={{ gap: space[2] }}>
              <Sticky>
                <View style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'baseline', gap: space[3] }}>
                  <Title level={2}>{aisles[aisle]}</Title>
                  {subtotal(aisle)}
                </View>
              </Sticky>
              {items.map((item, i) => (
                <ItemRow key={item.key} item={item} last={i === items.length - 1} onToggle={() => toggle(item)} onSubstitute={(id) => substitute(item, id)} />
              ))}
            </Card>
          ))}
          {recipes.length && !byAisle.size ? <Body tone="muted">Tout est pris.</Body> : null}
          {stock.length ? (
            <Card style={{ gap: space[2] }}>
              <Pressable onPress={() => setPantry(!pantry)} accessibilityRole="button" style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', minHeight: 44 }}>
                <Title level={2}>Placard, à vérifier</Title>
                <Body tone="muted" size={15}>{[pantry ? 'Replier' : `${stock.length} article${stock.length > 1 ? 's' : ''}`, subtotals.pantry ? formatTotal(subtotals.pantry) : null].filter(Boolean).join(' · ')}</Body>
              </Pressable>
              {pantry ? shown(stock).map((item, i, all) => <ItemRow key={item.key} item={item} last={i === all.length - 1} onToggle={() => toggle(item)} />) : null}
            </Card>
          ) : null}
        </Stack>
        <Stack gap={space[5]}>
          {(['breakfast_desserts', 'special'] as const).map((block) => (
            <ManualBlock key={block} title={blockTitles[block]} items={shown(data.items.filter((i) => i.block === block))} onAdd={(label) => add(block, label)} onToggle={toggle} onRemove={remove} />
          ))}
        </Stack>
      </Columns>
    </Screen>
  );
}

/**
 * « Une promo ? » : chercher un ingrédient (« lieu noir ») donne les lignes du menu qui peuvent le prendre, à la place
 * de quoi et combien.
 */
function Promo({ items, onChoose }: { items: ShoppingItem[]; onChoose: (item: ShoppingItem, id: string) => void }) {
  const [query, setQuery] = useState('');
  const needle = searchKey(query.trim());
  const matches = needle
    ? items.flatMap((item) => item.substitutes.filter((o) => searchKey(o.label).includes(needle)).map((offer) => ({ item, offer })))
    : [];
  if (!items.some((i) => i.substitutes.length)) return null;
  return (
    <Card style={{ gap: space[2], maxWidth: 720 }}>
      <Field label="Une promo, un rayon vide ?" hint="Cherche ce que tu as sous les yeux : les plats du menu qui peuvent le prendre." value={query} onChangeText={setQuery} placeholder="Lieu noir, dinde…" autoCorrect={false} />
      {needle && !matches.length ? <Body tone="muted" size={15}>Aucun plat du menu ne le prend à la place d’un ingrédient.</Body> : null}
      {matches.map(({ item, offer }) => (
        <Row key={`${item.key}-${offer.id}`} gap={space[3]}>
          <Body size={16} style={{ flex: 1 }}>
            {`${offer.label}, ${item.unit ? formatQuantity(offer.qty, item.unit) : ''} au lieu de ${(item.replaces?.label ?? item.label).toLowerCase()}${recipesText(item.recipes) ? ` (${recipesText(item.recipes)})` : ''}${offer.note ? ` : ${offer.note}` : ''}`}
          </Body>
          {item.substitution === offer.id ? <Body tone="muted" size={15}>Choisi</Body> : <Button label="Choisir" onPress={() => onChoose(item, offer.id)} />}
        </Row>
      ))}
    </Card>
  );
}

/** Une ligne : la case, l'article, la quantité ; qui l'a pris ; ce qui a changé depuis ; les remplacements. */
function ItemRow({ item, last, onToggle, onRemove, onSubstitute }: { item: ShoppingItem; last: boolean; onToggle: () => void; onRemove?: () => void; onSubstitute?: (id: string | null) => void }) {
  const { palette, fonts, weights, radius } = useTheme();
  const [swapping, setSwapping] = useState(false);
  const state = item.state === 'frozen' || item.state === 'canned' || item.state === 'cooked' ? ingredientStates[item.state].toLowerCase() : null;
  const detail = [state, item.optional ? 'facultatif' : null, recipesText(item.recipes)].filter(Boolean).join(' · ');
  const qty = item.unit ? formatQuantity(item.qty, item.unit) : item.qty != null ? String(item.qty) : '';
  return (
    <View style={{ paddingVertical: space[2], gap: space[1], borderBottomWidth: last ? 0 : 1, borderBottomColor: palette.line }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: space[3] }}>
        <Pressable
          onPress={onToggle}
          accessibilityRole="checkbox"
          accessibilityState={{ checked: item.checked }}
          accessibilityLabel={item.label}
          style={{ flex: 1, flexDirection: 'row', alignItems: 'center', gap: space[3], minHeight: 52 }}
        >
          <View style={{ width: 28, height: 28, borderRadius: Math.min(14, radius.tag), borderWidth: 2, borderColor: item.checked ? palette.actionBorder : palette.muted, backgroundColor: item.checked ? palette.action : 'transparent', alignItems: 'center', justifyContent: 'center' }}>
            {item.checked ? <Icon name="check" size={18} color={palette.onAction} strokeWidth={3} /> : null}
          </View>
          <View style={{ flex: 1, gap: 2 }}>
            <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: space[3] }}>
              <Text style={{ flex: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: item.checked ? palette.muted : palette.text, textDecorationLine: item.checked ? 'line-through' : 'none' }}>{item.label}</Text>
              <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 17, color: item.checked ? palette.muted : palette.text, fontVariant: ['tabular-nums'] }}>{qty}</Text>
              {item.price_cents != null ? <Text style={{ fontFamily: fonts.text, fontSize: 15, color: palette.muted, fontVariant: ['tabular-nums'] }}>{formatPrice(item.price_cents)}</Text> : null}
            </View>
            {detail ? <Body tone="muted" size={14}>{detail}</Body> : null}
            {item.checked && item.checked_by ? <Body tone="muted" size={14}>{`Pris par ${item.checked_by}`}</Body> : null}
            {item.grown && item.unit ? <NoticeLine notice={{ tone: 'warn', icon: 'info', text: `Le menu a changé : ${formatQuantity(item.grown, item.unit)} de plus à prendre` }} /> : null}
            {item.freeze.map((hint) =>
              hint.frozen ? (
                <Body key={hint.date} tone="muted" size={14}>{`Congelé pour ${dateText(hint.date)} (${splitTitle(hint.recipe).main}).`}</Body>
              ) : (
                <NoticeLine key={hint.date} notice={{ tone: 'warn', icon: 'fresh', text: `À congeler en rentrant : la part de ${dateText(hint.date)}` }} />
              ),
            )}
            {item.replaces ? <Body tone="muted" size={14}>{`Au lieu de ${item.replaces.label.toLowerCase()}${item.replaces.qty != null && item.unit ? `, ${formatQuantity(item.replaces.qty, item.unit)}` : ''}`}</Body> : null}
          </View>
        </Pressable>
        {onRemove ? <Button kind="quiet" label="Retirer" accessibilityLabel={`Retirer ${item.label}`} onPress={onRemove} /> : null}
      </View>
      {/* Sous la ligne, aligné sur le texte : la case garde toute la largeur. */}
      {onSubstitute && (item.substitutes.length || item.replaces) && !item.checked ? (
        <Pressable onPress={() => setSwapping(!swapping)} accessibilityRole="button" accessibilityLabel={`Remplacer ${item.label}`} style={{ alignSelf: 'flex-start', marginLeft: 28 + space[3], minHeight: 32, justifyContent: 'center' }}>
          <Body tone="muted" size={14} style={{ textDecorationLine: 'underline' }}>{swapping ? 'Fermer' : item.replaces ? 'Changer de remplacement' : 'Remplacer…'}</Body>
        </Pressable>
      ) : null}
      {swapping && onSubstitute ? (
        <View style={{ marginLeft: 28 + space[3], gap: space[2], paddingBottom: space[2] }}>
          {item.substitutes.map((o) => (
            <Pill
              key={o.id}
              role="radio"
              label={`${o.label}${item.unit ? `, ${formatQuantity(o.qty, item.unit)}` : ''}${o.note ? ` · ${o.note}` : ''}`}
              selected={item.substitution === o.id}
              onPress={() => {
                onSubstitute(o.id);
                setSwapping(false);
              }}
            />
          ))}
          {item.replaces ? (
            <Pill
              role="radio"
              label={`Revenir à : ${item.replaces.label.toLowerCase()}`}
              selected={false}
              onPress={() => {
                onSubstitute(null);
                setSwapping(false);
              }}
            />
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

/** Les recettes d'un article, en bref : « Wok de bœuf et Blanquette de dinde », « Wok de bœuf et 2 autres ». */
function recipesText(recipes: string[]): string | null {
  const names = recipes.map((r) => splitTitle(r).main);
  if (!names.length) return null;
  if (names.length <= 2) return joinNames(names);
  return `${names[0]} et ${names.length - 1} autres`;
}

/** Web : le titre du rayon reste en haut de l'écran pendant qu'on fait défiler ses articles. */
function Sticky({ children }: { children: ReactNode }) {
  const { palette } = useTheme();
  if (Platform.OS !== 'web') return <>{children}</>;
  return <View style={{ position: 'sticky', top: 0, zIndex: 1, backgroundColor: palette.card, paddingVertical: space[1] } as never}>{children}</View>;
}

/** Un bloc d'ajouts à la main (petits-déjeuners et desserts, autre). */
function ManualBlock({ title, items, onAdd, onToggle, onRemove }: { title: string; items: ShoppingItem[]; onAdd: (label: string) => void; onToggle: (item: ShoppingItem) => void; onRemove: (item: ShoppingItem) => void }) {
  const [label, setLabel] = useState('');
  const submit = () => {
    if (!label.trim()) return;
    onAdd(label.trim());
    setLabel('');
  };
  return (
    <Card style={{ gap: space[2] }}>
      <Title level={2}>{title}</Title>
      {items.map((item, i) => (
        <ItemRow key={item.key} item={item} last={i === items.length - 1} onToggle={() => onToggle(item)} onRemove={() => onRemove(item)} />
      ))}
      <Field label="Ajouter" hideLabel value={label} onChangeText={setLabel} onSubmitEditing={submit} placeholder="Ajouter un article" returnKeyType="done" />
      {label.trim() ? <TextAction label={`Ajouter « ${label.trim()} »`} onPress={submit} /> : null}
    </Card>
  );
}
