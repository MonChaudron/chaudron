// Composants des menus : un plat prévu (badges, avertissements), le panneau « Déplacer vers… », la poignée pour
// glisser un plat d'un jour à l'autre.
import type { Meal, MenuSlot, MoveMode, Warning } from '@chaudron/api';
import { addDays, dayLabel, servingsText, shortDateText, space, weekDays } from '@chaudron/domain';
import { Link } from 'expo-router';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { PanResponder, Pressable, Text, View } from 'react-native';

import { useTheme } from '../lib/theme';
import { Body, Button, Choice, Pill, Row, Stack, Tag } from './index';

export const mealLabels: Record<Meal, string> = { lunch: 'midi', dinner: 'soir' };

/** Lien vers la fiche, avec les quantités du créneau. */
export function slotHref(slot: MenuSlot) {
  return `/recettes/${slot.recipe.slug}?slot=${slot.id}&menu=${slot.week_start}` as const;
}

/** Badges d'un plat : portions, express, boîtes, phase de la veille. */
export function SlotBadges({ slot }: { slot: MenuSlot }) {
  return (
    <Row gap={space[2]}>
      <Tag>{`${servingsText(slot)} portions`}</Tag>
      {slot.express ? <Tag>Express</Tag> : null}
      {slot.prep_days_before > 0 ? <Tag>{slot.prep_days_before === 1 ? 'Se prépare la veille' : `${slot.prep_days_before} jours avant`}</Tag> : null}
      {slot.portions_date ? <Tag>{`Portions du ${shortDateText(slot.portions_date)}`}</Tag> : null}
    </Row>
  );
}

/** Avertissements, en clair. */
export function Warnings({ warnings }: { warnings: Warning[] }) {
  const { palette } = useTheme();
  if (!warnings.length) return null;
  return (
    <Stack gap={space[2]}>
      {warnings.map((w, i) => (
        <View key={`${w.kind}-${i}`} style={{ flexDirection: 'row', gap: space[2] }}>
          <Text style={{ color: palette.alert, fontSize: 16, lineHeight: 24 }} aria-hidden>
            ⚠
          </Text>
          <Body tone="alert" size={15} style={{ flex: 1 }}>
            {w.message}
          </Body>
        </View>
      ))}
    </Stack>
  );
}

/** Un plat prévu : titre (vers la fiche du créneau), variante, badges, avertissements. */
export function SlotSummary({ slot, warnings, big = false, handle, actions }: { slot: MenuSlot; warnings: Warning[]; big?: boolean; handle?: ReactNode; actions?: ReactNode }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: space[3], alignItems: 'flex-start' }}>
      {handle}
      <View style={{ flex: 1, gap: space[2], minWidth: 0 }}>
        <Link href={slotHref(slot)} style={{ fontFamily: big ? fonts.title : fonts.text, fontWeight: big ? weights.title : weights.medium, fontSize: big ? 24 : 18, lineHeight: big ? 30 : 24, color: palette.text }}>
          {slot.recipe.title}
        </Link>
        <Body tone="muted" size={15}>
          {slot.variant.label}
          {slot.note ? ` · ${slot.note}` : ''}
        </Body>
        <SlotBadges slot={slot} />
        <Warnings warnings={warnings} />
        {actions}
      </View>
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
      style={{ width: 36, minHeight: 52, alignItems: 'center', justifyContent: 'center', borderRadius: radius.button, backgroundColor: palette.bg, cursor: 'grab', userSelect: 'none', touchAction: 'none' } as never}
    >
      <Text style={{ color: palette.muted, fontSize: 22, lineHeight: 24 }} aria-hidden>
        ⠿
      </Text>
    </View>
  );
}
