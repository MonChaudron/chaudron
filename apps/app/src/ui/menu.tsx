// Composants des menus : un plat prévu (badges, avertissements), le panneau « Déplacer vers… », la poignée pour
// glisser un plat d'un jour à l'autre.
import type { Meal, MenuSlot, MoveMode, Warning } from '@chaudron/api';
import { addDays, dayLabel, equipment as equipmentLabels, formatDuration, label as labelOf, type Notice, notices, servingsLabel, shortDateText, space, vignetteOf, weekDays } from '@chaudron/domain';
import { Link } from 'expo-router';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { PanResponder, Pressable, Text, View } from 'react-native';

import { useTheme } from '../lib/theme';
import { Icon, type IconName } from './icons';
import { Body, Button, Choice, DishName, IconButton, Pill, Row, Stack, Tag, Vignette } from './index';

export const mealLabels: Record<Meal, string> = { lunch: 'midi', dinner: 'soir' };

/** Lien vers la fiche, avec les quantités du créneau. */
export function slotHref(slot: MenuSlot) {
  return `/recettes/${slot.recipe.slug}?slot=${slot.id}&menu=${slot.week_start}` as const;
}

/** Badges d'un plat : express, phase de la veille, portions d'un autre jour. Les portions sont dans `slotMeta`. */
export function SlotBadges({ slot }: { slot: MenuSlot }) {
  if (!slot.express && slot.prep_days_before <= 0 && !slot.portions_date) return null;
  return (
    <Row gap={space[2]}>
      {slot.express ? <Tag tone="strong" icon="bolt">Express</Tag> : null}
      {slot.prep_days_before > 0 ? <Tag icon="moon">{slot.prep_days_before === 1 ? 'La veille' : `${slot.prep_days_before} jours avant`}</Tag> : null}
      {slot.portions_date ? <Tag>{`Portions du ${shortDateText(slot.portions_date)}`}</Tag> : null}
    </Row>
  );
}

const noticeIcons: Record<Notice['icon'], IconName> = { allergy: 'alert', dislike: 'dislike', fresh: 'snow', info: 'info' };

/** Une ligne d'avertissement : son icône, en rouge (allergie) ou en ambre. */
export function NoticeLine({ notice, onAccent = false }: { notice: Notice; onAccent?: boolean }) {
  const { palette, fonts, weights } = useTheme();
  const color = onAccent ? palette.onAction : notice.tone === 'alert' ? palette.alert : palette.warn;
  return (
    <View style={{ flexDirection: 'row', gap: space[2], alignItems: 'flex-start' }}>
      <View style={{ paddingTop: 2 }}>
        <Icon name={noticeIcons[notice.icon]} size={17} color={color} strokeWidth={2.2} />
      </View>
      <Text style={{ flex: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 15, lineHeight: 21, color }}>{notice.text}</Text>
    </View>
  );
}

/** Avertissements, regroupés et sans la date du repas : une ligne par sujet. */
export function Warnings({ warnings, onAccent = false }: { warnings: Warning[]; onAccent?: boolean }) {
  const lines = notices(warnings);
  if (!lines.length) return null;
  return (
    <Stack gap={space[1]}>
      {lines.map((n, i) => (
        <NoticeLine key={`${n.icon}-${i}`} notice={n} onAccent={onAccent} />
      ))}
    </Stack>
  );
}

/** « Cookeo · 20 min · 6 portions, dont 2 en boîtes » : la façon de cuisiner, le temps actif, les portions. */
export function slotMeta(slot: MenuSlot): string {
  const tool = slot.variant.label || slot.variant.equipment.map((code) => labelOf(equipmentLabels, `equipment.${code}`))[0];
  return [tool, slot.variant.active_min ? formatDuration(slot.variant.active_min) : null, servingsLabel(slot)].filter(Boolean).join(' · ');
}

/** Vignette d'un plat prévu. */
export function SlotVignette({ slot, size = 56 }: { slot: MenuSlot; size?: number }) {
  const v = vignetteOf(slot.recipe.slug, slot.variant.equipment);
  return <Vignette hue={v.hue} pictogram={v.pictogram} size={size} />;
}

/**
 * Un plat prévu : vignette, nom (vers la fiche du créneau), façon de cuisiner, badges, avertissements. `onMore` : le
 * bouton « ⋯ » à droite (déplacer…) ; `handle` : la poignée, en mode réorganisation.
 */
export function SlotSummary({ slot, warnings, handle, actions, onMore, moreOpen = false }: { slot: MenuSlot; warnings: Warning[]; handle?: ReactNode; actions?: ReactNode; onMore?: () => void; moreOpen?: boolean }) {
  const { palette, fonts } = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: space[3], alignItems: 'flex-start' }}>
      {handle}
      <Link href={slotHref(slot)} accessibilityLabel={slot.recipe.title}>
        <SlotVignette slot={slot} />
      </Link>
      <View style={{ flex: 1, gap: space[2], minWidth: 0 }}>
        <Link href={slotHref(slot)} style={{ textDecorationLine: 'none' }}>
          <DishName title={slot.recipe.title} size={17} />
        </Link>
        <Text style={{ fontFamily: fonts.text, fontSize: 14.5, color: palette.muted }}>
          {slotMeta(slot)}
          {slot.note ? ` · ${slot.note}` : ''}
        </Text>
        <SlotBadges slot={slot} />
        <Warnings warnings={warnings} />
        {actions}
      </View>
      {onMore ? <IconButton icon="more" label={`Déplacer ${slot.recipe.title}`} onPress={onMore} active={moreOpen} /> : null}
    </View>
  );
}

/** Action discrète sous un plat (« Déplacer… »), en texte, avec une cible tactile pleine. */
export function TextAction({ label, onPress }: { label: string; onPress: () => void }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <Pressable onPress={onPress} accessibilityRole="button" style={({ pressed }) => ({ alignSelf: 'flex-start', minHeight: 44, justifyContent: 'center', opacity: pressed ? 0.6 : 1 })}>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 15, color: palette.link, textDecorationLine: 'underline' }}>{label}</Text>
    </Pressable>
  );
}

/** Avertissements d'un plat (les autres concernent le repas ou le menu). */
export function warningsOf(warnings: Warning[], slotId: string) {
  return warnings.filter((w) => w.slots.includes(slotId));
}

export interface MoveRequest {
  slot: string;
  date: string;
  meal: Meal;
  mode: MoveMode;
}

const modes: { value: MoveMode; label: string }[] = [
  { value: 'insert', label: 'Intercaler' },
  { value: 'swap', label: 'Échanger' },
  { value: 'together', label: 'Même soir' },
];

/** Ce que fait chaque façon de poser un plat sur un repas déjà pris. */
export const modeHelp: Record<MoveMode, string> = {
  insert: 'Le plat prend la place ; les suivants glissent d’un jour, jusqu’au premier soir libre.',
  swap: 'Les deux plats changent de jour.',
  together: 'Les deux plats sont servis ensemble.',
};

/**
 * « Déplacer vers… » : jour, repas, et que faire si le repas est déjà pris. `occupied` dit quels plats sont prévus à
 * un repas, dans le même menu.
 */
export function MovePanel({ slot, today, occupied, onMove, onCancel, busy }: { slot: MenuSlot; today: string; occupied: (date: string, meal: Meal) => MenuSlot[]; onMove: (move: MoveRequest) => void; onCancel: () => void; busy: boolean }) {
  const shift = () => onMove({ slot: slot.id, date: addDays(slot.date, 1), meal: slot.meal, mode: 'insert' });
  const first = slot.week_start > today ? slot.week_start : today;
  const days = weekDays(first, 14).filter((d) => d >= slot.week_start);
  const [date, setDate] = useState(addDays(slot.date, 1));
  const [meal, setMeal] = useState<Meal>(slot.meal);
  const [mode, setMode] = useState<MoveMode>('insert');
  const taken = occupied(date, meal).filter((s) => s.id !== slot.id);
  const same = date === slot.date && meal === slot.meal;
  return (
    <Stack gap={space[3]}>
      <Button label="Décaler la suite d’un jour" busy={busy} onPress={shift} />
      <Body tone="muted" size={14}>Ce plat passe au lendemain ; les suivants glissent d’un jour, jusqu’au premier soir libre.</Body>
      <Body size={16}>Ou déplacer vers :</Body>
      <View accessibilityRole="radiogroup" aria-label="Jour" style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
        {days.map((d) => (
          <Pill key={d} role="radio" label={d === today ? 'Auj.' : shortDateText(d)} accessibilityLabel={dayLabel(d, today)} selected={d === date} onPress={() => setDate(d)} />
        ))}
      </View>
      <Choice label="Repas" value={meal} onChange={setMeal} options={[{ value: 'lunch', label: 'Midi' }, { value: 'dinner', label: 'Soir' }]} />
      {taken.length ? (
        <Stack gap={space[2]}>
          <Body tone="muted" size={15}>{`${dayLabel(date, today)} ${mealLabels[meal]} : ${taken.map((s) => s.recipe.title).join(', ')}.`}</Body>
          <Choice label="Si le repas est pris" value={mode} onChange={setMode} options={modes} />
          <Body tone="muted" size={14}>{modeHelp[mode]}</Body>
        </Stack>
      ) : null}
      <Row>
        <Button kind="primary" label="Déplacer" busy={busy} disabled={same} onPress={() => onMove({ slot: slot.id, date, meal, mode })} />
        <Button kind="quiet" label="Annuler" onPress={onCancel} />
      </Row>
    </Stack>
  );
}

/**
 * Poignée pour glisser un plat : on la tient et on descend ou remonte jusqu'au jour voulu. `onDrag` reçoit le
 * déplacement vertical et la position dans la fenêtre ; `onDrop` la position au lâcher.
 */
export function DragHandle({ label, onStart, onDrag, onDrop }: { label: string; onStart: () => void; onDrag: (dy: number, pageY: number) => void; onDrop: (pageY: number, dy: number) => void }) {
  const { palette, radius } = useTheme();
  const handlers = useRef({ onStart, onDrag, onDrop });
  useEffect(() => {
    handlers.current = { onStart, onDrag, onDrop };
  });
  // Les gestionnaires ne lisent `handlers` que pendant le geste, jamais au rendu.
  // eslint-disable-next-line react-hooks/refs
  const [responder] = useState(() =>
    PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponderCapture: () => true,
      onPanResponderTerminationRequest: () => false,
      onPanResponderGrant: () => handlers.current.onStart(),
      onPanResponderMove: (_, g) => handlers.current.onDrag(g.dy, g.moveY),
      onPanResponderRelease: (_, g) => handlers.current.onDrop(g.moveY || g.y0, g.dy),
      onPanResponderTerminate: (_, g) => handlers.current.onDrop(g.y0, 0),
    }),
  );
  return (
    <View
      {...responder.panHandlers}
      accessibilityLabel={label}
      accessibilityHint="Maintenir et glisser vers un autre jour"
      style={{ width: 36, minHeight: 56, alignItems: 'center', justifyContent: 'center', borderRadius: radius.button, backgroundColor: palette.chip, cursor: 'grab', userSelect: 'none', touchAction: 'none' } as never}
    >
      <Icon name="grip" size={20} color={palette.muted} />
    </View>
  );
}
